//! End-to-end example: build the guest program, run it, and produce + verify a STARK proof,
//! all using the `Poseidon2Rv32Config` / `Poseidon2Rv32CpuBuilder` shipped by the extension.
//!
//! The guest program itself lives in `guest/`; it exercises `hash_u32s`, `hash_bytes`, and
//! `permute` and prints its results. This host binary is what wires the extension into the
//! [`openvm-sdk`]: the config is a plain `VmConfig` composed with the `Poseidon2` extension, so no
//! `cargo openvm` / `openvm.toml` is involved.

use std::path::PathBuf;

use eyre::Result;
use openvm_build::TargetFilter;
use openvm_instructions::exe::VmExe;
use openvm_poseidon2_circuit::{Poseidon2Rv32Config, Poseidon2Rv32CpuBuilder};
use openvm_poseidon2_transpiler::Poseidon2TranspilerExtension;
use openvm_rv32im_transpiler::{
    Rv32ITranspilerExtension, Rv32IoTranspilerExtension, Rv32MTranspilerExtension,
};
use openvm_sdk::{
    config::{AggregationSystemParams, AppConfig},
    DefaultStarkEngine, GenericSdk, StdIn, F,
};
use openvm_stark_sdk::config::{app_params_with_100_bits_security, MAX_APP_LOG_STACKED_HEIGHT};
use openvm_transpiler::{transpiler::Transpiler, FromElf};

fn main() -> Result<()> {
    // 1. An SDK built around the poseidon2 extension: `Poseidon2Rv32Config` is a `VmConfig`
    //    composed of rv32im + io + poseidon2, and `Poseidon2Rv32CpuBuilder` is the CPU prover
    //    backend. We skip the default RISC-V transpiler here and configure one manually so the
    //    example makes explicit which transpiler extensions are required.
    let app_config = AppConfig::new(
        Poseidon2Rv32Config::default(),
        app_params_with_100_bits_security(MAX_APP_LOG_STACKED_HEIGHT),
    );
    let sdk = GenericSdk::<DefaultStarkEngine, Poseidon2Rv32CpuBuilder>::new_without_transpiler(
        app_config,
        AggregationSystemParams::default(),
    )?;

    // 2. Build the guest program (`guest/` package) into an ELF.
    let guest_pkg_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guest");
    let elf = sdk.build(
        Default::default(),
        guest_pkg_dir,
        &Some(TargetFilter {
            name: "poseidon2-example-guest".to_string(),
            kind: "bin".to_string(),
        }),
        None,
    )?;

    // 3. Transpile the ELF into an OpenVM executable. Poseidon2 uses RISC-V custom instruction
    //    `0x0b`/funct3 `0b111`, recognized by `Poseidon2TranspilerExtension`; the rv32im and io
    //    extensions cover the rest of the guest's instruction set.
    let transpiler = Transpiler::<F>::default()
        .with_extension(Poseidon2TranspilerExtension)
        .with_extension(Rv32ITranspilerExtension)
        .with_extension(Rv32MTranspilerExtension)
        .with_extension(Rv32IoTranspilerExtension);
    let exe = VmExe::from_elf(elf, transpiler)?;
    let exe = std::sync::Arc::new(exe);

    // 4. Execute: the guest performs all hashing checks internally and asserts on them.
    println!("executing guest...");
    let public_values = sdk.execute(exe.clone(), StdIn::default())?;
    println!("public values: {public_values:02x?}");

    // 5. Prove and verify the whole execution.
    println!("proving...");
    let (proof, baseline) = sdk.prove(exe, StdIn::default(), &[])?;
    let (_agg_pk, agg_vk) = sdk.agg_keygen();
    GenericSdk::<DefaultStarkEngine, Poseidon2Rv32CpuBuilder>::verify_proof(
        agg_vk, baseline, &proof,
    )?;
    println!("proof verified!");

    Ok(())
}
