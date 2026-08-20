//! Raw sample generation: pull byte streams out of a [`QppRngSource`] and
//! write them to disk, one file per implementation/target, so Tier 2's
//! external tools (which only speak "path to a binary file") have
//! something to consume.
//!
//! NIST SP 800-90B's own guidance for a non-IID/IID entropy-source
//! evaluation asks for at least one million samples (one million *bytes*
//! here, since this source's native output unit is a byte -- see
//! `qpp-rng-reference`'s "Random number extraction" note). [`MIN_SAMPLE_BYTES`]
//! is that floor; callers can ask for more but [`generate_sample`] refuses
//! to silently produce less.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use rng_core::QppRngSource;

/// SP 800-90B's minimum recommended sample size for an entropy source
/// evaluation: one million (byte) samples.
pub const MIN_SAMPLE_BYTES: usize = 1_000_000;

/// How many bytes are pulled from the source per `fill_bytes` call while
/// streaming to disk. Large enough to amortize the write() syscall,
/// small enough not to hold a multi-hundred-MB buffer in memory for
/// oversized requests.
const CHUNK_BYTES: usize = 64 * 1024;

/// One written sample file plus the metadata a report needs about it.
#[derive(Debug, Clone)]
pub struct SampleFile {
    pub candidate: &'static str,
    pub path: PathBuf,
    pub len_bytes: usize,
}

/// Streams `n_bytes` of raw output from `rng` into `out`, in fixed-size
/// chunks so this scales to SP 800-90B-sized (or larger) samples without
/// an `n_bytes`-sized allocation.
///
/// Refuses to write fewer than [`MIN_SAMPLE_BYTES`] -- a sample smaller
/// than that isn't just "a smaller sample", it's not a sample Tier 2's
/// tools are validated for interpreting.
pub fn generate_sample<R: QppRngSource + ?Sized>(
    rng: &mut R,
    out: &mut impl Write,
    n_bytes: usize,
) -> io::Result<()> {
    assert!(
        n_bytes >= MIN_SAMPLE_BYTES,
        "sample of {n_bytes} bytes is below SP 800-90B's {MIN_SAMPLE_BYTES}-byte floor"
    );

    let mut chunk = [0u8; CHUNK_BYTES];
    let mut remaining = n_bytes;
    while remaining > 0 {
        let take = remaining.min(CHUNK_BYTES);
        rng.fill_bytes(&mut chunk[..take]);
        out.write_all(&chunk[..take])?;
        remaining -= take;
    }
    Ok(())
}

/// Generates one sample file per registered [`candidates::Candidate`],
/// named `{dir}/{candidate.name}.bin`, and returns the manifest Tier 2
/// and the report ingestion layer consume.
///
/// `dir` is created if it doesn't exist. `seed` is the same 128-bit seed
/// handed to every candidate -- fine to share, since the entropy each
/// candidate claims comes from timing jitter folded in during
/// generation, not from the seed (see `qpp-rng-reference`'s "Seed
/// evolution" fidelity note).
pub fn generate_all_candidate_samples(
    dir: &Path,
    n_bytes: usize,
    seed: u128,
) -> anyhow::Result<Vec<SampleFile>> {
    std::fs::create_dir_all(dir)?;

    let mut files = Vec::new();
    for candidate in candidates::all_candidates() {
        let mut rng = (candidate.make)(seed);
        let path = dir.join(format!("{}.bin", candidate.name));
        let mut writer = BufWriter::new(File::create(&path)?);
        generate_sample(rng.as_mut(), &mut writer, n_bytes)?;
        writer.flush()?;
        files.push(SampleFile {
            candidate: candidate.name,
            path,
            len_bytes: n_bytes,
        });
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingRng {
        counter: u8,
    }

    impl rand_core::TryRng for CountingRng {
        type Error = core::convert::Infallible;
        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            let mut buf = [0u8; 4];
            self.try_fill_bytes(&mut buf)?;
            Ok(u32::from_le_bytes(buf))
        }
        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            let mut buf = [0u8; 8];
            self.try_fill_bytes(&mut buf)?;
            Ok(u64::from_le_bytes(buf))
        }
        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            for b in dst.iter_mut() {
                *b = self.counter;
                self.counter = self.counter.wrapping_add(1);
            }
            Ok(())
        }
    }
    // `Rng` is blanket-implemented by `rand_core` for every
    // `TryRng<Error = Infallible>`, so `CountingRng` gets `fill_bytes`
    // etc. for free from the `TryRng` impl above -- implementing `Rng`
    // directly here would conflict with that blanket impl.
    impl QppRngSource for CountingRng {
        fn diagnostics(&self) -> rng_core::RngDiagnostics {
            rng_core::RngDiagnostics {
                permutation_size_bits: 0,
                last_permutation_count: 0,
                last_jitter_ns: None,
            }
        }
    }

    #[test]
    fn generate_sample_writes_exactly_n_bytes() {
        let mut rng = CountingRng { counter: 0 };
        let mut out = Vec::new();
        generate_sample(&mut rng, &mut out, MIN_SAMPLE_BYTES).unwrap();
        assert_eq!(out.len(), MIN_SAMPLE_BYTES);
    }

    #[test]
    fn generate_sample_spans_multiple_chunks_without_gaps() {
        // CountingRng emits a wrapping 0..=255 counter, so a
        // multi-chunk sample should show that same wraparound pattern
        // uninterrupted across the chunk boundary.
        let mut rng = CountingRng { counter: 0 };
        let mut out = Vec::new();
        let n = MIN_SAMPLE_BYTES + CHUNK_BYTES + 3;
        generate_sample(&mut rng, &mut out, n).unwrap();
        assert_eq!(out.len(), n);
        for w in out.windows(2) {
            assert_eq!(w[1], w[0].wrapping_add(1));
        }
    }

    #[test]
    #[should_panic(expected = "below SP 800-90B")]
    fn generate_sample_rejects_undersized_requests() {
        let mut rng = CountingRng { counter: 0 };
        let mut out = Vec::new();
        let _ = generate_sample(&mut rng, &mut out, 10);
    }

    #[test]
    #[ignore = "slow: drives real jitter-timed candidates at ~10-15 KB/s to \
                write two genuine 1MB SP 800-90B-sized samples (~2-3 minutes); \
                run explicitly with `cargo test -- --ignored`"]
    fn generate_all_candidate_samples_writes_one_file_per_candidate() {
        let dir = std::env::temp_dir().join(format!("qpp-rng-stats-test-{}", std::process::id()));
        let files =
            generate_all_candidate_samples(&dir, MIN_SAMPLE_BYTES, 0x1234_5678).unwrap();
        assert_eq!(files.len(), candidates::all_candidates().len());
        for f in &files {
            assert!(f.path.exists());
            assert_eq!(std::fs::metadata(&f.path).unwrap().len() as usize, f.len_bytes);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
