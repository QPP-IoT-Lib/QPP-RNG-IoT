//! Backend for 64-bit ARM (aarch64) Linux, e.g. Raspberry Pi 4 on a
//! 64-bit kernel. Wraps the `CNTVCT_EL0` read in
//! `c/linux_arm64_cntvct.c`.

use super::HighResTimer;

unsafe extern "C" {
    fn qpp_timer_tick() -> u64;
}

/// ARM generic timer's virtual count register, always readable from
/// EL0 under Linux -- no probing or fallback needed.
#[derive(Default)]
pub struct LinuxArm64Timer;

impl HighResTimer for LinuxArm64Timer {
    fn init(&mut self) {
        // CNTVCT_EL0 is unconditionally readable from EL0 under Linux.
    }

    fn tick(&mut self) -> u64 {
        // SAFETY: `qpp_timer_tick` takes no arguments, has no
        // preconditions, and only reads a CPU system register that
        // Linux always exposes to EL0.
        unsafe { qpp_timer_tick() }
    }
}
