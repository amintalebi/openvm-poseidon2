// [!region imports]
#![cfg_attr(all(target_os = "zkvm", not(feature = "std")), no_main)]
#![cfg_attr(all(target_os = "zkvm", not(feature = "std")), no_std)]

extern crate alloc;

use alloc::{format, string::String};

use openvm_poseidon2::{hash_bytes, hash_u32s, Poseidon2, POSEIDON2_WIDTH};

openvm::entry!(main);
// [!endregion imports]

// [!region main]
fn println(s: String) {
    #[cfg(target_os = "zkvm")]
    openvm::io::println(s);
    #[cfg(not(target_os = "zkvm"))]
    println!("{}", s);
}

pub fn main() {
    // Poseidon2 hashes *field elements*, not bytes. Each input word must be a canonical
    // BabyBear element, i.e. less than 0x78000001.
    let digest = hash_u32s(&[1, 2, 3, 4, 5]);
    println(format!("hash_u32s([1..5]): {:?}", digest));

    // The same hash computed incrementally. Absorbing in several `update` calls must give
    // exactly the same digest as absorbing everything at once.
    let mut hasher = Poseidon2::new();
    hasher.update(&[1, 2]);
    hasher.update(&[3, 4, 5]);
    assert_eq!(hasher.finalize(), digest, "incremental hashing must match one-shot");

    // Absorbing more than the rate (8 elements) forces multiple permutations, so this
    // exercises the PERMUTE instruction repeatedly rather than just once.
    let long_input: [u32; 25] = core::array::from_fn(|i| i as u32);
    println(format!("hash_u32s(0..25):  {:?}", hash_u32s(&long_input)));

    // `hash_bytes` accepts arbitrary bytes of any length. It packs 3 bytes per field element
    // (3 bytes always fit; 4 would not) and applies byte-level padding, so unlike `hash_u32s`
    // it can never reject its input.
    println(format!("hash_bytes(hello): {:?}", hash_bytes(b"hello world")));
    assert_ne!(
        hash_bytes(b"a"),
        hash_bytes(b"a\0"),
        "padding must distinguish inputs differing only in trailing zeroes"
    );

    // The raw permutation underneath everything above: one PERMUTE instruction, scrambling
    // a 16-element state in place.
    let mut state = [0u32; POSEIDON2_WIDTH];
    state[0] = 1;
    openvm_poseidon2::permute(&mut state);
    println(format!("permute(e0):       {:?}", &state[..4]));

    println(String::from("All poseidon2 checks passed!"));
}
// [!endregion main]
