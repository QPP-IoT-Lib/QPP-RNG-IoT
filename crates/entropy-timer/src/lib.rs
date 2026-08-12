//! The entropy-timer crate is in charge of measuring the cpu time
//! C is used for its high-resolution timers in a multi-platform setting.
//! These platforms include ESP32 with Wifi, Raspi0, Raspi4, Arduino Uno, Arduino Nano

// Raspberry Pi 0/4 and host dev machines run under Linux/macOS/Windows
// and get `std`. ESP32 and AVR targets have no OS underneath this
// crate, so it must stay `no_std` there.
#![cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    no_std
)]

pub mod variants;

pub use variants::{HighResTimer, PlatformTimer};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_timer_ticks_forward() {
        let mut timer = PlatformTimer;
        timer.init();

        let a = timer.tick();
        // Busy-loop briefly, so there's guaranteed forward progress to
        // observe, without depending on std::thread::sleep semantics
        // matching across every future non-host backend.
        let mut acc: u64 = 0;
        for i in 0..100_000u64 {
            acc = acc.wrapping_add(i);
        }
        core::hint::black_box(acc);
        let b = timer.tick();

        assert!(
            b.wrapping_sub(a) > 0,
            "tick() did not advance: a={a}, b={b}"
        );
    }
}
