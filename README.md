# openvm-poseidon2

Standalone [OpenVM](https://github.com/openvm-org/openvm) extension and guest library for the
Poseidon2 hash function.

Poseidon2 is provided as a RISC-V custom instruction (`PERMUTE`, opcode offset `0x330`) with a
matching circuit AIR, plus a host/zkvm dual guest library exposing a field-element sponge,
`hash_u32s`, and `hash_bytes`.

The extension is **not** part of the OpenVM monorepo and is **not** wired into `openvm-sdk-config`,
so `cargo openvm` / `[app_vm_config.poseidon2]` does not apply here. Instead you compose the
`Poseidon2` extension into your own VM configuration and use the SDK programmatically.

## Layout

- `extensions/poseidon2/guest` — `openvm-poseidon2-guest`: RISC-V custom instruction constants + `native_poseidon2_permute`.
- `extensions/poseidon2/transpiler` — `openvm-poseidon2-transpiler`: `Poseidon2Opcode` + transpiler extension.
- `extensions/poseidon2/circuit` — `openvm-poseidon2-circuit`: adapter/periphery AIRs, `Poseidon2` extension, `Poseidon2Rv32Config` + CPU builder.
- `guest-libs/poseidon2` — `openvm-poseidon2`: guest library (`Poseidon2` sponge, `hash_u32s`, `hash_bytes`, `permute`) + integration tests.
- `examples/poseidon2` — end-to-end example: build, execute, prove, and verify a guest program via the SDK.

## Usage

Add the crates to your workspace:

```toml
[dependencies]
openvm-poseidon2 = "2.0.2"
openvm-poseidon2-circuit = "2.0.2"
openvm-poseidon2-transpiler = "2.0.2"
openvm-poseidon2-guest = "2.0.2"
```

In your guest program, hash away:

```rust
#![cfg_attr(not(feature = "std"), no_main)]
#![cfg_attr(not(feature = "std"), no_std)]

use openvm_poseidon2::{hash_bytes, hash_u32s, permute};

openvm::entry!(main);

pub fn main() {
    let digest = hash_u32s(&[1, 2, 3, 4, 5]);
    assert_eq!(hash_bytes(b"hello world").len(), 32);
    let mut state = [0u32; 16];
    state[0] = 1;
    permute(&mut state);
}
```

On the host, prove programmatically. The extension ships a ready-made `VmConfig`
(`Poseidon2Rv32Config`, rv32im + io + poseidon2) and its CPU builder:

```rust
use openvm_poseidon2_circuit::{Poseidon2Rv32Config, Poseidon2Rv32CpuBuilder};
use openvm_poseidon2_transpiler::Poseidon2TranspilerExtension;
use openvm_rv32im_transpiler::{
    Rv32ITranspilerExtension, Rv32IoTranspilerExtension, Rv32MTranspilerExtension,
};
use openvm_sdk::{
    config::{AggregationSystemParams, AppConfig},
    DefaultStarkEngine, F, GenericSdk, StdIn,
};
use openvm_transpiler::{transpiler::Transpiler, FromElf};

let sdk = GenericSdk::<DefaultStarkEngine, Poseidon2Rv32CpuBuilder>::new_without_transpiler(
    AppConfig::new(Poseidon2Rv32Config::default(), /* SystemParams */ params),
    AggregationSystemParams::default(),
)?;

let exe = VmExe::from_elf(
    elf,
    Transpiler::<F>::default()
        .with_extension(Poseidon2TranspilerExtension)
        .with_extension(Rv32ITranspilerExtension)
        .with_extension(Rv32MTranspilerExtension)
        .with_extension(Rv32IoTranspilerExtension),
)?;

let pvs = sdk.execute(exe.clone(), StdIn::default())?;
let (proof, baseline) = sdk.prove(exe, StdIn::default(), &[])?;
let (_, agg_vk) = sdk.agg_keygen();
GenericSdk::<DefaultStarkEngine, Poseidon2Rv32CpuBuilder>::verify_proof(agg_vk, baseline, &proof)?;
```

To compose Poseidon2 into your *own* config instead, add it as an extension:

```rust
#[derive(VmConfig)]
struct MyConfig {
    #[config]
    pub rv32im: Rv32ImConfig,
    #[extension]
    pub poseidon2: Poseidon2,
}
```

and implement `VmBuilder<E>` for your backend (see `Poseidon2Rv32CpuBuilder` in
`extensions/poseidon2/circuit/src/extension/mod.rs` for the reference implementation).

### Running the example

```bash
# requires nightly-2026-01-18 + rust-src (see AGENTS.md)
cargo run --profile fast -p poseidon2-example
```

Builds the guest in `examples/poseidon2/guest`, executes it (it asserts on its own hashes), then
proves and verifies the whole execution.

## Hashing

The sponge operates on **canonical field elements** (each `< 0x78000001`):

- `hash_u32s(&[u32])` absorbs `u32` words directly; every word must be canonical.
- `hash_bytes(&[u8])` accepts arbitrary bytes: it packs 3 bytes per field element (`2^24 < 0x78000001`) with byte-level `pad10*1`, so it never rejects its input and is *not*
  interchangeable with `hash_u32s`.
- `permute(&mut [u32; 16])` applies the raw permutation (one `PERMUTE` instruction).

The host permutation reference is plonky3's `default_babybear_poseidon2_16`, which matches the
permutation constrained by the circuit. The KAT test vectors cross-check the two.

## Development

See `AGENTS.md`.
