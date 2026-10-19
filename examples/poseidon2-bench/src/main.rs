//! CPU vs GPU proving benchmark for the Poseidon2 extension.
//!
//! Builds a guest that performs `iters` Poseidon2 permutations, then times STARK proof generation
//! with the CPU backend (`Poseidon2Rv32CpuBuilder`) and, when the `cuda` feature is enabled, the
//! GPU backend (`Poseidon2Rv32GpuBuilder`), and prints a comparison. One-time keygen is done
//! before the proving timer and reported separately.
//!
//! Usage:
//!   poseidon2-bench [--cpu | --gpu] [--log-height=N] [iters]
//!
//!   iters            number of permutations in the guest (default 20000)
//!   --cpu            run only the CPU backend
//!   --gpu            run only the GPU backend (requires `--features cuda`)
//!   --log-height=N   app log stacked height, 5..=24 (default 24, the 100-bit security cap)
//!
//! With neither `--cpu` nor `--gpu`, runs CPU and, if built with `cuda`, GPU too.
//!
//! Examples:
//!   cargo run --profile fast -p poseidon2-bench -- 30000
//!   cargo run --profile fast -p poseidon2-bench --features cuda -- --gpu 900000

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use eyre::{bail, Result};
use openvm_build::TargetFilter;
use openvm_instructions::exe::VmExe;
use openvm_poseidon2_circuit::Poseidon2Rv32Config;
use openvm_poseidon2_transpiler::Poseidon2TranspilerExtension;
use openvm_rv32im_transpiler::{
    Rv32ITranspilerExtension, Rv32IoTranspilerExtension, Rv32MTranspilerExtension,
};
use openvm_sdk::{
    config::{AggregationSystemParams, AppConfig},
    openvm_circuit::arch::{
        Executor, MeteredExecutor, PreflightExecutor, VmBuilder, VmExecutionConfig,
    },
    GenericSdk, StdIn, F, SC,
};
use openvm_stark_sdk::{
    config::{app_params_with_100_bits_security, DEFAULT_APP_L_SKIP, MAX_APP_LOG_STACKED_HEIGHT},
    openvm_stark_backend::StarkEngine,
};
use openvm_transpiler::{transpiler::Transpiler, FromElf};

const DEFAULT_ITERS: u32 = 20_000;
const MIN_LOG_HEIGHT: usize = DEFAULT_APP_L_SKIP + 1;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Cpu,
    Gpu,
    Both,
}

struct Args {
    iters: u32,
    log_height: usize,
    mode: Mode,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        iters: DEFAULT_ITERS,
        log_height: MAX_APP_LOG_STACKED_HEIGHT,
        mode: Mode::Both,
    };
    let mut mode_set = false;
    for arg in std::env::args().skip(1) {
        if arg == "--cpu" || arg == "--gpu" {
            if mode_set {
                bail!("pass at most one of --cpu / --gpu");
            }
            mode_set = true;
            args.mode = if arg == "--cpu" { Mode::Cpu } else { Mode::Gpu };
        } else if let Some(v) = arg.strip_prefix("--log-height=") {
            let h: usize = v
                .parse()
                .map_err(|_| eyre::eyre!("invalid --log-height value: {v:?}"))?;
            if !(MIN_LOG_HEIGHT..=MAX_APP_LOG_STACKED_HEIGHT).contains(&h) {
                bail!(
                    "--log-height must be in {MIN_LOG_HEIGHT}..={MAX_APP_LOG_STACKED_HEIGHT} \
                     (the max is the 100-bit security cap), got {h}"
                );
            }
            args.log_height = h;
        } else if let Ok(v) = arg.parse::<u32>() {
            args.iters = v;
        } else {
            bail!(
                "unrecognised argument {arg:?}\n\
                 usage: poseidon2-bench [--cpu | --gpu] [--log-height=N] [iters]"
            );
        }
    }
    #[cfg(not(feature = "cuda"))]
    if args.mode == Mode::Gpu {
        bail!("--gpu requires building with `--features cuda`");
    }
    Ok(args)
}

fn main() -> Result<()> {
    let Args {
        iters,
        log_height,
        mode,
    } = parse_args()?;
    println!(
        "=== Poseidon2 benchmark: iters = {iters}, log_stack_height = {log_height} (mode = {mode:?}) ===\n"
    );

    let cpu = if mode != Mode::Gpu {
        use openvm_poseidon2_circuit::Poseidon2Rv32CpuBuilder;
        use openvm_stark_sdk::config::baby_bear_poseidon2::BabyBearPoseidon2CpuEngine;
        Some(run::<BabyBearPoseidon2CpuEngine, Poseidon2Rv32CpuBuilder>(
            "CPU", iters, log_height,
        )?)
    } else {
        None
    };

    #[cfg(feature = "cuda")]
    if mode != Mode::Cpu {
        use openvm_cuda_backend::BabyBearPoseidon2GpuEngine;
        use openvm_poseidon2_circuit::extension::cuda::Poseidon2Rv32GpuBuilder;
        let gpu =
            run::<BabyBearPoseidon2GpuEngine, Poseidon2Rv32GpuBuilder>("GPU", iters, log_height)?;
        if let Some(cpu) = &cpu {
            let speedup = cpu.as_secs_f64() / gpu.as_secs_f64();
            println!("\nspeedup (CPU/GPU): {speedup:.2}x");
        }
    }
    #[cfg(not(feature = "cuda"))]
    let _ = cpu;

    Ok(())
}

fn timed<T>(f: impl FnOnce() -> Result<T>) -> Result<(T, Duration)> {
    let start = Instant::now();
    let out = f()?;
    Ok((out, start.elapsed()))
}

/// Build the guest, then time keygen, execute and prove+verify separately on backend `E`/`VB`.
/// Prints all three and returns the prove+verify time.
fn run<E, VB>(label: &str, iters: u32, log_height: usize) -> Result<Duration>
where
    E: StarkEngine<SC = SC>,
    VB: VmBuilder<E, VmConfig = Poseidon2Rv32Config> + Clone + Default,
    <Poseidon2Rv32Config as VmExecutionConfig<F>>::Executor:
        Executor<F> + MeteredExecutor<F> + PreflightExecutor<F, VB::RecordArena>,
{
    let sdk = GenericSdk::<E, VB>::new_without_transpiler(
        AppConfig::new(
            Poseidon2Rv32Config::default(),
            app_params_with_100_bits_security(log_height),
        ),
        AggregationSystemParams::default(),
    )?;
    let exe = Arc::new(VmExe::from_elf(
        sdk.build(
            Default::default(),
            guest_dir(),
            &Some(TargetFilter {
                name: "poseidon2-bench-guest".to_string(),
                kind: "bin".to_string(),
            }),
            None,
        )?,
        transpiler(),
    )?);

    // The SDK generates the app and aggregation keys lazily on first use (inside `sdk.prove`).
    // Generate them here so the `prove` timing only measures proving + verification.
    let (agg_vk, keygen) = timed(|| Ok(sdk.agg_vk().as_ref().clone()))?;
    let ((), execute) = timed(|| {
        sdk.execute(exe.clone(), stdin_for(iters))?;
        Ok(())
    })?;
    let ((), prove) = timed(|| {
        let (proof, baseline) = sdk.prove(exe.clone(), stdin_for(iters), &[])?;
        GenericSdk::<E, VB>::verify_proof(agg_vk, baseline, &proof)?;
        Ok(())
    })?;
    println!(
        "{label} prove+verify:  {prove:>10.2?}   (execute {execute:.2?}, keygen {keygen:.2?})"
    );
    Ok(prove)
}

fn stdin_for(iters: u32) -> StdIn {
    let mut stdin = StdIn::default();
    stdin.write(&iters);
    stdin
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
