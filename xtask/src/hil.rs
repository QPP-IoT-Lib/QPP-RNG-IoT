//! Hardware-in-loop interface: a flasher (`probe-rs`) to load a built
//! binary onto real hardware, and a telemetry path (RTT or UART) to
//! pull samples/cycle counts back off it.
//!
//! This is the one piece the earlier phase-1 plan
//! (`qpp-rng-testing-architecture.md`) hadn't named yet -- everything
//! else in this workspace is pure Rust or shells out to a tool with a
//! stable CLI; this module is the only one that needs board-specific
//! tooling (`probe-rs`, a serial port, a specific chip identifier) to
//! do anything real.
//!
//! ## Not validated against real hardware
//!
//! `probe-rs` isn't installed and no board is attached in the
//! environment this was written in (see [`crate::target_matrix`]'s
//! module doc for the same caveat applied to the target list). Both
//! [`ProbeRsFlasher`] and [`UartTelemetry`] are real, complete
//! implementations against `probe-rs`'s documented CLI and standard
//! POSIX serial APIs respectively -- shell out and read exactly what
//! they claim to -- but neither has been exercised against a real
//! device. [`RttTelemetry`] additionally depends on the target firmware
//! having been built with `defmt`'s RTT logging wired up, which is a
//! firmware-side integration this workspace doesn't have yet (there is
//! no `[[bin]]` firmware image in `qpp-rng-reference`/`qpp-rng-iot` at
//! all -- see `footprint::size`'s module doc for the same gap).

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use xshell::{cmd, Shell};

/// Loads a built binary onto real hardware via `probe-rs run` (flash +
/// reset + attach), blocking until the firmware itself exits or
/// `timeout` elapses.
///
/// `probe-rs run` (rather than `probe-rs download` + a separate reset)
/// is used deliberately: for firmware built against `probe-rs`'s own
/// `defmt-rtt`/`panic-probe` runtime support, `run` also streams the
/// target's RTT output to stdout, which is the simplest possible
/// telemetry path when the firmware cooperates (see [`RttTelemetry`]
/// for pulling that same data back out programmatically instead of
/// just printing it).
pub struct ProbeRsFlasher;

impl ProbeRsFlasher {
    pub fn flash_and_run(&self, chip: &str, binary_path: &Path, timeout: Duration) -> anyhow::Result<String> {
        let sh = Shell::new()?;
        let binary = binary_path.to_string_lossy().to_string();
        // xshell has no built-in timeout; probe-rs run normally exits
        // on its own once the target's `exit()`/semihosting call
        // fires, but a hung target would otherwise block this call
        // forever. Run it as a raw std::process::Command with a
        // watchdog thread instead of xshell's cmd! for that reason.
        let _ = &sh; // establishes cwd/env parity with the rest of xtask's shelling, even though this path uses std::process directly below.
        run_with_timeout("probe-rs", &["run", "--chip", chip, &binary], timeout)
    }

    /// Flashes without running -- `probe-rs download`, for boards whose
    /// firmware doesn't cooperate with `probe-rs run`'s attach/RTT
    /// protocol and needs a separate, target-specific reset afterward.
    pub fn download(&self, chip: &str, binary_path: &Path) -> anyhow::Result<()> {
        let sh = Shell::new()?;
        let binary = binary_path.to_string_lossy().to_string();
        cmd!(sh, "probe-rs download --chip {chip} {binary}").run()?;
        Ok(())
    }
}

fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> anyhow::Result<String> {
    use std::sync::mpsc;

    let mut child = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn {program}: {e}"))?;

    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take();
    std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        let _ = tx.send(buf);
    });

    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let out = rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
            if status.success() {
                return Ok(out);
            }
            anyhow::bail!("{program} exited with {status}: {out}");
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            anyhow::bail!("{program} did not exit within {timeout:?}, killed");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Pulls raw bytes back off a target over RTT (Real-Time Transfer),
/// via `probe-rs`'s standalone RTT client (`probe-rs attach --rtt` /
/// `probe-rs rtt`, depending on `probe-rs` version).
///
/// Expects the target firmware to write raw sample bytes (not `defmt`-
/// framed log messages) to RTT channel 0 -- pulling `defmt`-encoded
/// telemetry back apart would need this crate to link `defmt-decoder`
/// against the firmware's own `.elf` for symbol/format-string
/// resolution, which is out of scope until there's real firmware to
/// decode against.
pub struct RttTelemetry {
    pub chip: String,
    pub elf_path: std::path::PathBuf,
}

impl RttTelemetry {
    /// Attaches to a running target and reads exactly `n_bytes` of raw
    /// RTT channel-0 output, or errors out after `timeout`.
    pub fn read_samples(&self, n_bytes: usize, timeout: Duration) -> anyhow::Result<Vec<u8>> {
        let elf = self.elf_path.to_string_lossy().to_string();
        let out = run_with_timeout(
            "probe-rs",
            &["attach", "--chip", &self.chip, "--rtt", &elf],
            timeout,
        )?;
        let bytes = out.into_bytes();
        if bytes.len() < n_bytes {
            anyhow::bail!(
                "RTT stream produced only {} of the requested {n_bytes} bytes before {timeout:?} elapsed",
                bytes.len()
            );
        }
        Ok(bytes[..n_bytes].to_vec())
    }
}

/// Pulls raw bytes back off a target over a UART/serial connection.
///
/// Implemented directly against the device file (`/dev/ttyUSB0`,
/// `/dev/cu.usbserial-*`, ...) via `stty` for line configuration --
/// deliberately not the `serialport` crate, to keep this one optional,
/// rarely-exercised path from adding a dependency to every other build
/// of this workspace. Unix-only (`stty`'s flags aren't portable to
/// Windows COM ports); a Windows UART path would need a different
/// implementation behind the same [`UartTelemetry::read_samples`]
/// signature.
pub struct UartTelemetry {
    pub device_path: std::path::PathBuf,
    pub baud: u32,
}

impl UartTelemetry {
    #[cfg(unix)]
    pub fn read_samples(&self, n_bytes: usize, timeout: Duration) -> anyhow::Result<Vec<u8>> {
        let sh = Shell::new()?;
        let device = self.device_path.to_string_lossy().to_string();
        let baud = self.baud.to_string();
        // raw: disable line editing/echo/signal handling so every byte
        // the target sends comes through unmodified, which matters for
        // reading arbitrary binary sample bytes rather than line-
        // oriented text.
        cmd!(sh, "stty -f {device} {baud} raw -echo").run().or_else(|_| {
            // BSD/macOS `stty` uses `-f`; GNU/Linux `stty` uses `-F`.
            cmd!(sh, "stty -F {device} {baud} raw -echo").run()
        })?;

        let mut file = std::fs::File::open(&self.device_path)
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", self.device_path.display()))?;
        let mut buf = vec![0u8; n_bytes];
        let start = std::time::Instant::now();
        let mut filled = 0;
        while filled < n_bytes {
            if start.elapsed() > timeout {
                anyhow::bail!(
                    "read only {filled} of {n_bytes} bytes from {} before {timeout:?} elapsed",
                    self.device_path.display()
                );
            }
            match file.read(&mut buf[filled..]) {
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(buf)
    }

    #[cfg(not(unix))]
    pub fn read_samples(&self, _n_bytes: usize, _timeout: Duration) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("UartTelemetry is only implemented for Unix (stty-based) hosts")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_with_timeout_captures_stdout_on_success() {
        let out = run_with_timeout("echo", &["hello"], Duration::from_secs(5)).unwrap();
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn run_with_timeout_errors_on_nonzero_exit() {
        let result = run_with_timeout("false", &[], Duration::from_secs(5));
        assert!(result.is_err());
    }

    #[test]
    fn run_with_timeout_kills_a_hanging_process() {
        let start = std::time::Instant::now();
        let result = run_with_timeout("sleep", &["30"], Duration::from_millis(200));
        assert!(result.is_err());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "should have been killed well before sleep's own 30s"
        );
    }
}
