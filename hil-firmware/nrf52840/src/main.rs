//! QPP-RNG hardware-in-the-loop firmware for the makerdiary nRF52840 MDK.
//!
//! Same protocol as `../avr`: one ASCII header line, then `0xA5` + two
//! little-endian `u32`s per record, over UART0 at 115200 baud on the
//! pins wired to the board's DAPLink USB-serial bridge (TXD = P0.20).
//! The host resets the board through the debug probe (`probe-rs reset`)
//! once the serial port is open, and then the probe stays idle -- the
//! data path is the UART, not RTT, so debugger SWD bus traffic can't
//! leak into the timing being measured.

#![no_std]
#![no_main]
// The jitter-source helpers are shared by the `rawsrc` and `iot` modes
// and unused by the reference-trace build.
#![cfg_attr(not(any(feature = "iot", feature = "rawsrc")), allow(dead_code))]

use core::ptr::{read_volatile, write_volatile};

use cortex_m_rt::entry;

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
use qpp_rng_reference::QppRngXorshift;
#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
use rng_core::QppRngSource;

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

const TX_PIN: u32 = 20;

const P0_OUTSET: *mut u32 = 0x5000_0508 as *mut u32;
const P0_DIRSET: *mut u32 = 0x5000_0518 as *mut u32;

// Legacy (non-EasyDMA) UART0, so TX needs no RAM buffer.
const UART0: usize = 0x4000_2000;
const UART_STARTTX: *mut u32 = (UART0 + 0x008) as *mut u32;
const UART_TXDRDY: *mut u32 = (UART0 + 0x11C) as *mut u32;
const UART_ENABLE: *mut u32 = (UART0 + 0x500) as *mut u32;
const UART_PSELRTS: *mut u32 = (UART0 + 0x508) as *mut u32;
const UART_PSELTXD: *mut u32 = (UART0 + 0x50C) as *mut u32;
const UART_PSELCTS: *mut u32 = (UART0 + 0x510) as *mut u32;
const UART_PSELRXD: *mut u32 = (UART0 + 0x514) as *mut u32;
const UART_TXD: *mut u32 = (UART0 + 0x51C) as *mut u32;
const UART_BAUDRATE: *mut u32 = (UART0 + 0x524) as *mut u32;
const UART_CONFIG: *mut u32 = (UART0 + 0x56C) as *mut u32;
const BAUD_115200: u32 = 0x01D7_E000;

const CLOCK_HFCLKSTART: *mut u32 = 0x4000_0000 as *mut u32;
const CLOCK_HFCLKSTARTED: *mut u32 = 0x4000_0100 as *mut u32;

fn clock_init() {
    // Start the 32 MHz crystal so the UART baud rate is accurate. CPU
    // timing is unaffected either way: DWT->CYCCNT counts CPU cycles.
    unsafe {
        write_volatile(CLOCK_HFCLKSTARTED, 0);
        write_volatile(CLOCK_HFCLKSTART, 1);
        while read_volatile(CLOCK_HFCLKSTARTED) == 0 {}
    }
}

fn uart_init() {
    unsafe {
        write_volatile(P0_OUTSET, 1 << TX_PIN);
        write_volatile(P0_DIRSET, 1 << TX_PIN);
        write_volatile(UART_PSELTXD, TX_PIN);
        write_volatile(UART_PSELRXD, 0xFFFF_FFFF);
        write_volatile(UART_PSELRTS, 0xFFFF_FFFF);
        write_volatile(UART_PSELCTS, 0xFFFF_FFFF);
        write_volatile(UART_BAUDRATE, BAUD_115200);
        write_volatile(UART_CONFIG, 0);
        write_volatile(UART_ENABLE, 4);
        write_volatile(UART_STARTTX, 1);
    }
}

fn uart_write(b: u8) {
    unsafe {
        write_volatile(UART_TXDRDY, 0);
        write_volatile(UART_TXD, b as u32);
        while read_volatile(UART_TXDRDY) == 0 {}
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

#[entry]
fn main() -> ! {
    clock_init();
    uart_init();
    // Give the host a moment after the probe reset before the header.
    for _ in 0..2_000_000 {
        cortex_m_rt_nop();
    }

    #[cfg(not(any(feature = "iot", feature = "rawsrc")))]
    run_reference();
    #[cfg(feature = "rawsrc")]
    run_rawsrc();
    #[cfg(feature = "iot")]
    run_iot();
}

// LFRC/RTC helpers (rawsrc only) and RNG helpers (rawsrc and iot).
#[cfg(feature = "rawsrc")]
const CLOCK_LFCLKSTART: *mut u32 = 0x4000_0008 as *mut u32;
// LFRC/RTC helpers (rawsrc only) and RNG helpers (rawsrc and iot).
#[cfg(feature = "rawsrc")]
const CLOCK_LFCLKSTARTED: *mut u32 = 0x4000_0104 as *mut u32;
#[cfg(feature = "rawsrc")]
const CLOCK_LFCLKSRC: *mut u32 = 0x4000_0518 as *mut u32;
#[cfg(feature = "rawsrc")]
const RTC0_START: *mut u32 = 0x4000_B000 as *mut u32;
#[cfg(feature = "rawsrc")]
const RTC0_COUNTER: *mut u32 = 0x4000_B504 as *mut u32;
#[cfg(feature = "rawsrc")]
const RTC0_PRESCALER: *mut u32 = 0x4000_B508 as *mut u32;

/// RTC ticks (32.768 kHz LFRC periods) accumulated per jitter sample.
#[cfg(feature = "rawsrc")]
const RTC_TICKS_PER_SAMPLE: u32 = 32;

/// Starts the LFCLK from its internal RC oscillator (LFRC) -- a clock
/// domain physically independent of the CPU's HFCLK -- and runs RTC0
/// off it with no prescaling.
#[cfg(feature = "rawsrc")]
fn lfrc_rtc_init() {
    unsafe {
        write_volatile(CLOCK_LFCLKSRC, 0); // RC
        write_volatile(CLOCK_LFCLKSTARTED, 0);
        write_volatile(CLOCK_LFCLKSTART, 1);
        while read_volatile(CLOCK_LFCLKSTARTED) == 0 {}
        write_volatile(RTC0_PRESCALER, 0);
        write_volatile(RTC0_START, 1);
    }
}

#[cfg(feature = "rawsrc")]
fn rtc_counter() -> u32 {
    unsafe { read_volatile(RTC0_COUNTER) }
}

/// Busy-waits until RTC0 has advanced `ticks` times past its value on
/// entry, then returns the CPU cycle counter at the moment of the last
/// edge.
#[cfg(feature = "rawsrc")]
fn cycles_at_rtc_edge<T: entropy_timer::HighResTimer>(timer: &mut T, ticks: u32) -> u32 {
    let start = rtc_counter();
    let target = start.wrapping_add(ticks) & 0x00FF_FFFF;
    while rtc_counter() != target {}
    timer.tick() as u32
}

#[cfg(any(feature = "rawsrc", feature = "iot"))]
const RNG_START: *mut u32 = 0x4000_D000 as *mut u32;
#[cfg(any(feature = "rawsrc", feature = "iot"))]
const RNG_VALRDY: *mut u32 = 0x4000_D100 as *mut u32;
#[cfg(any(feature = "rawsrc", feature = "iot"))]
const RNG_CONFIG: *mut u32 = 0x4000_D504 as *mut u32;
#[cfg(any(feature = "rawsrc", feature = "iot"))]
const RNG_VALUE: *mut u32 = 0x4000_D508 as *mut u32;

#[cfg(any(feature = "rawsrc", feature = "iot"))]
fn hwrng_init() {
    unsafe {
        write_volatile(RNG_CONFIG, 1); // digital bias correction on
        write_volatile(RNG_VALRDY, 0);
        write_volatile(RNG_START, 1);
    }
}

#[cfg(any(feature = "rawsrc", feature = "iot"))]
fn hwrng_byte() -> u8 {
    unsafe {
        while read_volatile(RNG_VALRDY) == 0 {}
        let v = read_volatile(RNG_VALUE) as u8;
        write_volatile(RNG_VALRDY, 0);
        v
    }
}

#[cfg(feature = "rawsrc")]
fn run_rawsrc() -> ! {
    use entropy_timer::{HighResTimer, PlatformTimer};
    let mut timer = PlatformTimer;
    timer.init();
    lfrc_rtc_init();
    hwrng_init();
    uart_str("QPPHIL nrf52840 rawsrc lfrc-rtc32-vs-cyccnt+hwrng\n");
    let mut prev = cycles_at_rtc_edge(&mut timer, 1);
    loop {
        let t = cycles_at_rtc_edge(&mut timer, RTC_TICKS_PER_SAMPLE);
        let r = hwrng_byte();
        record(t.wrapping_sub(prev), r as u32);
        prev = t;
    }
}

#[inline(always)]
fn cortex_m_rt_nop() {
    unsafe { core::arch::asm!("nop") };
}

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
fn run_reference() -> ! {
    let mut rng = QppRngXorshift::from_seed(SEED).with_oversample(1);
    uart_str("QPPHIL nrf52840 ref-xorshift128plus N=5\n");
    loop {
        let _ = rng.next_byte();
        let d = rng.diagnostics();
        record(d.last_permutation_count as u32, d.last_jitter_ns.unwrap_or(0) as u32);
    }
}

/// The on-chip RNG peripheral (thermal noise, digital bias correction
/// on) as the nRF52840 [`qpp_rng_iot::JitterSource`]. The LFRC-vs-CYCCNT
/// cross-clock source assessed at only ~0.6 bits/sample on this board
/// (the 32.768 kHz RC oscillator is far more stable than the AVR
/// watchdog's, so RTC-polling quantization dominates), against ~6.9
/// bits/byte for this peripheral -- see REPORT.md section 2.
#[cfg(feature = "iot")]
struct HwRng;

#[cfg(feature = "iot")]
impl HwRng {
    fn new() -> Self {
        hwrng_init();
        Self
    }
}

#[cfg(feature = "iot")]
impl qpp_rng_iot::JitterSource for HwRng {
    fn sample(&mut self) -> u32 {
        hwrng_byte() as u32
    }
}

/// Min-entropy credited per RNG byte: NIST `ea_non_iid` assessed 6.89
/// bits/byte (bitstring compression estimate binding); 4 is the highest
/// value the health-test table covers and leaves ~2.9 bits of margin.
#[cfg(feature = "iot")]
const CREDITED_BITS: u8 = 4;

#[cfg(feature = "iot")]
fn run_iot() -> ! {
    qpp_hil_common::run_iot::<Uart0, _, _>(
        entropy_timer::PlatformTimer,
        qpp_hil_common::IotConfig {
            header: "QPPHIL nrf52840 iot-hwrng N=6 credit=",
            credited_bits: CREDITED_BITS,
            bench_bytes: 256,
        },
        HwRng::new,
    )
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    uart_str("PANIC\n");
    loop {}
}
