//! Shared parts of the QPP-RNG hardware-in-the-loop firmware.
//!
//! Every firmware prints one ASCII header line starting with `QPPHIL`,
//! then streams records of `0xA5` followed by two little-endian `u32`s
//! (`hil-firmware/tools/capture.py` parses exactly this). The helpers
//! here write that protocol through a board-provided [`Uart`], and
//! [`run_iot`] is the whole `iot` mode: the part that is identical on
//! every board.

#![no_std]

/// A blocking byte sink, implemented by each board over its UART.
///
/// An associated function, not a method: the UART is a fixed set of
/// memory-mapped registers, so no state is passed around.
pub trait Uart {
    fn write(byte: u8);
}

#[inline(always)]
pub fn write_str<U: Uart>(s: &str) {
    for b in s.bytes() {
        U::write(b);
    }
}

#[inline(always)]
pub fn write_dec<U: Uart>(mut v: u32) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for &b in &buf[i..] {
        U::write(b);
    }
}

/// One stream record: sync byte, then `a` and `b` little-endian.
#[inline(always)]
pub fn record<U: Uart>(a: u32, b: u32) {
    U::write(0xA5);
    for x in a.to_le_bytes() {
        U::write(x);
    }
    for x in b.to_le_bytes() {
        U::write(x);
    }
}

#[cfg(feature = "iot")]
pub use iot::{IotConfig, run_iot};

#[cfg(feature = "iot")]
mod iot {
    use super::{Uart, record, write_dec, write_str};
    use entropy_timer::HighResTimer;
    use qpp_rng_iot::{DEFAULT_ARRAY_SIZE, JitterSource, QppRngIot};

    // The header below hard-codes the permutation width.
    const _: () = assert!(DEFAULT_ARRAY_SIZE == 6, "update the `N=6` in the iot header");

    /// Instant synthetic source for the startup compute benchmark:
    /// isolates the convergence-walk cost from the time spent waiting
    /// on the real jitter source.
    struct Synthetic(u32);

    impl JitterSource for Synthetic {
        fn sample(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }
    }

    pub struct IotConfig<'a> {
        /// Header text up to, and including, `credit=`; e.g.
        /// `"QPPHIL avr iot-wdt N=6 credit="`.
        pub header: &'a str,
        /// Min-entropy credited per raw sample of the real source.
        pub credited_bits: u8,
        /// Bytes generated from the synthetic source to time the walk.
        pub bench_bytes: u32,
    }

    /// The `iot` mode: time the generator's compute cost on a synthetic
    /// source, then stream records of `(4 output bytes, ticks taken)`
    /// from the real source forever. A failed health test prints
    /// `HEALTH-FAIL` and halts.
    ///
    /// `make_source` runs only after the benchmark, so sources that
    /// start a free-running background mechanism (the AVR watchdog
    /// interrupt) don't disturb the timing measurement.
    #[inline(always)]
    pub fn run_iot<U: Uart, T: HighResTimer, J: JitterSource>(
        mut clock: T,
        cfg: IotConfig<'_>,
        make_source: impl FnOnce() -> J,
    ) -> ! {
        clock.init();

        // Compute-only cost, with the same credit (so the same walks and
        // samples per byte) as the real source.
        let mut bench = QppRngIot::<_>::new(Synthetic(0x1234_5678), cfg.credited_bits);
        let _ = bench.next_byte(); // priming cycles excluded
        let t0 = clock.tick() as u32;
        for _ in 0..cfg.bench_bytes {
            let _ = core::hint::black_box(bench.next_byte());
        }
        let compute_ticks_per_byte = (clock.tick() as u32).wrapping_sub(t0) / cfg.bench_bytes;

        let mut rng = QppRngIot::<_>::new(make_source(), cfg.credited_bits);
        write_str::<U>(cfg.header);
        write_dec::<U>(cfg.credited_bits as u32);
        write_str::<U>(" samples_per_byte=");
        write_dec::<U>(rng.samples_per_byte() as u32);
        write_str::<U>(" compute_ticks_per_byte=");
        write_dec::<U>(compute_ticks_per_byte);
        write_str::<U>("\n");
        loop {
            let t0 = clock.tick() as u32;
            let mut w = [0u8; 4];
            for b in w.iter_mut() {
                match rng.next_byte() {
                    Ok(v) => *b = v,
                    Err(_) => {
                        write_str::<U>("HEALTH-FAIL\n");
                        loop {}
                    }
                }
            }
            let t1 = clock.tick() as u32;
            record::<U>(u32::from_le_bytes(w), t1.wrapping_sub(t0));
        }
    }
}
