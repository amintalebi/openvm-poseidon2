//! Local replacement for the unpublished `openvm-toolchain-tests` helper, replicating
//! `build_example_program_at_path` / `get_programs_dir!` on top of the published `openvm-build`
//! crate so guest example programs can be compiled and decoded inside tests.

use std::{fs::read, path::PathBuf};

use eyre::{Context, Result};
use openvm_build::{build_guest_package, get_package, GuestOptions, TargetFilter};
use openvm_circuit::arch::{InitFileGenerator, OPENVM_DEFAULT_INIT_FILE_BASENAME};
use openvm_transpiler::{elf::Elf, openvm_platform::memory::MEM_SIZE};

/// Absolute path to this crate's guest example programs directory.
pub fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/programs")
}

/// Builds the guest example `example_name` in `manifest_dir` and decodes it into an [`Elf`].
pub fn build_example_program_at_path(
    manifest_dir: PathBuf,
    example_name: &str,
    init_config: &impl InitFileGenerator,
) -> Result<Elf> {
    let pkg = get_package(&manifest_dir);
    let guest_opts = GuestOptions::default();
    init_config.write_to_init_file(
        &manifest_dir,
        Some(&format!(
            "{OPENVM_DEFAULT_INIT_FILE_BASENAME}_{example_name}.rs"
        )),
    )?;
    let output_dir = match build_guest_package(
        &pkg,
        &guest_opts,
        None,
        &Some(TargetFilter {
            name: example_name.to_string(),
            kind: "example".to_string(),
        }),
    ) {
        Ok(dir) => dir,
        Err(Some(code)) => std::process::exit(code),
        Err(None) => eyre::bail!("Guest build was skipped"),
    };
    let elf_path = output_dir.join(example_name);
    let data = read(&elf_path).with_context(|| format!("Path not found: {elf_path:?}"))?;
    Elf::decode(&data, MEM_SIZE as u32)
}
