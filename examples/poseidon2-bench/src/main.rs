//! CPU vs GPU proving benchmark for the Poseidon2 extension.
//!
//! Builds a guest that performs `iters` Poseidon2 permutations, then times STARK proof generation
//! with the CPU backend (`Poseidon2Rv32CpuBuilder`) and, when the `cuda` feature is enabled, the
//! GPU backend (`Poseidon2Rv32GpuBuilder`), and prints a comparison.
//!
//! Usage:
//!   cargo run --release -p poseidon2-bench -- [iters]                 # CPU only
//!   cargo run --release -p poseidon2-bench --features cuda -- [iters]  # CPU + GPU comparison

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use eyre::Result;
use openvm_build::TargetFilter;
use openvm_instructions::exe::VmExe;
use openvm_poseidon2_circuit::Poseidon2Rv32Config;
use openvm_poseidon2_transpiler::Poseidon2TranspilerExtension;
use openvm_rv32im_transpiler::{
    Rv32ITranspilerExtension, Rv32IoTranspilerExtension, Rv32MTranspilerExtension,
};
use openvm_sdk::{
    config::{AggregationSystemParams, AppConfig},
    GenericSdk, StdIn, F,
};
use openvm_stark_sdk::config::{app_params_with_100_bits_security, MAX_APP_LOG_STACKED_HEIGHT};
use openvm_transpiler::{transpiler::Transpiler, FromElf};

fn iters_from_args() -> u32 {
    std::env::args()
        .skip(1)
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000)
}

fn app_config() -> AppConfig<Poseidon2Rv32Config> {
    AppConfig::new(
        Poseidon2Rv32Config::default(),
        app_params_with_100_bits_security(MAX_APP_LOG_STACKED_HEIGHT),
    )
}

fn stdin_for(iters: u32) -> StdIn {
    let mut stdin = StdIn::default();
    stdin.write(&iters);
    stdin
}

#[derive(Default)]
struct Times {
    execute: std::time::Duration,
    prove: std::time::Duration,
}

fn main() -> Result<()> {
    let iters = iters_from_args();
    println!("=== Poseidon2 benchmark: iters = {iters} ===\n");

    let cpu = run_cpu(iters)?;

    #[cfg(feature = "cuda")]
    let gpu = run_gpu(iters)?;
    #[cfg(feature = "cuda")]
    {
        println!("--- results ---");
        println!(
            "CPU prove:  {:>10.2?}   (execute {:.2?})",
            cpu.prove, cpu.execute
        );
        println!(
            "GPU prove:  {:>10.2?}   (execute {:.2?})",
            gpu.prove, gpu.execute
        );
        let speedup = cpu.prove.as_secs_f64() / gpu.prove.as_secs_f64();
        println!("speedup (CPU/GPU): {speedup:.2}x");
    }
    #[cfg(not(feature = "cuda"))]
    {
        println!("--- results (cpu only; build with --features cuda for the GPU comparison) ---");
        println!("CPU prove:  {:10.2?}   (execute {:.2?})", cpu.prove, cpu.execute);
    }

    Ok(())
}

fn timed<F: FnOnce() -> Result<()>>(f: F) -> Result<std::time::Duration> {
    let start = Instant::now();
    f()?;
    Ok(start.elapsed())
}

/// Run the full CPU pipeline and time execute + prove.
fn run_cpu(iters: u32) -> Result<Times> {
    use openvm_poseidon2_circuit::Poseidon2Rv32CpuBuilder;
    use openvm_sdk::DefaultStarkEngine;

    let sdk = GenericSdk::<DefaultStarkEngine, Poseidon2Rv32CpuBuilder>::new_without_transpiler(
        app_config(),
        AggregationSystemParams::default(),
    )?;
    let exe = Arc::new(VmExe::from_elf(sdk.build(
            Default::default(),
            guest_dir(),
            &Some(TargetFilter {
                name: "poseidon2-bench-guest".to_string(),
                kind: "bin".to_string(),
            }),
            None,
        )?, transpiler())?);

    let execute = timed(|| {
        sdk.execute(exe.clone(), stdin_for(iters))?;
        Ok(())
    })?;
    let prove = timed(|| {
        let (proof, baseline) = sdk.prove(exe.clone(), stdin_for(iters), &[])?;
        let (_agg_pk, agg_vk) = sdk.agg_keygen();
        GenericSdk::<DefaultStarkEngine, Poseidon2Rv32CpuBuilder>::verify_proof(
            agg_vk, baseline, &proof,
        )?;
        Ok(())
    })?;
    println!("CPU: execute {execute:.2?}, prove+verify {prove:.2?}");
    Ok(Times { execute, prove })
}

#[cfg(feature = "cuda")]
/// Run the full GPU pipeline and time execute + prove.
fn run_gpu(iters: u32) -> Result<Times> {
    use openvm_cuda_backend::BabyBearPoseidon2GpuEngine as GpuEngine;
    use openvm_poseidon2_circuit::Poseidon2Rv32GpuBuilder;

    let sdk = GenericSdk::<GpuEngine, Poseidon2Rv32GpuBuilder>::new_without_transpiler(
        app_config(),
        AggregationSystemParams::default(),
    )?;
    let exe = Arc::new(VmExe::from_elf(sdk.build(
            Default::default(),
            guest_dir(),
            &Some(TargetFilter {
                name: "poseidon2-bench-guest".to_string(),
                kind: "bin".to_string(),
            }),
            None,
        )?, transpiler())?);

    let execute = timed(|| {
        sdk.execute(exe.clone(), stdin_for(iters))?;
        Ok(())
    })?;
    let prove = timed(|| {
        let (proof, baseline) = sdk.prove(exe.clone(), stdin_for(iters), &[])?;
        let (_agg_pk, agg_vk) = sdk.agg_keygen();
        GenericSdk::<GpuEngine, Poseidon2Rv32GpuBuilder>::verify_proof(agg_vk, baseline, &proof)?;
        Ok(())
    })?;
    println!("GPU: execute {execute:.2?}, prove+verify {prove:.2?}");
    Ok(Times { execute, prove })
}

/// Directory of the benchmark guest package.
fn guest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guest")
}

/// Build a transpiler with the poseidon2 + rv32im + io extensions.
fn transpiler() -> Transpiler<F> {
    Transpiler::<F>::default()
        .with_extension(Poseidon2TranspilerExtension)
        .with_extension(Rv32ITranspilerExtension)
        .with_extension(Rv32MTranspilerExtension)
        .with_extension(Rv32IoTranspilerExtension)
}