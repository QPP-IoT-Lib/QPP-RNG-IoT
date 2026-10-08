//! QPP-RNG hardware-in-the-loop firmware for AVR Arduinos.
//!
//! Boots, prints one ASCII header line, then streams fixed-size binary
//! records over USART0 at 115200 baud until reset. The host side
//! (`hil-firmware/tools/capture.py`) resets the board by opening the
//! port (Arduino auto-reset on DTR), so every capture starts from the
//! same fixed seed -- which is exactly what makes the cross-reset
//! reproducibility check meaningful.
//!
//! Record format: `0xA5`, then two little-endian `u32`s.
//! - reference build: `n_p` and `Δt` (Timer1 ticks) of one convergence
//!   cycle (`oversample = 1`, so every cycle is observed)
//! - `rawsrc` build: one watchdog period in Timer1 ticks, and its timestamp
//! - `iot` build: a 4-byte output word and the Timer1 ticks it took

#![no_std]
#![no_main]
// The jitter-source helpers are shared by the `rawsrc` and `iot` modes
// and unused by the reference-trace build.
#![cfg_attr(not(any(feature = "iot", feature = "rawsrc")), allow(dead_code))]
#![feature(asm_experimental_arch)]

use core::ptr::{read_volatile, write_volatile};

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
use qpp_rng_reference::QppRngXorshift;
#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
use rng_core::QppRngSource;

const F_CPU: u32 = 16_000_000;
#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

// USART0 lives at the same data-space addresses on the ATmega328P and
// the ATmega2560.
const UCSR0A: *mut u8 = 0xC0 as *mut u8;
const UCSR0B: *mut u8 = 0xC1 as *mut u8;
const UCSR0C: *mut u8 = 0xC2 as *mut u8;
const UBRR0L: *mut u8 = 0xC4 as *mut u8;
const UBRR0H: *mut u8 = 0xC5 as *mut u8;
const UDR0: *mut u8 = 0xC6 as *mut u8;
const U2X0: u8 = 1;
const UDRE0: u8 = 5;
const TXEN0: u8 = 3;

fn uart_init() {
    // 115200 baud with U2X: UBRR = F_CPU / (8 * baud) - 1 = 16 (2.1% error,
    // the same setting the Arduino core uses).
    let ubrr: u16 = (F_CPU / 8 / 115_200 - 1) as u16;
    unsafe {
        write_volatile(UCSR0A, 1 << U2X0);
        write_volatile(UBRR0H, (ubrr >> 8) as u8);
        write_volatile(UBRR0L, ubrr as u8);
        write_volatile(UCSR0C, 0b0000_0110); // 8N1
        write_volatile(UCSR0B, 1 << TXEN0);
    }
}

fn uart_write(b: u8) {
    unsafe {
        while read_volatile(UCSR0A) & (1 << UDRE0) == 0 {}
        write_volatile(UDR0, b);
    }
}

/// This board's UART, as the byte sink the shared protocol code writes to.
struct Uart0;

impl qpp_hil_common::Uart for Uart0 {
    fn write(byte: u8) {
        uart_write(byte);
    }
}

fn uart_str(s: &str) {
    qpp_hil_common::write_str::<Uart0>(s);
}

#[cfg(not(feature = "iot"))]
fn record(a: u32, b: u32) {
    qpp_hil_common::record::<Uart0>(a, b);
}

fn delay_cycles(n: u32) {
    for _ in 0..n {
        unsafe { core::arch::asm!("nop") };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    uart_init();
    // Interrupts on: entropy-timer's Timer1 overflow ISR extends the
    // 16-bit counter.
    unsafe { core::arch::asm!("sei") };
    // Let the host finish opening the port after the auto-reset, so the
    // header isn't lost.
    delay_cycles(400_000);

    #[cfg(not(any(feature = "iot", feature = "rawsrc")))]
    run_reference();
    #[cfg(feature = "rawsrc")]
    run_rawsrc();
    #[cfg(feature = "iot")]
    run_iot();
}

unsafe extern "C" {
    fn qpp_wdt_init();
    fn qpp_wdt_take(out: *mut u32) -> u8;
}

/// Blocks until the next watchdog interrupt and returns its Timer1
/// timestamp.
fn wdt_next() -> u32 {
    let mut t = 0u32;
    while unsafe { qpp_wdt_take(&mut t) } == 0 {}
    t
}

#[cfg(feature = "rawsrc")]
fn run_rawsrc() -> ! {
    use entropy_timer::{HighResTimer, PlatformTimer};
    let mut timer = PlatformTimer;
    timer.init();
    unsafe { qpp_wdt_init() };
    uart_str("QPPHIL avr rawsrc wdt-vs-timer1\n");
    let mut prev = wdt_next();
    loop {
        let t = wdt_next();
        record(t.wrapping_sub(prev), t);
        prev = t;
    }
}

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
fn run_reference() -> ! {
    // The paper's default configuration, exactly as the host harness
    // builds it, but with oversample = 1 so every convergence cycle's
    // (n_p, Δt) is observable. The default oversample-5 output stream is
    // the XOR of 5 consecutive n_p mod 256 values, so it is fully
    // reconstructible from this trace on the host.
    let mut rng = QppRngXorshift::from_seed(SEED).with_oversample(1);
    uart_str("QPPHIL avr ref-xorshift128plus N=5\n");
    loop {
        let _ = rng.next_byte();
        let d = rng.diagnostics();
        record(d.last_permutation_count as u32, d.last_jitter_ns.unwrap_or(0) as u32);
    }
}

/// Watchdog RC oscillator period, measured in crystal-clocked Timer1
/// ticks: the AVR [`qpp_rng_iot::JitterSource`].
#[cfg(feature = "iot")]
struct WdtJitter {
    prev: u32,
}

#[cfg(feature = "iot")]
impl WdtJitter {
    fn new() -> Self {
        use entropy_timer::{HighResTimer, PlatformTimer};
        PlatformTimer.init();
        unsafe { qpp_wdt_init() };
        Self { prev: wdt_next() }
    }
}

#[cfg(feature = "iot")]
impl qpp_rng_iot::JitterSource for WdtJitter {
    fn sample(&mut self) -> u32 {
        let t = wdt_next();
        let d = t.wrapping_sub(self.prev);
        self.prev = t;
        d
    }
}

/// Min-entropy credited per watchdog-period sample. NIST `ea_non_iid`
/// on 1 M samples assessed 1.62 bits/sample on the Mega 2560 and 2.24 on
/// the Nano (t-tuple binding on both; 2.14 and 2.98 on the first 20k
/// samples). Credit 1 bit, leaving a 1.6x margin on the lower board.
#[cfg(feature = "iot")]
const CREDITED_BITS: u8 = 1;

#[cfg(feature = "iot")]
fn run_iot() -> ! {
    qpp_hil_common::run_iot::<Uart0, _, _>(
        entropy_timer::PlatformTimer,
        qpp_hil_common::IotConfig {
            header: "QPPHIL avr iot-wdt N=6 credit=",
            credited_bits: CREDITED_BITS,
            bench_bytes: 32,
        },
        WdtJitter::new,
    )
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    uart_str("PANIC\n");
    loop {}
}
