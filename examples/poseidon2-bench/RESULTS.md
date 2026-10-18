# Poseidon2 GPU benchmark

Compares STARK proof-generation time for the OpenVM-rv32im + Poseidon2 extension between the CPU
backend (`Poseidon2Rv32CpuBuilder`) and the CUDA backend (`Poseidon2Rv32GpuBuilder` /
`Poseidon2GpuProverExt`).

## Method

- **Workload**: a benchmark guest performs `iters` Poseidon2 permutations (one `PERMUTE` custom
  instruction per iteration) plus a couple of one-shot hashes. The trace height therefore scales
  with `iters`.
- **What is timed**: `sdk.prove(...)` **plus** `verify_proof(...)` (i.e. proof generation + the
  verifier; verification is negligible vs. proving). Reported via `examples/poseidon2-bench`.
- **Both backends prove the *same* guest and the timings are wall-clock**.
- The GPU number also confirms end-to-end correctness: the GPU proof must verify, which it does
  (same AIR constraints, byte-identical trace columns to the CPU tracegen).

## Hardware / environment

| | |
|---|---|
| GPU | NVIDIA RTX PRO 6000 Blackwell Server Edition (sm_120, 97 GB) |
| Driver / toolkit | CUDA 13.0, nvcc 13.0 |
| CPU | 24 vCPU (Nebius `1gpu-24vcpu-218gb`) |
| Build | OpenVM monorepo v2.0.2 + this extension, `cargo build --release`/`fast` with `--features cuda` |

## Results

Build profile: `--profile fast` (opt-level 1) for both backends; CUDA kernels are always
`--ptxas-options=-O3`. All runs on the s2t-rtx6000 instance above.

| iters | CPU prove+verify | GPU prove+verify | speedup |
|------:|-----------------:|-----------------:|--------:|
| 1000  | 30.08 s          | 6.39 s           | 4.71×   |
| 5000  | 34.48 s          | 6.29 s           | 5.48×   |
| 30000 | 72.55 s          | 6.74 s           | 10.76×  |

Notes:

- The GPU proof **verifies** at every size, confirming the CUDA tracegen produces byte-identical
  trace columns to the CPU path.
- CPU prove time is flat between `iters=1000` and `iters=5000` because trace height is padded up to
  a power of two; proof cost is dominated by that padded height, so adding permutes doesn't move the
  needle until it crosses the next height boundary. Once it does (between 5000 and 30000) the CPU
  cost roughly doubles while the GPU stays flat, so the speedup widens (5.5× → 10.8×).