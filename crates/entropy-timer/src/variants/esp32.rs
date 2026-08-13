//! Backend for the ESP32 (Xtensa LX6). Wraps the `CCOUNT` cycle-counter
//! read in `c/esp32_timer.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// Per-core CPU cycle counter (`CCOUNT`), free-running from reset.
/// 32-bit width, zero-extended to `u64`; diff with wrapping arithmetic.
#[derive(Default)]
pub struct Esp32Timer;

impl HighResTimer for Esp32Timer {
    fn init(&mut self) -> u8 {
        // CCOUNT free-runs from reset; nothing to configure.
        5 // ns is the native timer resolution on ESP32 systems
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments, has no
        // preconditions, and only reads a CPU special register.
        unsafe { qpp_timer_tick() }
    }
}
