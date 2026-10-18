//! GPU (`GpuBackend`) chip implementations for the Poseidon2 extension.
//!
//! These mirror the CpuBackend chips in [`crate::permute`] and [`crate::periphery`] but produce
//! their traces on-device with CUDA kernels (compiled by `openvm-cuda-builder`). Preflight
//! execution still runs on the host and produces the same `Poseidon2PermuteRecord`s; only trace
//! generation moves to the GPU.

use std::{
    mem::size_of,
    sync::{Arc, Mutex},
};

use derive_new::new;
use openvm_circuit::{arch::DenseRecordArena, utils::next_power_of_two_or_zero};
use openvm_circuit_primitives::{
    bitwise_op_lookup::BitwiseOperationLookupChipGPU, var_range::VariableRangeCheckerChipGPU, Chip,
};
use openvm_cuda_backend::{base::DeviceMatrix, prelude::F, GpuBackend};
use openvm_cuda_common::{copy::MemCopyH2D, d_buffer::DeviceBuffer, stream::GpuDeviceCtx};
use openvm_poseidon2_air::POSEIDON2_WIDTH;
use openvm_stark_backend::prover::AirProvingContext;

use crate::{
    periphery::Poseidon2PeripheryCols,
    permute::{columns::NUM_POSEIDON2_PERMUTE_OP_COLS, trace::Poseidon2PermuteRecord},
};

mod cuda_abi;

/// Shared device records buffer produced by the adapter chip and consumed by the periphery chip.
/// The adapter is trace-generated *first* (it is inserted after the periphery, and chips are
/// trace-generated in reverse insertion order); it writes the recomposed input state for every
/// executed `PERMUTE` here, then the periphery chip expands the full permutation subtrace from it.
#[derive(Default)]
pub struct SharedPoseidon2Records {
    /// `num_records * POSEIDON2_WIDTH` canonical input words in execution order.
    pub records: Option<Arc<DeviceBuffer<F>>>,
    pub num_records: usize,
}

pub type SharedPoseidon2RecordsGpu = Arc<Mutex<SharedPoseidon2Records>>;

/// GPU counterpart of [`crate::permute::Poseidon2PermuteChip`] (the adapter AIR).
#[derive(new)]
pub struct Poseidon2PermuteChipGpu {
    pub range_checker: Arc<VariableRangeCheckerChipGPU>,
    pub bitwise_lookup: Arc<BitwiseOperationLookupChipGPU<8>>,
    pub pointer_max_bits: usize,
    pub timestamp_max_bits: u32,
    pub shared_records: SharedPoseidon2RecordsGpu,
}

impl Chip<DenseRecordArena, GpuBackend> for Poseidon2PermuteChipGpu {
    fn generate_proving_ctx(&self, arena: DenseRecordArena) -> AirProvingContext<GpuBackend> {
        const RECORD_SIZE: usize = size_of::<Poseidon2PermuteRecord>();
        let records = arena.allocated();
        let device_ctx = &self.range_checker.device_ctx;

        if records.is_empty() {
            let mut shared = self.shared_records.lock().unwrap();
            shared.records = None;
            shared.num_records = 0;
            return AirProvingContext::simple_no_pis(DeviceMatrix::dummy());
        }
        debug_assert_eq!(records.len() % RECORD_SIZE, 0);

        let num_records = records.len() / RECORD_SIZE;
        let trace_height = next_power_of_two_or_zero(num_records);
        let trace_width = NUM_POSEIDON2_PERMUTE_OP_COLS;

        let d_records = records.to_device_on(device_ctx).unwrap();
        let d_trace = DeviceMatrix::<F>::with_capacity_on(trace_height, trace_width, device_ctx);

        // Buffer into which the adapter kernel writes the recomposed input words for each record;
        // consumed by the periphery chip (trace-generated after this chip).
        let d_periphery_records = Arc::new(DeviceBuffer::<F>::with_capacity_on(
            num_records * POSEIDON2_WIDTH,
            device_ctx,
        ));

        unsafe {
            cuda_abi::poseidon2_permute::tracegen(
                d_trace.buffer(),
                trace_height,
                trace_width,
                &d_records,
                num_records,
                &self.range_checker.count,
                &self.bitwise_lookup.count,
                self.pointer_max_bits as u32,
                self.timestamp_max_bits,
                &d_periphery_records,
                device_ctx.stream.as_raw(),
            )
            .unwrap();
        }

        {
            let mut shared = self.shared_records.lock().unwrap();
            shared.records = Some(d_periphery_records);
            shared.num_records = num_records;
        }

        AirProvingContext::simple_no_pis(d_trace)
    }
}

/// GPU counterpart of [`crate::periphery::Poseidon2PeripheryChip`] (the periphery AIR).
#[derive(new)]
pub struct Poseidon2PeripheryChipGpu {
    pub shared_records: SharedPoseidon2RecordsGpu,
    pub device_ctx: GpuDeviceCtx,
}

impl Chip<DenseRecordArena, GpuBackend> for Poseidon2PeripheryChipGpu {
    fn generate_proving_ctx(&self, _: DenseRecordArena) -> AirProvingContext<GpuBackend> {
        let (records, num_records) = {
            let mut shared = self.shared_records.lock().unwrap();
            match shared.records.take() {
                Some(r) => (Some(r), shared.num_records),
                None => (None, 0),
            }
        };

        let Some(records) = records else {
            return AirProvingContext::simple_no_pis(DeviceMatrix::dummy());
        };
        if num_records == 0 {
            return AirProvingContext::simple_no_pis(DeviceMatrix::dummy());
        }

        let trace_height = next_power_of_two_or_zero(num_records);
        let trace_width = Poseidon2PeripheryCols::<F>::width();

        let d_trace =
            DeviceMatrix::<F>::with_capacity_on(trace_height, trace_width, &self.device_ctx);
        unsafe {
            cuda_abi::poseidon2_periphery::tracegen(
                d_trace.buffer(),
                trace_height,
                trace_width,
                &records,
                num_records,
                self.device_ctx.stream.as_raw(),
            )
            .unwrap();
        }
        AirProvingContext::simple_no_pis(d_trace)
    }
}
