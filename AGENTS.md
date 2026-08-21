# AGENTS.md

## Agent Quickstart (Read First)

- Prefer targeted commands (`-p <crate>` or `cd <crate-dir>`) over workspace-wide runs.
- Use the CI-like build profile: `cargo build --profile fast -p <crate>`.
- Formatting requires nightly rustfmt: `cargo +nightly fmt --all` (stable `cargo fmt` fails due to
  unstable options in `rustfmt.toml`).
- Run tests with nextest when available: `cargo nextest run --cargo-profile=fast -p <crate>`;
  fallback: `cargo test -p <crate>`.
- After major code edits, run `cargo +nightly fmt --all -- --check`, `cargo clippy -p <crate>
  --all-targets --tests -- -D warnings`, and `cargo shear` before pushing.

## Build & Development Commands

### Rust Toolchain

- Stable Rust 1.91.1 (see `rust-toolchain.toml`).
- Nightly `nightly-2026-01-18` + `rust-src` are only needed for guest program compilation and the
  guest-lib integration tests:
  ```
  rustup component add rust-src --toolchain nightly-2026-01-18
  ```

### Building

```bash
cargo build --profile fast -p <crate>            # optimized dev build (opt-level=1, incremental)
cargo build --release -p <crate>                 # release build
```

### Formatting & Linting

```bash
cargo +nightly fmt --all                         # format (nightly required for unstable options)
cargo +nightly fmt --all -- --check              # check formatting
cargo clippy -p <crate> --all-targets --tests -- -D warnings
cargo shear                                      # check for unused dependencies
```

Formatting uses unstable options: `group_imports = "StdExternalCrate"`,
`imports_granularity = "Crate"`.

### Testing

Tests use [cargo-nextest](https://nexte.st/). The `fast` cargo profile is used for test runs.

```bash
# Circuit unit tests
cargo nextest run --cargo-profile=fast -p openvm-poseidon2-circuit

# Guest-lib integration tests (guest->transpile->execute->prove; nightly + rust-src required)
cd guest-libs/poseidon2 && cargo nextest run --cargo-profile=fast --profile=heavy
```

## Architecture

The extension mirrors the OpenVM `keccak256`/`sha2` extension split:

| Component    | Crate                          | Role                                                        |
| ------------ | ------------------------------ | ----------------------------------------------------------- |
| guest        | `openvm-poseidon2-guest`       | RISC-V custom instruction constants + `native_poseidon2_permute` |
| transpiler   | `openvm-poseidon2-transpiler`  | `Poseidon2Opcode` + `TranspilerExtension::process_custom`    |
| circuit      | `openvm-poseidon2-circuit`     | Adapter (memory + buses) + periphery (permutation) AIRs, `VmConfig`/builder |
| guest-lib    | `openvm-poseidon2`             | Host/zkvm hasher API + integration tests                     |

### Key design points

- The extension is **opt-in**: users compose it into their own `#[derive(VmConfig)]` config with an
  `#[extension] poseidon2: Poseidon2` field and implement a `VmBuilder`, or use the ready-made
  `Poseidon2Rv32Config` / `Poseidon2Rv32CpuBuilder`.
- It is **not** wired into `openvm-sdk-config`, so `cargo openvm` / `[app_vm_config.poseidon2]`
  does not apply to the standalone version; users prove programmatically via `openvm-sdk`.
- The sponge hashes **canonical field elements** (`< 0x78000001`). `hash_u32s` requires canonical
  words; `hash_bytes` packs 3 bytes per element so it accepts arbitrary bytes.
- The host permutation reference is plonky3's `default_babybear_poseidon2_16`, which matches the
  permutation constrained by the circuit. **Do not** switch the host reference to `zkhash` — its
  BabyBear Poseidon2 uses a different internal MDS diagonal and would disagree with the circuit.
- `openvm-poseidon2-air` (the permutation AIR primitive) lives in the OpenVM repo (the VM's system
  Poseidon2 chip needs it) and is depended on from crates.io.

## Guest Programs

Guest programs use `#![no_std]` with `openvm::entry!(main)` and live under `tests/programs/` within
the guest-lib test crate.

## Proof Field

BabyBear (31-bit prime field from Plonky3). The host/zkvm guest-lib implementations must agree
byte-for-byte; the KAT test vectors cross-check against the circuit.
