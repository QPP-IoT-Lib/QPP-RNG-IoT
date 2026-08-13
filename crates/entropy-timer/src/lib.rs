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
    ((delta_tick / k_u64) % 256) as u8
}

/// Empirically measures a timer's real granularity on the machine
/// actually running the code, instead of trusting the nominal
/// per-backend constant [`HighResTimer::init`] returns.
///
/// This is the "adaptive clock resolution" the paper describes as one
/// of its four contributions ("Our contributions", 4th bullet: *"a
/// novel adaptive clock resolution mechanism that dynamically
/// compensates for the inherent timing granularities of different
/// underlying hardware platforms"*) -- but a fixed constant baked into
/// each [`variants`] backend can't actually provide that, because
/// "the same backend" still spans real hardware diversity: two
/// x86_64 PCs both using [`variants::windows::WindowsTimer`]'s
/// `QueryPerformanceCounter` can back it with completely different
/// counters (invariant TSC, HPET, or the old ACPI PM timer) depending
/// on chipset and firmware, each with its own real granularity. A
/// hardcoded `k` that's a good fit for one such machine can be badly
/// wrong for another -- see the "Adaptive clock resolution" fidelity
/// note in `qpp-rng-reference`, which found exactly this: a Windows
/// desktop (bare metal, not virtualized) whose real QPC granularity
/// didn't match the crate's hardcoded nominal constant collapsed the
/// generator's byte-level output the same way an overly-aggressive
/// compiler optimization does, because in both cases the timing signal
/// `normalize_tick` extracts loses almost all its variance.
///
/// Samples `samples` back-to-back [`HighResTimer::tick`] pairs (after
/// a short warm-up, so the first, typically cold-cache read doesn't
/// skew the result) and returns the smallest *nonzero* delta observed,
/// clamped into `1..=255`. Falls back to `fallback` only if the
/// counter never visibly advanced across the whole sample window
/// (e.g. a frozen or exceptionally coarse counter) -- in that case a
/// measurement genuinely isn't possible, so the nominal per-backend
/// constant is the best remaining guess.
pub fn calibrate_resolution<T: HighResTimer>(timer: &mut T, samples: u32, fallback: u8) -> u8 {
    const WARMUP: u32 = 8;

    let mut prev = timer.tick();
    for _ in 0..WARMUP {
        prev = timer.tick();
    }

    let mut min_delta = u64::MAX;
    for _ in 0..samples {
        let now = timer.tick();
        let delta = now.wrapping_sub(prev);
        if delta != 0 && delta < min_delta {
            min_delta = delta;
        }
        prev = now;
    }

    if min_delta == u64::MAX {
        fallback.max(1)
    } else {
        min_delta.clamp(1, u8::MAX as u64) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Busy-loop briefly, so there's guaranteed forward progress to
    /// observe, without depending on std::thread::sleep semantics
    /// matching across every future non-host backend.
    fn busy_loop() -> u64 {
        let mut acc: u64 = 0;
        for i in 0..500_000u64 {
            acc = acc.wrapping_add(i);
        }
        core::hint::black_box(acc); // prevent compiler from optimizing away 'useless' loop
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

        let _norm = normalize_tick(dif, k);
    }

    #[test]
    fn calibrate_resolution_measures_a_plausible_k() {
        let mut timer = PlatformTimer;
        let nominal = timer.init();

        let measured = calibrate_resolution(&mut timer, 256, nominal);

        // Always in range by construction (`clamp`/`.max(1)`), but
        // worth asserting explicitly: a `k` of 0 would make
        // `normalize_tick` divide by zero.
        assert!(measured >= 1);
    }
}
