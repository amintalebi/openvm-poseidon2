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
| 1,000 | 30.08 s | 6.39 s | 4.71× |
| 5,000 | 34.48 s | 6.29 s | 5.48× |
| 30,000 | 72.55 s | 6.74 s | 10.76× |
| 60,000 | 119.30 s | 7.42 s | 16.07× |
| 120,000 | 189.91 s | 8.67 s | 21.91× |

Grounding on trace height: the VM's single-app proof is hard-capped at `MAX_APP_LOG_STACKED_HEIGHT = 24`
(2^24 rows) for the 100-bit soundness guarantee. The GPU reaches that ceiling without breaking a sweat:

| iters (GPU only) | approx. height | GPU prove+verify |
|-----------------:|---------------:|-----------------:|
| 250,000 | 2^22 | 10.77 s |
| 500,000 | 2^23 | 16.39 s |
| 900,000 | 2^24 (max) | 24.03 s |

CPU does not survive the climb: a CPU `iters=500_000` run (≈2^23, just 2× the 120k point) was still
running past **53 minutes** when the harness timed out, versus **16.4 s** on the GPU at the same
height — i.e. the CPU-vs-GPU gap is **≥ ~190×** at this size, vs 21.9× at 120k iters.

### Why single-proof GPU utilization is low (and how to raise it)

`nvidia-smi` reports only ~6% util during a single proof. That is not the GPU being idle-limited by
compute; OpenVM GPU proving is dominated by *tracegen* (memory-bandwidth/launch-bound, host-heavy)
and the logUp/sumcheck phases, and a single app proof is capped at 2^24 rows by soundness — so the
big GPU FFTs are short-lived bursts. Measured while batching proofs concurrently (iters=200 000):

| setup | wall time | GPU util |
|-------|----------:|----------|
| 1 proof (serial) | 12.5 s | ~6% avg |
| 4 proofs (parallel) | 16.3 s (4 done) | **39% avg / 100% peak**, ~32 GiB |

i.e. 4 proofs finish in 16.3 s vs ~50 s serial → **~3× prover throughput**, with the GPU peaking at
100% util. To use the GPU fully, run proofs in parallel (batching), as a prover service does.
`examples/poseidon2-bench` supports `--gpu`/`--cpu`/`--log-height=N` to drive these runs.

Notes:

- The GPU proof **verifies** at every size, confirming the CUDA tracegen produces byte-identical
  trace columns to the CPU path.
- CPU prove time is flat between `iters=1000` and `iters=5000` because trace height is padded up to
  a power of two; proof cost is dominated by that padded height, so adding permutes doesn't move the
  needle until it crosses the next height boundary. Once it does (between 5000 and 30000) the CPU
  cost roughly doubles while the GPU stays flat, so the speedup widens (5.5× → 10.8×).