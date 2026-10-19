# Formal verification plan (Lean 4)

This is the plan for formally verifying the soundness of the Poseidon2 extension's circuits in Lean,
following the approach OpenVM used for its own Keccak and SHA-2 extensions in
[`openvm-fv`](https://github.com/openvm-org/openvm-fv).

Formal verification can cover most of what an audit would check in the circuit, but it does not
replace an audit entirely. See [What stays unverified](#what-stays-unverified).

## Scope

Soundness (whether a false proof can be accepted) depends only on the **AIR constraints**. Trace
generation (CPU and CUDA) and the executor cannot make a false proof verify; a bug there only makes
honest proofs fail. So the verification target is about 375 lines of Rust:

| AIR | File | Role |
|-----|------|------|
| `Poseidon2PermuteAir` | `extensions/poseidon2/circuit/src/permute/air.rs` | Opcode AIR: memory reads/writes, execution bus, bus lookup |
| `CanonicitySubAir` | `extensions/poseidon2/circuit/src/canonicity/air.rs` | Output bytes are the canonical encoding of a field element |
| `Poseidon2PeripheryAir` | `extensions/poseidon2/circuit/src/periphery/air.rs` | Wraps OpenVM's `Poseidon2SubAir`; publishes `input ‖ output` on the bus |

The CUDA kernels are out of scope for these proofs.

This has the same structure as OpenVM's Keccak extension, which openvm-fv verifies as an opcode
AIR (`KeccakfOpAir`) plus a permutation AIR (`KeccakfPermAir`) joined by a bus. Their proofs are
the template for ours.

## Steps

### 1. Extract the constraints into Lean

openvm-fv's [`extractor`](https://github.com/openvm-org/openvm-fv/tree/main/extractor) is about
70 lines of Rust. It runs keygen on a VM config and prints the constraints of any AIR whose name
matches, using `openvm_stark_backend::lean::extract_constraints_to_lean`.

- Copy it and replace `SdkVmConfig::standard` with `Poseidon2Rv32Config`.
- Extract `Poseidon2PermuteAir` and `Poseidon2PeripheryAir`.
- The `lean` feature it relies on exists in `openvm-stark-backend` 2.0.1, the version this repo
  already uses.
- openvm-fv pins OpenVM v2.0.0 and this repo uses 2.0.2, so their Lean library may need small
  fixes.

### 2. Write the reference spec

Define the BabyBear width-16 Poseidon2 permutation in Lean. The spec is trusted, so it must be
checked independently of the circuit:

- Take the round constants and internal diagonal from an independent source, not from the circuit.
- The circuit and host implementation use plonky3's `default_babybear_poseidon2_16`. `zkhash`'s
  BabyBear Poseidon2 uses a different internal diagonal (see `AGENTS.md`), so it is not a valid
  reference.

### 3. Prove the theorems, easiest first

1. **Canonicity.** If the `CanonicitySubAir` constraints hold and its range check passes, the four
   output bytes compose to a value less than `p = 0x78000001`. Short; a good first target.
2. **Opcode AIR.** On every enabled row of `Poseidon2PermuteAir`:
   - the buffer pointer is read from register `rd`;
   - 16 canonical words are written to `buffer_ptr .. buffer_ptr + 64`;
   - the written bytes are exactly the output words looked up on the bus;
   - the pc advances by 4 and the timestamp by 17.

   Mirrors openvm-fv's `KeccakfOpAir` proofs.
3. **Periphery.** Every row with a nonzero multiplicity has `output = Poseidon2(input)`. This is
   the largest piece, because it means reasoning about every Poseidon2 round. Nethermind has
   reportedly verified Plonky3's Poseidon2 before; ask whether that work can be reused.
4. **Bus bridge.** Combine (2) and (3) under the lookup-bus semantics, so every `PERMUTE` writes
   `Poseidon2(input)` to memory. Mirrors openvm-fv's combined Keccak theorem
   (`Keccakf.Soundness.keccakf_matches_spec`).

### 4. Add CI checks

Copy openvm-fv's checks so the result can be trusted by others:

- no `sorry`, `admit`, `native_decide` or `bv_decide` (`scripts/check_hygiene.py`);
- an axiom audit that fails on anything beyond `propext`, `Classical.choice` and `Quot.sound`
  (`#audit_axioms`);
- an independent re-check of the frozen theorem statements (`leanprover/comparator`).

### Optional: guest library

The sponge in `guest-libs/poseidon2` is outside the circuit, but some of its properties are cheap
to prove in Lean and worth adding. The main one: `hash_bytes`' byte padding never maps two
different inputs to the same field elements.

## What stays unverified

- **The extraction tool and the Lean spec.** openvm-fv lists both as trusted. Someone must review
  that the theorem statements say what is intended.
- **OpenVM itself:** the memory bridge, the bus argument and the STARK prover. These are covered by
  OpenVM's own audits and it is reasonable to rely on them.
- **Anything not stated as a theorem.** A proof only covers what it says; a missing property passes
  silently.
- **Code outside the circuit:** the transpiler's instruction decoding, and the guest library unless
  the optional proofs above are done.
- **Completeness:** like openvm-fv, this plan proves only soundness, not that every honest trace is
  accepted. Tests cover that direction.

OpenVM had openvm-fv and still had zkSecurity
[audit the same chips](https://reports.zksecurity.xyz/reports/openvm.md). With the proofs done, a
much narrower and cheaper review is possible: an expert reads only the theorem statements and the
spec, not the circuit.

## Funding

- The Ethereum Foundation
  [funds zkVM formal verification](https://pq.ethereum.org/blog/formal-verification-zkevm/) and paid
  for OpenVM's own Lean work. A small grant for this is plausible.
- Proposing the extension upstream to OpenVM would put it under their verification and audit
  process.

## References

- [openvm-fv](https://github.com/openvm-org/openvm-fv): Lean proofs for OpenVM RV32IM, Keccak and SHA-2
- [Axiom: formal verification of Keccak and SHA-2](https://axiom.xyz/blog/fv-keccak-sha2)
- [Axiom: formal verification of OpenVM RV32IM](https://axiom.xyz/blog/formal-verification)
- [zkSecurity audit of OpenVM 2.0](https://reports.zksecurity.xyz/reports/openvm.md)
- [Nethermind: shared methods for verifying RISC-V zkVMs](https://www.nethermind.io/blog/from-certiplonk-to-zkvms-shared-methodologies-for-verifying-risc-v-conformance)
- [zkSecurity: Clean, a Lean 4 DSL for ZK circuits](https://blog.zksecurity.xyz/posts/clean)
