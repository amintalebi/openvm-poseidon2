// CUDA tracegen for the Poseidon2 periphery AIR (`Poseidon2PeripheryAir`).
//
// One row is generated per (input -> output) permutation produced by the adapter chip's `PERMUTE`
// evaluation. The adapter kernel appends the recomposed 16 input words for every executed
// `PERMUTE` into a device records buffer; this kernel reads those records and, for each one,
// expands the full `Poseidon2SubCols` subtrace (via `poseidon2::generate_trace_row_for_perm`) and
// stamps the multiplicity column with 1. Padding rows (up to the next power of two) use a zero
// input and a multiplicity of 0, which satisfies the sub-AIR (output = permute(000..0)).
//
// The column layout written here exactly matches the CPU `Poseidon2PeripheryChip`: a
// `Poseidon2SubCols` subrow followed by a single `mult` column.

#include "fp.h"
#include "launcher.cuh"
#include "poseidon2-air/params.cuh"
#include "poseidon2-air/tracegen.cuh"
#include "primitives/trace_access.h"

#include <cassert>
#include <cstdint>
#include <cstddef>

// WIDTH is always 16 and the permutation is the default BabyBear Poseidon2 (degree 7, 4 half-full
// rounds, 13 partial rounds, 1 sbox register) matching `SBOX_REGISTERS = 1` and
// `Poseidon2Config::default()` used by both the CPU periphery chip and the AIR.
constexpr size_t POSEIDON2_WIDTH = 16;

template <size_t WIDTH, typename PoseidonParams>
__global__ void poseidon2_periphery_tracegen_kernel(
    Fp *d_trace,
    size_t trace_height,
    const uint32_t *d_records, // num_records rows of WIDTH canonical input words
    size_t num_records
) {
    uint32_t idx = blockIdx.x * blockDim.x + threadIdx.x;

    using Poseidon2Row = poseidon2::Poseidon2Row<
        WIDTH,
        PoseidonParams::SBOX_DEGREE,
        PoseidonParams::SBOX_REGS,
        PoseidonParams::HALF_FULL_ROUNDS,
        PoseidonParams::PARTIAL_ROUNDS>;

    if (idx >= trace_height) {
        return;
    }

    // Build the input state for this row (real record, else the zero dummy row).
    Fp state[WIDTH];
    if (idx < num_records) {
#pragma unroll
        for (size_t i = 0; i < WIDTH; i++) {
            state[i] = Fp(d_records[idx * WIDTH + i]);
        }
    } else {
#pragma unroll
        for (size_t i = 0; i < WIDTH; i++) {
            state[i] = Fp::zero();
        }
    }

    RowSlice state_slice(state, 1);
    Poseidon2Row row(d_trace + idx, trace_height);
    poseidon2::generate_trace_row_for_perm(row, state_slice);

    // Multiplicity column, right after the Poseidon2SubCols subrow.
    d_trace[idx + Poseidon2Row::get_total_size() * trace_height] =
        (idx < num_records) ? Fp::one() : Fp::zero();
}

extern "C" int _poseidon2_periphery_tracegen(
    Fp *d_trace,
    size_t trace_height,
    size_t trace_width,
    const uint32_t *d_records,
    size_t num_records,
    cudaStream_t stream
) {
    assert((trace_height & (trace_height - 1)) == 0);

    using Poseidon2Row = poseidon2::Poseidon2Row<
        POSEIDON2_WIDTH,
        Poseidon2ParamsS1::SBOX_DEGREE,
        Poseidon2ParamsS1::SBOX_REGS,
        Poseidon2ParamsS1::HALF_FULL_ROUNDS,
        Poseidon2ParamsS1::PARTIAL_ROUNDS>;
    static_assert(
        Poseidon2Row::get_total_size() + 1 == trace_width,
        "Poseidon2 periphery trace width must be Poseidon2SubCols + mult"
    );

    auto [grid, block] = kernel_launch_params(trace_height, 256);
    poseidon2_periphery_tracegen_kernel<POSEIDON2_WIDTH, Poseidon2ParamsS1>
        <<<grid, block, 0, stream>>>(d_trace, trace_height, d_records, num_records);
    return CHECK_KERNEL();
}