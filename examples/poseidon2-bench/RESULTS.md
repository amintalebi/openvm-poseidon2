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
- **Keygen is excluded**: the SDK generates the app + aggregation keys lazily on the first
  `sdk.prove`, so the bench generates them first and reports `keygen` separately. It is small
  (~0.35–0.4 s on both backends), so the ~28 s CPU / ~6 s GPU floor at small `iters` is proving
  cost, not keygen.
- **Both backends prove the *same* guest and the timings are wall-clock**.
- The GPU proof must verify, which it does at every size. This shows the GPU trace satisfies the
  same AIR constraints. It does **not** show the GPU trace is byte-identical to the CPU trace
  (padding rows or auxiliary columns could differ and still satisfy the constraints); that needs a
  direct CPU-vs-GPU trace comparison test.

## Hardware / environment

| | |
|---|---|
| GPU | NVIDIA RTX PRO 6000 Blackwell Server Edition (sm_120, 97 GB) |
| Driver / toolkit | CUDA 13.0, nvcc 13.0 |
| CPU | 24 vCPU (Nebius `1gpu-24vcpu-218gb`) |
| Build | OpenVM monorepo v2.0.2 + this extension, `--profile fast` with `--features cuda` |

## Results

Build profile: `--profile fast` (opt-level 1) for both backends; CUDA kernels are always
`--ptxas-options=-O3`. All runs on the s2t-rtx6000 instance above.

| iters | CPU prove+verify | GPU prove+verify | speedup |
|------:|-----------------:|-----------------:|--------:|
| 1,000 | 28.27 s | 5.98 s | 4.73× |
| 5,000 | 32.30 s | 5.97 s | 5.41× |
| 30,000 | 69.83 s | 6.45 s | 10.83× |
| 60,000 | 113.58 s | 7.19 s | 15.80× |
| 120,000 | 179.85 s | 8.55 s | 21.02× |

Grounding on trace height: the VM's single-app proof is hard-capped at `MAX_APP_LOG_STACKED_HEIGHT = 24`
(2^24 rows) for the 100-bit soundness guarantee. The GPU reaches that ceiling without breaking a sweat:

| iters (GPU only) | approx. height | GPU prove+verify |
|-----------------:|---------------:|-----------------:|
| 250,000 | 2^22 | 11.26 s |
| 500,000 | 2^23 | 17.49 s |
| 900,000 | 2^24 (max) | 25.49 s |

CPU does not survive the climb: a CPU `iters=500_000` run (≈2^23, just 2× the 120k point) was still
running past **53 minutes** when the harness timed out, versus **17.5 s** on the GPU at the same
height — i.e. the CPU-vs-GPU gap is **≥ ~180×** at this size, vs 21.0× at 120k iters. (The CPU
500k run is from the earlier bench version, which counted keygen; at ~0.4 s that doesn't matter
here.)

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

- The GPU proof **verifies** at every size (constraint-satisfying trace; see the Method note on
  why this is not a trace-equality check).
- CPU prove time is flat between `iters=1000` and `iters=5000` because trace height is padded up to
  a power of two; proof cost is dominated by that padded height, so adding permutes doesn't move the
  needle until it crosses the next height boundary. Once it does (between 5000 and 30000) the CPU
  cost roughly doubles while the GPU stays flat, so the speedup widens (5.4× → 10.8×).