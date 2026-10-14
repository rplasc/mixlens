//! MixLens analysis engine.
//!
//! `analyze_file` decodes once and runs every analyzer in parallel; the resulting
//! [`Report`] is small, serializable, and is what both the UI and the AI mode consume.

pub mod analyze;
pub mod audio;
pub mod codec;
pub mod compare;
pub mod dsp;
pub mod findings;
pub mod report;
pub mod tools;

pub use audio::{decode_file, Audio};
pub use compare::{compare, Comparison};
pub use report::{Finding, Kind, Report, Severity, Targets};

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(String),
    #[error("analysis: {0}")]
    Analysis(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Decode and analyze a file. Returns the decoded audio too, so callers can cache it
/// for comparisons and range tools.
pub fn analyze_file(path: &Path, targets: &Targets) -> Result<(Audio, Report)> {
    let audio = decode_file(path)?;
    let report = analyze::analyze(&audio, targets)?;
    Ok((audio, report))
}

/// Re-run only the rules (e.g. after switching mix/master) without re-analyzing.
pub fn refresh_findings(report: &mut Report, targets: &Targets) {
    report.findings = findings::evaluate(report, targets);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(fs: u32, hz: f32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(fs as f32 * secs) as usize).map(|i| amp * (2.0 * PI * hz * i as f32 / fs as f32).sin()).collect()
    }

    #[test]
    fn analyzes_sine() {
        let fs = 48000;
        let x = sine(fs, 1000.0, 10.0, 0.5);
        let a = audio::from_planar(fs, x.clone(), x, "sine");
        let r = analyze::analyze(&a, &Targets::default()).unwrap();
        // A 1 kHz sine at -6 dBFS in both channels reads about -3.0 LUFS... (-6 dBFS peak => -9 dBFS RMS => +3 dB for 2 ch, -0.691 offset, ~0 K-weight gain at 1k)
        assert!((r.loudness.integrated_lufs - (-6.02)).abs() < 0.3, "{}", r.loudness.integrated_lufs);
        assert!((r.loudness.sample_peak_dbfs - (-6.02)).abs() < 0.01);
        assert!((r.stereo.correlation - 1.0).abs() < 1e-6);
        // Range loudness from stored blocks should match the meter.
        let rl = analyze::range_loudness(&r.kblocks, 0.0, 10.0);
        assert!((rl - r.loudness.integrated_lufs).abs() < 0.1, "{rl}");
    }

    #[test]
    fn compare_detects_offset_and_gain() {
        let fs = 48000;
        let mut noise = vec![0f32; fs as usize * 20];
        let mut s = 12345u32;
        for v in noise.iter_mut() {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            *v = (s as f32 / u32::MAX as f32 - 0.5) * 0.4;
        }
        let a = audio::from_planar(fs, noise.clone(), noise.clone(), "a");
        let mut shifted = vec![0f32; 480];
        shifted.extend(noise.iter().map(|v| v * 0.5));
        shifted.truncate(noise.len());
        let b = audio::from_planar(fs, shifted.clone(), shifted, "b");
        let ra = analyze::analyze(&a, &Targets::default()).unwrap();
        let rb = analyze::analyze(&b, &Targets::default()).unwrap();
        let c = compare(&a, &ra, &b, &rb).unwrap();
        assert!((c.lag_ms - 10.0).abs() < 0.05, "lag {}", c.lag_ms);
        assert!((c.gain_db - (-6.02)).abs() < 0.1, "gain {}", c.gain_db);
        assert!(c.residual_rel_db < -40.0, "res {}", c.residual_rel_db);
    }
}
