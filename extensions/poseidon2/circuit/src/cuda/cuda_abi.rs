#![allow(clippy::missing_safety_doc)]

//! Rust bindings for the extension's CUDA tracegen kernels (compiled by `openvm-cuda-builder`).

use openvm_cuda_backend::prelude::F;
use openvm_cuda_common::{d_buffer::DeviceBuffer, error::CudaError, stream::cudaStream_t};

/// Kernels for the Poseidon2 adapter AIR (`Poseidon2PermuteAir`).
pub mod poseidon2_permute {
    use super::*;

    extern "C" {
        fn _poseidon2_permute_tracegen(
            d_trace: *mut F,
            trace_height: usize,
            trace_width: usize,
            d_records: *const u8,
            num_records: usize,
            d_range_checker: *mut u32,
            range_checker_num_bins: u32,
            d_bitwise_lookup: *mut u32,
            bitwise_num_bits: usize,
            pointer_max_bits: u32,
            timestamp_max_bits: u32,
            d_periphery_records: *mut u32,
            stream: cudaStream_t,
        ) -> i32;
    }

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn tracegen(
        d_trace: &DeviceBuffer<F>,
        trace_height: usize,
        trace_width: usize,
        d_records: &DeviceBuffer<u8>,
        num_records: usize,
        d_range_checker: &DeviceBuffer<F>,
        d_bitwise_lookup: &DeviceBuffer<F>,
        pointer_max_bits: u32,
        timestamp_max_bits: u32,
        d_periphery_records: &DeviceBuffer<F>,
        stream: cudaStream_t,
    ) -> Result<(), CudaError> {
        let result = _poseidon2_permute_tracegen(
            d_trace.as_mut_ptr(),
            trace_height,
            trace_width,
            d_records.as_ptr(),
            num_records,
            d_range_checker.as_mut_ptr() as *mut u32,
            d_range_checker.len() as u32,
            d_bitwise_lookup.as_mut_ptr() as *mut u32,
            8, // RV32_CELL_BITS
            pointer_max_bits,
            timestamp_max_bits,
            d_periphery_records.as_mut_ptr() as *mut u32,
            stream,
        );
        CudaError::from_result(result)
    }
}

/// Kernels for the Poseidon2 periphery AIR (`Poseidon2PeripheryAir`).
pub mod poseidon2_periphery {
    use super::*;

    extern "C" {
        fn _poseidon2_periphery_tracegen(
            d_trace: *mut F,
            trace_height: usize,
            trace_width: usize,
            d_records: *const F,
            num_records: usize,
            stream: cudaStream_t,
        ) -> i32;
    }

    pub unsafe fn tracegen(
        d_trace: &DeviceBuffer<F>,
        trace_height: usize,
        trace_width: usize,
        d_records: &DeviceBuffer<F>,
        num_records: usize,
        stream: cudaStream_t,
    ) -> Result<(), CudaError> {
        let result = _poseidon2_periphery_tracegen(
            d_trace.as_mut_ptr(),
            trace_height,
            trace_width,
            d_records.as_ptr(),
            num_records,
            stream,
        );
        CudaError::from_result(result)
    }
}
