// CUDA tracegen for the Poseidon2 adapter AIR (`Poseidon2PermuteAir`).
//
// Each executed `PERMUTE` instruction is a single `Poseidon2PermuteRecord`. For every record this
// kernel
//   1. recomposes the 16 little-endian preimage words into field elements,
//   2. appends those input words to the periphery's device records buffer (so the periphery chip
//      can expand the full permutation subtrace),
//   3. computes the postimage (the permutation output) using the shared Poseidon2-air library,
//   4. fills the adapter trace row: pc / is_valid / timestamp / rd_ptr / buffer_ptr limbs / preimage
//      / postimage bytes, the memory-aux columns (rd read + 16 buffer word writes), the canonicity
//      aux columns, and the bitwise-lookup histogram counts.
//
// Dummy rows (from `num_records` up to the padded trace height) are left zeroed, which satisfies
// the AIR (all bus sends are gated by `is_valid`).
//
// The scalar bitwise lookups and the memory-aux generation mirror the CPU tracegen exactly so the
// per-key histogram counts are identical.

#include "fp.h"
#include "launcher.cuh"
#include "poseidon2-air/params.cuh"
#include "poseidon2-air/tracegen.cuh"
#include "primitives/constants.h"
#include "primitives/histogram.cuh"
#include "primitives/trace_access.h"
#include "system/memory/controller.cuh"
#include "system/memory/offline_checker.cuh"

#include <cassert>
#include <cstddef>
#include <cstdint>

using namespace riscv;

constexpr size_t POSEIDON2_WIDTH = 16;
constexpr size_t POSEIDON2_STATE_BYTES = 64;
constexpr size_t POSEIDON2_WORD_SIZE = 4;
constexpr size_t F_NUM_BYTES = 4;

// Must match the Rust `#[repr(C)] Poseidon2PermuteRecord` exactly (148 bytes).
struct Poseidon2PermuteRecord {
    uint32_t pc;
    uint32_t timestamp;
    uint32_t rd_ptr;
    uint32_t buffer_ptr;
    uint32_t rd_aux_prev_timestamp;
    uint32_t buffer_word_aux_prev_timestamp[POSEIDON2_WIDTH];
    uint8_t preimage_buffer_bytes[POSEIDON2_STATE_BYTES];
};

// Must match the Rust `[repr(C)] CanonicityAuxCols<T>` (diff_marker[4], diff_val).
template <typename T> struct CanonicityAuxCols {
    T diff_marker[F_NUM_BYTES];
    T diff_val;
};

// Mirrors the Rust `Poseidon2PermuteOpCols<T>` layout. Only used for `COL_INDEX`/`sizeof`.
template <typename T> struct Poseidon2PermuteOpCols {
    T pc;
    T is_valid;
    T timestamp;
    T rd_ptr;
    T buffer_ptr_limbs[RV32_REGISTER_NUM_LIMBS];
    T preimage[POSEIDON2_STATE_BYTES];
    T postimage[POSEIDON2_STATE_BYTES];
    MemoryReadAuxCols<T> rd_aux;
    MemoryBaseAuxCols<T> buffer_word_aux[POSEIDON2_WIDTH];
    CanonicityAuxCols<T> postimage_canonicity_aux[POSEIDON2_WIDTH];
};

#define COL_WRITE_VALUE2(ROW, STRUCT, FIELD, VALUE) COL_WRITE_VALUE(ROW, STRUCT, FIELD, VALUE)
#define COL_WRITE_ARRAY2(ROW, STRUCT, FIELD, VALUES) COL_WRITE_ARRAY(ROW, STRUCT, FIELD, VALUES)

/// Byte-level canonicity subrow generator (mirrors `CanonicityTraceGen::generate_subrow`).
/// Returns the value to send to an 8-bit range check (`diff - 1`).
__device__ __forceinline__ uint32_t canonicity_generate_subrow(
    const uint8_t x_le[F_NUM_BYTES],
    RowSlice aux
) {
    // Big-endian byte decomposition of BabyBear ORDER_U32 = 0x78000001.
    static constexpr uint8_t ORDER_BE[F_NUM_BYTES] = {0x78, 0x00, 0x00, 0x01};

    uint32_t to_range_check = 0;
    bool found = false;
    for (size_t i = 0; i < F_NUM_BYTES; i++) {
        uint32_t x = x_le[F_NUM_BYTES - 1 - i];
        uint32_t y = ORDER_BE[i];
        if (!found && x != y) {
            // Canonicity guarantees x < y.
            uint32_t diff = y - x;
            aux.write(i, Fp::one());      // diff_marker[i]
            aux.write(F_NUM_BYTES, diff); // diff_val
            to_range_check = diff - 1;
            found = true;
        } else {
            aux.write(i, Fp::zero()); // diff_marker[i]
        }
    }
    (void)found;
    return to_range_check;
}

// Fill the single trace row for one Poseidon2PermuteRecord.
// Marked __noinline__ so the working set (Poseidon2 state, MemoryAuxColsFactory,
// BitwiseOperationLookup) lives in this helper's own stack frame.
static __device__ __noinline__ void fill_poseidon2_permute_row(
    RowSlice row,
    Poseidon2PermuteRecord const &rec,
    uint32_t *d_range_checker_ptr,
    uint32_t range_checker_num_bins,
    uint32_t *d_bitwise_lookup_ptr,
    size_t bitwise_num_bits,
    uint32_t pointer_max_bits,
    uint32_t timestamp_max_bits,
    uint32_t *d_periphery_records // appended per record: POSEIDON2_WIDTH canonical input words
) {
    MemoryAuxColsFactory mem_helper(
        VariableRangeChecker(d_range_checker_ptr, range_checker_num_bins), timestamp_max_bits
    );
    BitwiseOperationLookup bitwise_lookup(d_bitwise_lookup_ptr, bitwise_num_bits);

    // 1. Recompute the 16 preimage words and record them for the periphery chip.
    Fp state[POSEIDON2_WIDTH];
#pragma unroll
    for (size_t i = 0; i < POSEIDON2_WIDTH; i++) {
        const uint8_t *b = rec.preimage_buffer_bytes + i * POSEIDON2_WORD_SIZE;
        state[i] = Fp(u32_from_bytes_le(b));
    }
#pragma unroll
    for (size_t i = 0; i < POSEIDON2_WIDTH; i++) {
        d_periphery_records[i] = state[i].asUInt32();
    }

    // 2. Compute the postimage (permutation output) in place. A null row means we only want the
    //    output state, not the full subtrace (the periphery chip materializes the subtrace).
    RowSlice state_slice(state, 1);
    poseidon2::generate_trace_row_for_perm(Poseidon2Row::null(), state_slice);

    // 3. Decompose the output words into canonical little-endian bytes.
    uint8_t postimage[POSEIDON2_STATE_BYTES];
#pragma unroll
    for (size_t i = 0; i < POSEIDON2_WIDTH; i++) {
        uint32_t w = state[i].asUInt32();
        postimage[i * POSEIDON2_WORD_SIZE + 0] = w & 0xFF;
        postimage[i * POSEIDON2_WORD_SIZE + 1] = (w >> 8) & 0xFF;
        postimage[i * POSEIDON2_WORD_SIZE + 2] = (w >> 16) & 0xFF;
        postimage[i * POSEIDON2_WORD_SIZE + 3] = (w >> 24) & 0xFF;
    }

    // 4. Fill the scalar columns.
    COL_WRITE_VALUE2(row, Poseidon2PermuteOpCols, pc, rec.pc);
    COL_WRITE_VALUE2(row, Poseidon2PermuteOpCols, is_valid, 1);
    COL_WRITE_VALUE2(row, Poseidon2PermuteOpCols, timestamp, rec.timestamp);
    COL_WRITE_VALUE2(row, Poseidon2PermuteOpCols, rd_ptr, rec.rd_ptr);

    uint8_t buffer_ptr_limbs[RV32_REGISTER_NUM_LIMBS];
    buffer_ptr_limbs[0] = rec.buffer_ptr & 0xFF;
    buffer_ptr_limbs[1] = (rec.buffer_ptr >> 8) & 0xFF;
    buffer_ptr_limbs[2] = (rec.buffer_ptr >> 16) & 0xFF;
    buffer_ptr_limbs[3] = (rec.buffer_ptr >> 24) & 0xFF;
    COL_WRITE_ARRAY2(row, Poseidon2PermuteOpCols, buffer_ptr_limbs, buffer_ptr_limbs);

    COL_WRITE_ARRAY2(row, Poseidon2PermuteOpCols, preimage, rec.preimage_buffer_bytes);
    COL_WRITE_ARRAY2(row, Poseidon2PermuteOpCols, postimage, postimage);

    // 5. Memory aux columns: one read (rd) + POSEIDON2_WIDTH writes (buffer words).
    uint32_t ts = rec.timestamp;
    mem_helper.fill(row.slice_from(COL_INDEX(Poseidon2PermuteOpCols, rd_aux)), rec.rd_aux_prev_timestamp, ts);
    ts++;
    for (size_t w = 0; w < POSEIDON2_WIDTH; w++) {
        mem_helper.fill(
            row.slice_from(COL_INDEX(Poseidon2PermuteOpCols, buffer_word_aux[w])),
            rec.buffer_word_aux_prev_timestamp[w],
            ts
        );
        ts++;
    }

    // 6. Bitwise lookups: buffer pointer MSL limb pair (identical to the CPU tracegen).
    constexpr uint32_t RV32_TOTAL_BITS = RV32_CELL_BITS * RV32_REGISTER_NUM_LIMBS;
    uint32_t scaled_limb = (uint32_t)buffer_ptr_limbs[RV32_REGISTER_NUM_LIMBS - 1]
                           << (RV32_TOTAL_BITS - pointer_max_bits);
    bitwise_lookup.add_range(scaled_limb, scaled_limb);

    // Postimage byte pairs.
    for (size_t i = 0; i < POSEIDON2_STATE_BYTES; i += 2) {
        bitwise_lookup.add_range(postimage[i], postimage[i + 1]);
    }

    // 7. Canonicity aux columns + their range-check pairs.
    uint32_t canonicity_rcs[POSEIDON2_WIDTH];
#pragma unroll
    for (size_t i = 0; i < POSEIDON2_WIDTH; i++) {
        RowSlice aux_slice =
            row.slice_from(COL_INDEX(Poseidon2PermuteOpCols, postimage_canonicity_aux[i]));
        canonicity_rcs[i] = canonicity_generate_subrow(&postimage[i * POSEIDON2_WORD_SIZE], aux_slice);
    }
    for (size_t i = 0; i < POSEIDON2_WIDTH; i += 2) {
        bitwise_lookup.add_range(canonicity_rcs[i], canonicity_rcs[i + 1]);
    }
}

// Main kernel: one thread per record (= one trace row, since NUM_OP_ROWS_PER_INS == 1).
__global__ void poseidon2_permute_tracegen_kernel(
    Fp *d_trace,
    size_t trace_height,
    const Poseidon2PermuteRecord *d_records,
    size_t num_records,
    uint32_t *d_range_checker_ptr,
    uint32_t range_checker_num_bins,
    uint32_t *d_bitwise_lookup_ptr,
    size_t bitwise_num_bits,
    uint32_t pointer_max_bits,
    uint32_t timestamp_max_bits,
    uint32_t *d_periphery_records
) {
    uint32_t record_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (record_idx >= trace_height) {
        return;
    }

    RowSlice row(d_trace + record_idx, trace_height);
    // Zero the whole row first (also covers dummy rows).
    row.fill_zero(0, sizeof(Poseidon2PermuteOpCols<uint8_t>));

    if (record_idx < num_records) {
        fill_poseidon2_permute_row(
            row,
            d_records[record_idx],
            d_range_checker_ptr,
            range_checker_num_bins,
            d_bitwise_lookup_ptr,
            bitwise_num_bits,
            pointer_max_bits,
            timestamp_max_bits,
            d_periphery_records + record_idx * POSEIDON2_WIDTH
        );
    }
}

extern "C" int _poseidon2_permute_tracegen(
    Fp *d_trace,
    size_t trace_height,
    size_t trace_width,
    const Poseidon2PermuteRecord *d_records,
    size_t num_records,
    uint32_t *d_range_checker_ptr,
    uint32_t range_checker_num_bins,
    uint32_t *d_bitwise_lookup_ptr,
    size_t bitwise_num_bits,
    uint32_t pointer_max_bits,
    uint32_t timestamp_max_bits,
    uint32_t *d_periphery_records,
    cudaStream_t stream
) {
    assert((trace_height & (trace_height - 1)) == 0);
    assert(trace_width == sizeof(Poseidon2PermuteOpCols<uint8_t>));

    auto [grid, block] = kernel_launch_params(trace_height, 256);
    poseidon2_permute_tracegen_kernel<<<grid, block, 0, stream>>>(
        d_trace,
        trace_height,
        d_records,
        num_records,
        d_range_checker_ptr,
        range_checker_num_bins,
        d_bitwise_lookup_ptr,
        bitwise_num_bits,
        pointer_max_bits,
        timestamp_max_bits,
        d_periphery_records
    );
    return CHECK_KERNEL();
}

#undef COL_WRITE_VALUE2
#undef COL_WRITE_ARRAY2