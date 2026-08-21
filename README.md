# openvm-poseidon2

Standalone [OpenVM](https://github.com/openvm-org/openvm) extension and guest library for the
Poseidon2 hash function.

Poseidon2 is provided as a RISC-V custom instruction (`PERMUTE`, opcode offset `0x330`) with a
matching circuit AIR, plus a host/zkvm dual guest library exposing a field-element sponge,
`hash_u32s`, and `hash_bytes`.

> **Status: scaffolding.** Crates and CI are being set up. See `POSEIDON2.md` roadmap notes in the
> originating [openvm PR](https://github.com/amintalebi/openvm/pulls).

## Layout

- `extensions/poseidon2/guest` — `openvm-poseidon2-guest`: RISC-V custom instruction constants + `native_poseidon2_permute`.
- `extensions/poseidon2/transpiler` — `openvm-poseidon2-transpiler`: `Poseidon2Opcode` + transpiler extension.
- `extensions/poseidon2/circuit` — `openvm-poseidon2-circuit`: adapter/periphery AIRs, `Poseidon2` extension, `Poseidon2Rv32Config` + CPU builder.
- `guest-libs/poseidon2` — `openvm-poseidon2`: guest library (`Poseidon2` sponge, `hash_u32s`, `hash_bytes`, `permute`) + integration tests.
- `examples/poseidon2` — end-to-end example proving with the extension via the SDK.

## Development

See `AGENTS.md`.
