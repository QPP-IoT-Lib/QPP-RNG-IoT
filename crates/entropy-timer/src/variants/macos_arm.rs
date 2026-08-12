//! Backend for macOS on Apple silicon (aarch64). Wraps the
//! `mach_absolute_time()` shim in `c/macos_mach.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// CPU timebase counter (1ns per tick on Apple silicon), read directly
/// without going through the extra clock_gettime() indirection.
#[derive(Default)]
pub struct MacosArmTimer;

impl HighResTimer for MacosArmTimer {
    fn init(&mut self) {
        // mach_absolute_time() needs no setup.
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments, has no
        // preconditions, and only reads OS-owned clock state.
        unsafe { qpp_timer_tick() }
    }
}
