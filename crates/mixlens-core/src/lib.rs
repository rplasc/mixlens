//! MixLens analysis engine.
//!
//! `analyze_file` decodes once and runs every analyzer in parallel; the resulting
//! [`Report`] is small, serializable, and is what both the UI and the AI mode consume.

pub mod analyze;
pub mod audio;
pub mod codec;
pub mod compare;
pub mod dsp;
pub mod export;
pub mod findings;
pub mod genre;
pub mod key;
pub mod report;
pub mod streaming;
pub mod tools;

pub use audio::{decode_file, Audio};
pub use compare::{compare, reference_compare, Comparison, ReferenceComparison};
pub use export::{ExportContext, ReportDoc};
pub use genre::{builtin_presets, GenrePreset};
pub use key::MusicalKey;
pub use streaming::{builtin_platforms, Platform, StreamingReport};
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
    #[error("export: {0}")]
    Export(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Decode and analyze a file. Returns the decoded audio too, so callers can cache it
/// for comparisons and range tools.
pub fn analyze_file(path: &Path, targets: &Targets) -> Result<(Audio, Report)> {
    let audio = decode_file(path)?;
    let report = analyze::analyze(&audio, targets)?;
    Ok((audio, report))
}

/// Apply the song context (genre, key) and re-run the rules without re-analyzing, e.g. after
/// switching mix/master or changing the song's genre or key.
pub fn refresh_findings(report: &mut Report, targets: &Targets) {
    report.genre_match = targets.genre.as_ref().map(|g| genre::evaluate(report, g, targets.kind));
    report.key_analysis = targets.key.as_ref().map(|k| key::analyze(report, k));
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
    fn locates_bass_peak_between_bins() {
        let fs = 48000;
        let x = sine(fs, 55.3, 10.0, 0.5);
        let a = audio::from_planar(fs, x.clone(), x, "sine");
        let r = analyze::analyze(&a, &Targets::default()).unwrap();
        let p = r.spectrum.lf_peaks.iter().max_by(|a, b| a.above_median_db.partial_cmp(&b.above_median_db).unwrap()).unwrap();
        assert!((p.hz - 55.3).abs() < 0.2, "{}", p.hz);
    }

    #[test]
    fn context_changes_rules() {
        let fs = 48000;
        let x = sine(fs, 1000.0, 10.0, 0.5);
        let a = audio::from_planar(fs, x.clone(), x, "sine");
        let mut r = analyze::analyze(&a, &Targets::default()).unwrap();
        assert!(r.genre_match.is_none() && r.key_analysis.is_none());

        // References only get the reference marker.
        refresh_findings(&mut r, &Targets { kind: Kind::Reference, ..Targets::default() });
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].code, "reference");

        // A lone 1 kHz sine is nothing like a pop balance.
        let pop = builtin_presets().into_iter().find(|p| p.id == "pop").unwrap();
        let t = Targets { genre: Some(pop), key: MusicalKey::parse("A minor"), ..Targets::default() };
        refresh_findings(&mut r, &t);
        let g = r.genre_match.as_ref().unwrap();
        assert!(g.bands.iter().any(|b| !b.within()));
        let tone = r.findings.iter().find(|f| f.code == "genre_tone").unwrap();
        assert_eq!(tone.severity, Severity::Warn);
        assert_eq!(r.key_analysis.as_ref().unwrap().key_name, "A minor");

        // Clearing the context restores the neutral result.
        refresh_findings(&mut r, &Targets::default());
        assert!(r.genre_match.is_none() && !r.findings.iter().any(|f| f.code.starts_with("genre")));
    }

    #[test]
    fn low_mid_buildup_on_root_is_softened() {
        let fs = 48000;
        let x = sine(fs, 1000.0, 10.0, 0.5);
        let a = audio::from_planar(fs, x.clone(), x, "sine");
        let mut r = analyze::analyze(&a, &Targets::default()).unwrap();
        for b in r.spectrum.third_octave.iter_mut() {
            // Centres are exact third octaves, so "315 Hz" is 317.5.
            b.dev_db = if (b.fc / 315.0).log2().abs() < 0.1 { 6.0 } else { 0.0 };
        }
        // F4 sits inside the 315 Hz third octave.
        r.spectrum.mid_peaks = vec![report::LfPeak { hz: 349.2, above_median_db: 9.0, note: "F4".into() }];
        let sev = |r: &Report| r.findings.iter().find(|f| f.code == "low_mid_buildup").map(|f| f.severity);
        refresh_findings(&mut r, &Targets::default());
        assert_eq!(sev(&r), Some(Severity::Warn));
        refresh_findings(&mut r, &Targets { key: MusicalKey::parse("F minor"), ..Targets::default() });
        assert_eq!(sev(&r), Some(Severity::Info));
        refresh_findings(&mut r, &Targets { key: MusicalKey::parse("E major"), ..Targets::default() });
        assert_eq!(sev(&r), Some(Severity::Warn), "F is out of E major");
    }

    #[test]
    fn reference_compare_against_itself() {
        let fs = 48000;
        let x = sine(fs, 440.0, 10.0, 0.5);
        let a = audio::from_planar(fs, x.clone(), x, "sine");
        let r = analyze::analyze(&a, &Targets::default()).unwrap();
        let c = reference_compare(&r, &r);
        assert!(c.level_match_db.abs() < 1e-9);
        assert!(c.metric_deltas.iter().all(|m| m.delta.abs() < 1e-9));
        assert!(c.octave.iter().all(|o| o.delta_db.abs() < 1e-9));
        assert!(c.third_octave_delta.iter().all(|b| b.dev_db.abs() < 1e-9));
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
