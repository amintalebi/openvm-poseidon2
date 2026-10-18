// Builds the extension's CUDA tracegen kernels with `openvm-cuda-builder` when the `cuda` feature
// is enabled and nvcc is available (e.g. on a GPU instance). Skipped otherwise.

#[cfg(feature = "cuda")]
use openvm_cuda_builder::{cuda_available, CudaBuilder};

fn main() {
    #[cfg(feature = "cuda")]
    {
        if !cuda_available() {
            // nvcc not on PATH (e.g. a CPU-only machine); skip CUDA compilation. Attempting to link
            // the cuda-provided crates will fail fast downstream with a clear error.
            return;
        }

        let builder: CudaBuilder = CudaBuilder::new()
            .include_from_dep("DEP_CUDA_COMMON_INCLUDE")
            .include("cuda/include")
            .watch("cuda")
            .library_name("tracegen_gpu_poseidon2")
            .files_from_glob("cuda/src/*.cu");

        builder.emit_link_directives();
        builder.build();
    }
}