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

/// Normalization based on the implementation proposed by Vrana et al. (2025).
///
/// - Param: delta_tick = tick_f - tick_i
/// - Param: k = native timer resolution
/// - Ret: normalized = floor(delta_tick / k) mod 256
pub fn normalize_tick(delta_tick: u64, k: u8) -> u8 {
    let k_u64 = u64::from(k);
    ( (delta_tick / k_u64 ) % 256 ) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Busy-loop briefly, so there's guaranteed forward progress to
    /// observe, without depending on std::thread::sleep semantics
    /// matching across every future non-host backend.
    fn busy_loop() -> u64 {
        let mut acc: u64 = 0;
        for i in 0..100_000u64 {
            acc = acc.wrapping_add(i);
        }
        acc
    }

    #[test]
    fn platform_timer_ticks_forward() {
        let mut timer = PlatformTimer;
        let _k = timer.init();

        let a = timer.tick();

        let acc = busy_loop();
        core::hint::black_box(acc); // prevent compiler from optimizing away 'useless' loop

        let b = timer.tick();

        assert!(
            b.wrapping_sub(a) > 0,
            "tick() did not advance: a={a}, b={b}"
        );
    }

    #[test]
    fn platform_timer_ticks_normalize() {
        let mut timer = PlatformTimer;
        let k = timer.init();
        let a = timer.tick();

        let acc = busy_loop();

        core::hint::black_box(acc);
        let b = timer.tick();

        let dif = b.wrapping_sub(a);

        let norm = normalize_tick( dif, k );

        assert_ne!(norm, 0, "tick() did not normalize: a={a}, b={b}, dif={dif}, norm={norm}")
    }
}
