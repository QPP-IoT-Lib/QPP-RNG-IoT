//! Backend shared by x86/x86_64 Linux and macOS. Wraps the RDTSC (with
//! runtime `clock_gettime` fallback when the CPU lacks an invariant
//! TSC) shim in `c/x86_tsc.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// RDTSC cycle counter, gated behind a one-time invariant-TSC CPUID
/// check; falls back to `clock_gettime(CLOCK_MONOTONIC)` at runtime on
/// CPUs where RDTSC isn't safe to compare across cores.
#[derive(Default)]
pub struct X86TscTimer;

impl HighResTimer for X86TscTimer {
    fn init(&mut self) -> u8 {
        // The invariant-TSC CPUID check runs lazily, on first `tick()`.
        1 // ns is the native timer resolution on Windows
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments and has no
        // preconditions; it either reads the TSC register or, on its
        // fallback path, OS-owned clock state.
        unsafe { qpp_timer_tick() }
    }
}
