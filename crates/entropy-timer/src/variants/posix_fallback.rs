//! Generic Unix-like fallback backend: any Unix-like target not covered
//! by a more specific one (see `mod.rs`), e.g. Linux on riscv64/mips/
//! powerpc, or a BSD. Wraps the `clock_gettime(CLOCK_MONOTONIC)` shim
//! in `c/posix_timer.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// Nanosecond-resolution monotonic clock, sourced from the OS.
#[derive(Default)]
pub struct PosixFallbackTimer;

impl HighResTimer for PosixFallbackTimer {
    fn init(&mut self) -> u8 {
        // clock_gettime(CLOCK_MONOTONIC) needs no setup.
        1 // ns is the native timer resolution on Windows
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments, has no
        // preconditions, and only reads OS-owned clock state.
        unsafe { qpp_timer_tick() }
    }
}
