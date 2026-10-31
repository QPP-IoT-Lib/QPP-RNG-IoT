//! QPP-RNG hardware-in-the-loop firmware for the ESP32.
//!
//! Same protocol as `../avr` and `../nrf52840`: one ASCII header line,
//! then `0xA5` + two little-endian `u32`s per record, over UART0 at
//! 115200 baud (TXD = GPIO1, wired to the board's USB-serial bridge).
//! The ROM and second-stage bootloader print their own lines first;
//! `capture.py` skips everything before the `QPPHIL` line. The firmware
//! runs on core 0 at 240 MHz with the radio off and core 1 parked.

#![no_std]
#![no_main]
#![cfg_attr(not(any(feature = "iot", feature = "rawsrc")), allow(dead_code))]

use core::ptr::addr_of_mut;

use esp_hal::{
    Blocking, Config,
    clock::CpuClock,
    uart::{self, UartTx},
};

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
use qpp_rng_reference::QppRngXorshift;
#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
use rng_core::QppRngSource;

esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

static mut TX: Option<UartTx<'static, Blocking>> = None;

/// This board's UART, as the byte sink the shared protocol code writes to.
struct Uart0;

impl qpp_hil_common::Uart for Uart0 {
    fn write(byte: u8) {
        // SAFETY: one core, no interrupts, set once at the top of `main`.
        if let Some(tx) = unsafe { &mut *addr_of_mut!(TX) } {
            while !matches!(tx.write(&[byte]), Ok(1)) {}
        }
    }
}

fn uart_str(s: &str) {
    qpp_hil_common::write_str::<Uart0>(s);
}

#[cfg(not(feature = "iot"))]
fn record(a: u32, b: u32) {
    qpp_hil_common::record::<Uart0>(a, b);
}

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(Config::default().with_cpu_clock(CpuClock::max()));
    let tx = UartTx::new(p.UART0, uart::Config::default().with_baudrate(115_200))
        .unwrap()
        .with_tx(p.GPIO1);
    // SAFETY: nothing else has run yet.
    unsafe { *addr_of_mut!(TX) = Some(tx) };

    #[cfg(not(any(feature = "iot", feature = "rawsrc")))]
    run_reference();
    #[cfg(feature = "rawsrc")]
    {
        let timg = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
        core::mem::forget(timg);
        #[cfg(not(feature = "rawsrc-noadc"))]
        core::mem::forget(esp_hal::rng::TrngSource::new(p.RNG, p.ADC1));
        run_rawsrc();
    }
    #[cfg(feature = "iot")]
    {
        core::mem::forget(esp_hal::rng::TrngSource::new(p.RNG, p.ADC1));
        run_iot();
    }
}

#[cfg(not(any(feature = "iot", feature = "rawsrc")))]
fn run_reference() -> ! {
    let mut rng = QppRngXorshift::from_seed(SEED).with_oversample(1);
    uart_str("QPPHIL esp32 ref-xorshift128plus N=5\n");
    loop {
        let _ = rng.next_byte();
        let d = rng.diagnostics();
        record(d.last_permutation_count as u32, d.last_jitter_ns.unwrap_or(0) as u32);
    }
}

// Timer group 0's calibration unit: counts crystal cycles over a number
// of cycles of a slow clock, here the RTC slow clock (150 kHz RC).
#[cfg(feature = "rawsrc")]
const TIMG0_RTCCALICFG: *mut u32 = 0x3FF5_F068 as *mut u32;
#[cfg(feature = "rawsrc")]
const TIMG0_RTCCALICFG1: *const u32 = 0x3FF5_F06C as *const u32;
#[cfg(feature = "rawsrc")]
const CALI_RDY: u32 = 1 << 15;
#[cfg(feature = "rawsrc")]
const CALI_START: u32 = 1 << 31;

/// RTC slow clock cycles per jitter sample (about 1.7 ms).
#[cfg(feature = "rawsrc")]
const SLOW_CYCLES_PER_SAMPLE: u32 = 256;

/// 40 MHz crystal cycles taken by `SLOW_CYCLES_PER_SAMPLE` RTC slow cycles.
#[cfg(feature = "rawsrc")]
fn rtc_slow_period() -> u32 {
    use core::ptr::{read_volatile, write_volatile};
    unsafe {
        write_volatile(TIMG0_RTCCALICFG, SLOW_CYCLES_PER_SAMPLE << 16);
        write_volatile(TIMG0_RTCCALICFG, SLOW_CYCLES_PER_SAMPLE << 16 | CALI_START);
        // RDY stays set from the previous run for a moment after START.
        while read_volatile(TIMG0_RTCCALICFG) & CALI_RDY != 0 {}
        while read_volatile(TIMG0_RTCCALICFG) & CALI_RDY == 0 {}
        read_volatile(TIMG0_RTCCALICFG1) >> 7
    }
}

#[cfg(feature = "rawsrc")]
fn run_rawsrc() -> ! {
    #[cfg(not(feature = "rawsrc-noadc"))]
    uart_str("QPPHIL esp32 rawsrc rtcslow256-vs-xtal+hwrng-adc\n");
    #[cfg(feature = "rawsrc-noadc")]
    uart_str("QPPHIL esp32 rawsrc rtcslow256-vs-xtal+hwrng-noadc\n");
    let rng = esp_hal::rng::Rng::new();
    loop {
        let t = rtc_slow_period();
        record(t, rng.random());
    }
}

/// The on-chip RNG, with the SAR ADC noise source on, as the ESP32
/// [`qpp_rng_iot::JitterSource`]. One byte per sample, as assessed.
#[cfg(feature = "iot")]
struct HwRng(esp_hal::rng::Rng);

#[cfg(feature = "iot")]
impl qpp_rng_iot::JitterSource for HwRng {
    fn sample(&mut self) -> u32 {
        self.0.random() & 0xFF
    }
}

/// Min-entropy credited per RNG byte; see `hil-results/REPORT.md`.
#[cfg(feature = "iot")]
const CREDITED_BITS: u8 = 4;

#[cfg(feature = "iot")]
fn run_iot() -> ! {
    qpp_hil_common::run_iot::<Uart0, _, _>(
        entropy_timer::PlatformTimer,
        qpp_hil_common::IotConfig {
            header: "QPPHIL esp32 iot-hwrng N=6 credit=",
            credited_bits: CREDITED_BITS,
            bench_bytes: 256,
        },
        || HwRng(esp_hal::rng::Rng::new()),
    )
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    uart_str("PANIC\n");
    loop {}
}
