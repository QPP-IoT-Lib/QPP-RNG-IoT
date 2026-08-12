//! Backend for 32-bit ARM (ARMv7-A) Linux, e.g. a 32-bit kernel on
//! Raspberry Pi 4. Wraps the PMU cycle-counter shim (with runtime
//! `clock_gettime` fallback when userspace PMU access isn't granted)
//! in `c/linux_arm32_pmu.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// PMCCNTR cycle counter, gated behind a one-time SIGILL-guarded probe
/// (userspace access requires the kernel to have set PMUSERENR.EN,
/// which isn't the default on Raspberry Pi OS, and Raspberry Pi 0's
/// ARMv6 core doesn't implement this PMU at all); falls back to
/// `clock_gettime(CLOCK_MONOTONIC)` at runtime when the probe fails.
#[derive(Default)]
pub struct LinuxArm32Timer;

impl HighResTimer for LinuxArm32Timer {
    fn init(&mut self) {
        // The PMU availability probe runs lazily, on first `tick()`.
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments and has no
        // preconditions; it either reads the PMU cycle counter or, on
        // its fallback path, OS-owned clock state. Its own SIGILL probe
        // is confined to the C shim and restores the prior handler.
        unsafe { qpp_timer_tick() }
    }
}
