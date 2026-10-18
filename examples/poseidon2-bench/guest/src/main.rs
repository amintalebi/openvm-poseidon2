//! Benchmark guest: performs `iters` Poseidon2 permutations (one `PERMUTE` custom instruction per
//! iteration) to build a trace large enough that the CPU-vs-GPU proving difference is meaningful.
//!
//! `iters` is read from the host via `openvm::io::read` (a `u32` written with `StdIn::write`), so
//! the same guest binary can benchmark any workload size without recompiling.

#![cfg_attr(all(target_os = "zkvm", not(feature = "std")), no_main)]
#![cfg_attr(all(target_os = "zkvm", not(feature = "std")), no_std)]

extern crate alloc;

use alloc::format;

use openvm_poseidon2::{permute, Poseidon2, POSEIDON2_WIDTH};

openvm::entry!(main);

fn println(s: String) {
    #[cfg(target_os = "zkvm")]
    openvm::io::println(s);
    #[cfg(not(target_os = "zkvm"))]
    println!("{}", s);
}

pub fn main() {
    let iters: u32 = openvm::io::read();

    // A canonical (field-element) state to permute in place. Each `permute` emits exactly one
    // `PERMUTE` instruction that reads the 16-word buffer, permutes it, and writes it back.
    let mut state = [0u32; POSEIDON2_WIDTH];
    state[0] = 1;
    state[1] = 2;

    let mut acc: u32 = 0;
    for _ in 0..iters {
        permute(&mut state);
        // Use the result so the permutation can never be optimized away.
        acc = acc.wrapping_add(state[0]);
    }

    // Also exercise a couple of one-shot hashes so the guest's rv32im/io surface stays exercised.
    let mut hasher = Poseidon2::new();
    hasher.update(&[1u32, 2, 3, 4, 5, 6, 7, 8]);
    let digest = hasher.finalize();
    acc = acc.wrapping_add(digest[0]);

    println(format!("bench guest: iters={iters} acc={acc}"));
}