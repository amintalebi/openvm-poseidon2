//! End-to-end example: build the guest program, run it, and produce + verify a STARK proof,
//! all using the `Poseidon2Rv32Config` shipped by the extension.
//!
//! By default it proves with the CPU backend (`Poseidon2Rv32CpuBuilder`). With the `cuda` feature
//! enabled you can pass `--gpu` to prove with the CUDA backend (`Poseidon2Rv32GpuBuilder`, via
//! `Poseidon2GpuProverExt`) instead:
//!
//!   cargo run --profile fast -p poseidon2-example --features cuda -- --gpu
//!
//! The guest program itself lives in `guest/`; it exercises `hash_u32s`, `hash_bytes`, and
//! `permute` and prints its results.

use std::path::PathBuf;

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
// The CPU Stark engine: under `cuda` the SDK re-exports the CPU engine as
// `BabyBearPoseidon2Engine`; otherwise it is `DefaultStarkEngine`.
#[cfg(feature = "cuda")]
use openvm_sdk::BabyBearPoseidon2Engine as CpuEngine;
#[cfg(not(feature = "cuda"))]
use openvm_sdk::DefaultStarkEngine as CpuEngine;
use openvm_stark_sdk::config::{app_params_with_100_bits_security, MAX_APP_LOG_STACKED_HEIGHT};
use openvm_transpiler::{transpiler::Transpiler, FromElf};

fn main() -> Result<()> {
    let use_gpu = std::env::args().any(|a| a == "--gpu");
    if use_gpu {
        #[cfg(feature = "cuda")]
        {
            return run_gpu();
        }
        #[cfg(not(feature = "cuda"))]
        {
            eprintln!(
                "--gpu requested but the `cuda` feature is not enabled (build with --features cuda)."
            );
            std::process::exit(2);
        }
    }
    run_cpu()
}

fn app_config() -> AppConfig<Poseidon2Rv32Config> {
    AppConfig::new(
        Poseidon2Rv32Config::default(),
        app_params_with_100_bits_security(MAX_APP_LOG_STACKED_HEIGHT),
    )
}

fn guest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guest")
}

fn transpiler() -> Transpiler<F> {
    Transpiler::<F>::default()
        .with_extension(Poseidon2TranspilerExtension)
        .with_extension(Rv32ITranspilerExtension)
        .with_extension(Rv32MTranspilerExtension)
        .with_extension(Rv32IoTranspilerExtension)
}

/// CPU proving backend.
fn run_cpu() -> Result<()> {
    use openvm_poseidon2_circuit::Poseidon2Rv32CpuBuilder;

    let sdk = GenericSdk::<CpuEngine, Poseidon2Rv32CpuBuilder>::new_without_transpiler(
        app_config(),
        AggregationSystemParams::default(),
    )?;
    let elf = sdk.build(
        Default::default(),
        guest_dir(),
        &Some(TargetFilter {
            name: "poseidon2-example-guest".to_string(),
            kind: "bin".to_string(),
        }),
        None,
    )?;
    let exe = std::sync::Arc::new(VmExe::from_elf(elf, transpiler())?);

    let public_values = sdk.execute(exe.clone(), StdIn::default())?;
    println!("public values: {public_values:02x?}");

    println!("proving (cpu)...");
    let (proof, baseline) = sdk.prove(exe.clone(), StdIn::default(), &[])?;
    let (_agg_pk, agg_vk) = sdk.agg_keygen();
    GenericSdk::<CpuEngine, Poseidon2Rv32CpuBuilder>::verify_proof(
        agg_vk, baseline, &proof,
    )?;
    println!("cpu proof verified!");
    Ok(())
}

#[cfg(feature = "cuda")]
/// CUDA proving backend.
fn run_gpu() -> Result<()> {
    use openvm_cuda_backend::BabyBearPoseidon2GpuEngine as GpuEngine;
    use openvm_poseidon2_circuit::Poseidon2Rv32GpuBuilder;

    let sdk = GenericSdk::<GpuEngine, Poseidon2Rv32GpuBuilder>::new_without_transpiler(
        app_config(),
        AggregationSystemParams::default(),
    )?;
    let elf = sdk.build(
        Default::default(),
        guest_dir(),
        &Some(TargetFilter {
            name: "poseidon2-example-guest".to_string(),
            kind: "bin".to_string(),
        }),
        None,
    )?;
    let exe = std::sync::Arc::new(VmExe::from_elf(elf, transpiler())?);

    let public_values = sdk.execute(exe.clone(), StdIn::default())?;
    println!("public values: {public_values:02x?}");

    println!("proving (gpu)...");
    let (proof, baseline) = sdk.prove(exe.clone(), StdIn::default(), &[])?;
    let (_agg_pk, agg_vk) = sdk.agg_keygen();
    GenericSdk::<GpuEngine, Poseidon2Rv32GpuBuilder>::verify_proof(agg_vk, baseline, &proof)?;
    println!("gpu proof verified!");
    Ok(())
}