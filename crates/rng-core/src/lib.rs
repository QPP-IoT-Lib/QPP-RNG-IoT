//! Corte traits of the QPP-RNG engine.
//! Implements the RNG trait (TryRNG with Error=Infallible) of the rand_core lib.
//! Standardizes all QPP-RNG implementations throughout the library.

#![no_std] // No standard library for compatibility with no_std, bare metal microcontrollers.

use rand_core::Rng;

pub trait QppRngSource: Rng {
    fn diagnostics(&self) -> RngDiagnostics;
}

#[derive(Debug, Clone, Copy)]
pub struct RngDiagnostics {
    pub permutation_size_bits: u8,
    pub last_permutation_count: u64,
    pub last_jitter_ns: Option<u64>,
}

pub trait JitterClock {
    fn tick(&mut self) -> u64; // nanoseconds
}