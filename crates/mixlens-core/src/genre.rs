//! Genre presets: typical tonal balance, dynamics and width for a style, and how a report
//! compares with them. Presets are plain data so users can edit or add their own later.

use crate::analyze::OCTAVE_BANDS;
use crate::report::{Kind, Report};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenrePreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub builtin: bool,
    /// Typical energy share per octave band (dB of total 20 Hz–20 kHz), aligned with `OCTAVE_BANDS`.
    pub octave_target_db: Vec<f64>,
    /// Allowed distance from the target per band before it's flagged (dB).
    pub octave_tolerance_db: Vec<f64>,
    /// Typical fitted tilt (dB/oct, same scale as `Spectrum::tilt_db_per_oct`).
    pub tilt_range: (f64, f64),
    /// Typical integrated loudness of finished masters (LUFS). Informational only.
    pub master_lufs_range: (f64, f64),
    pub master_plr_range: (f64, f64),
    pub lra_range: (f64, f64),
    /// Mixes below this peak-to-loudness ratio may be over-compressed (dB).
    pub mix_plr_min: f64,
    /// Correlation below 120 Hz should stay above this.
    pub low_corr_min: f64,
    /// Loudest-1% 5–10 kHz share above this deserves a listen (dB).
    pub sibilance_p99_max: f64,
}

/// Default allowed deviation per octave band: looser at the extremes, where genres and
/// individual songs vary most.
const TOLERANCE: [f64; 12] = [6.0, 3.5, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.5, 4.0, 5.0, 6.0];

struct Spec {
    id: &'static str,
    name: &'static str,
    tilt: f64,
    /// Per-octave adjustment on top of the tilt line (dB), bands as in `OCTAVE_BANDS`.
    bumps: [f64; 12],
    tilt_range: (f64, f64),
    master_lufs: (f64, f64),
    master_plr: (f64, f64),
    lra: (f64, f64),
    mix_plr_min: f64,
    low_corr_min: f64,
    sibilance_p99_max: f64,
}

/// Built-in presets. These are hand-set starting points from general mixing practice, not
/// measurements of a reference corpus: retune them here (or override them with user presets)
/// as real references show where they're off.
#[rustfmt::skip]
const SPECS: [Spec; 10] = [
    Spec { id: "pop", name: "Pop", tilt: -2.0,
        bumps: [-5.0, 0.0, 0.5, 0.0, -0.5, 0.0, 0.0, 0.5, 0.5, 0.5, 0.0, -2.0],
        tilt_range: (-2.8, -1.4), master_lufs: (-11.0, -8.0), master_plr: (7.0, 10.0), lra: (4.0, 8.0),
        mix_plr_min: 9.0, low_corr_min: 0.8, sibilance_p99_max: -6.0 },
    Spec { id: "hiphop", name: "Hip-hop / Trap", tilt: -2.8,
        bumps: [-2.0, 1.5, 0.5, -0.5, -1.0, 0.0, 0.0, 0.0, 0.5, 0.0, -0.5, -2.5],
        tilt_range: (-3.6, -2.2), master_lufs: (-10.0, -7.0), master_plr: (7.0, 10.0), lra: (3.0, 7.0),
        mix_plr_min: 9.0, low_corr_min: 0.9, sibilance_p99_max: -6.0 },
    Spec { id: "edm", name: "EDM / House", tilt: -2.6,
        bumps: [-3.0, 1.5, 0.5, -0.5, -1.0, -0.5, 0.0, 0.0, 0.5, 1.0, 0.5, -1.5],
        tilt_range: (-3.4, -2.0), master_lufs: (-9.0, -6.0), master_plr: (6.0, 9.0), lra: (3.0, 6.0),
        mix_plr_min: 8.0, low_corr_min: 0.9, sibilance_p99_max: -5.0 },
    Spec { id: "rock", name: "Rock", tilt: -1.8,
        bumps: [-7.0, -1.0, 0.5, 0.5, 0.0, 0.5, 0.5, 0.5, 0.0, -0.5, -1.0, -3.0],
        tilt_range: (-2.4, -1.2), master_lufs: (-10.0, -8.0), master_plr: (7.0, 10.0), lra: (3.0, 7.0),
        mix_plr_min: 9.0, low_corr_min: 0.8, sibilance_p99_max: -6.0 },
    Spec { id: "metal", name: "Metal", tilt: -1.6,
        bumps: [-8.0, -1.0, 0.5, 0.0, -0.5, 0.0, 0.5, 1.0, 0.5, 0.0, -1.0, -3.0],
        tilt_range: (-2.2, -1.0), master_lufs: (-9.0, -7.0), master_plr: (6.0, 9.0), lra: (2.0, 6.0),
        mix_plr_min: 8.0, low_corr_min: 0.8, sibilance_p99_max: -5.0 },
    Spec { id: "rnb", name: "R&B / Soul", tilt: -2.5,
        bumps: [-3.5, 1.0, 0.5, 0.0, -0.5, 0.0, 0.0, -0.5, 0.0, 0.5, 0.0, -2.0],
        tilt_range: (-3.2, -1.8), master_lufs: (-12.0, -9.0), master_plr: (8.0, 11.0), lra: (4.0, 8.0),
        mix_plr_min: 9.0, low_corr_min: 0.85, sibilance_p99_max: -6.0 },
    Spec { id: "acoustic", name: "Acoustic / Folk", tilt: -1.6,
        bumps: [-9.0, -2.0, 0.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, -1.0, -3.0],
        tilt_range: (-2.2, -1.0), master_lufs: (-16.0, -12.0), master_plr: (10.0, 15.0), lra: (5.0, 12.0),
        mix_plr_min: 11.0, low_corr_min: 0.7, sibilance_p99_max: -6.0 },
    Spec { id: "jazz", name: "Jazz", tilt: -1.8,
        bumps: [-8.0, -1.5, 0.5, 0.5, 0.5, 0.0, 0.0, -0.5, -0.5, -1.0, -2.0, -4.0],
        tilt_range: (-2.4, -1.2), master_lufs: (-18.0, -13.0), master_plr: (12.0, 18.0), lra: (6.0, 14.0),
        mix_plr_min: 12.0, low_corr_min: 0.6, sibilance_p99_max: -7.0 },
    Spec { id: "classical", name: "Classical / Orchestral", tilt: -2.0,
        bumps: [-8.0, -2.0, 0.0, 0.5, 0.5, 0.5, 0.0, -0.5, -1.0, -1.5, -2.5, -5.0],
        tilt_range: (-2.6, -1.4), master_lufs: (-23.0, -16.0), master_plr: (15.0, 22.0), lra: (8.0, 20.0),
        mix_plr_min: 14.0, low_corr_min: 0.5, sibilance_p99_max: -8.0 },
    Spec { id: "lofi", name: "Lo-fi", tilt: -3.0,
        bumps: [-5.0, 0.0, 0.5, 0.5, 0.0, 0.0, -0.5, -1.0, -1.5, -2.5, -4.0, -7.0],
        tilt_range: (-3.8, -2.4), master_lufs: (-14.0, -10.0), master_plr: (8.0, 12.0), lra: (3.0, 8.0),
        mix_plr_min: 9.0, low_corr_min: 0.8, sibilance_p99_max: -8.0 },
];

/// Octave-band shares (dB of total) for a spectrum whose energy per octave falls `tilt` dB
/// per octave (pink noise = 0), plus per-band adjustments. Integrates over each band's
/// log-width, since the top bands are narrower than an octave.
pub fn tilt_curve(tilt: f64, bumps: &[f64; 12]) -> Vec<f64> {
    let energy: Vec<f64> = OCTAVE_BANDS
        .iter()
        .zip(bumps)
        .map(|(&(lo, hi), &bump)| {
            let (x0, x1) = (lo.log2(), hi.log2());
            let steps = 32;
            let dx = (x1 - x0) / steps as f64;
            let e: f64 = (0..steps).map(|i| 10f64.powf(tilt * (x0 + (i as f64 + 0.5) * dx) / 10.0) * dx).sum();
            e * 10f64.powf(bump / 10.0)
        })
        .collect();
    let total: f64 = energy.iter().sum();
    energy.iter().map(|e| 10.0 * (e / total).log10()).collect()
}

pub fn builtin_presets() -> Vec<GenrePreset> {
    SPECS
        .iter()
        .map(|s| GenrePreset {
            id: s.id.into(),
            name: s.name.into(),
            builtin: true,
            octave_target_db: tilt_curve(s.tilt, &s.bumps),
            octave_tolerance_db: TOLERANCE.to_vec(),
            tilt_range: s.tilt_range,
            master_lufs_range: s.master_lufs,
            master_plr_range: s.master_plr,
            lra_range: s.lra,
            mix_plr_min: s.mix_plr_min,
            low_corr_min: s.low_corr_min,
            sibilance_p99_max: s.sibilance_p99_max,
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenreBand {
    pub lo: f64,
    pub hi: f64,
    pub share_db: f64,
    pub target_db: f64,
    /// share minus target (dB). Positive = more energy than typical.
    pub delta_db: f64,
    pub tolerance_db: f64,
}

impl GenreBand {
    pub fn within(&self) -> bool {
        self.delta_db.abs() <= self.tolerance_db
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenreCheck {
    pub metric: String,
    pub label: String,
    pub value: f64,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub unit: String,
    pub within: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenreMatch {
    pub preset_id: String,
    pub preset_name: String,
    pub bands: Vec<GenreBand>,
    pub checks: Vec<GenreCheck>,
}

fn check(metric: &str, label: &str, value: f64, lo: Option<f64>, hi: Option<f64>, unit: &str) -> GenreCheck {
    let within = lo.map_or(true, |l| value >= l) && hi.map_or(true, |h| value <= h);
    GenreCheck { metric: metric.into(), label: label.into(), value, lo, hi, unit: unit.into(), within }
}

pub fn evaluate(r: &Report, g: &GenrePreset, kind: Kind) -> GenreMatch {
    let bands = r
        .spectrum
        .octave_share
        .iter()
        .zip(&g.octave_target_db)
        .zip(&g.octave_tolerance_db)
        // Bands above the file's Nyquist frequency read as near-silent; don't compare them.
        .filter(|((o, _), _)| o.share_db > -100.0)
        .map(|((o, &target), &tol)| GenreBand { lo: o.lo, hi: o.hi, share_db: o.share_db, target_db: target, delta_db: o.share_db - target, tolerance_db: tol })
        .collect();
    let l = &r.loudness;
    let mut checks = vec![
        check("tilt", "Tilt", r.spectrum.tilt_db_per_oct, Some(g.tilt_range.0), Some(g.tilt_range.1), "dB/oct"),
        check("lra", "Loudness range", l.lra_lu, Some(g.lra_range.0), Some(g.lra_range.1), "LU"),
        check("low_correlation", "Correlation < 120 Hz", r.stereo.low_correlation, Some(g.low_corr_min), None, ""),
        check("sibilance_p99", "5–10 kHz, loudest 1%", r.spectrum.sibilance.p99_db, None, Some(g.sibilance_p99_max), "dB"),
    ];
    match kind {
        Kind::Mix => checks.push(check("plr", "PLR", l.plr_db, Some(g.mix_plr_min), None, "dB")),
        // References are finished masters too.
        Kind::Master | Kind::Reference => {
            checks.push(check("integrated", "Integrated", l.integrated_lufs, Some(g.master_lufs_range.0), Some(g.master_lufs_range.1), "LUFS"));
            checks.push(check("plr", "PLR", l.plr_db, Some(g.master_plr_range.0), Some(g.master_plr_range.1), "dB"));
        }
    }
    GenreMatch { preset_id: g.id.clone(), preset_name: g.name.clone(), bands, checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_normalized() {
        for p in builtin_presets() {
            assert_eq!(p.octave_target_db.len(), OCTAVE_BANDS.len());
            assert_eq!(p.octave_tolerance_db.len(), OCTAVE_BANDS.len());
            let total: f64 = p.octave_target_db.iter().map(|d| 10f64.powf(d / 10.0)).sum();
            assert!((total - 1.0).abs() < 1e-9, "{} sums to {total}", p.id);
        }
        // Pink noise: equal energy per octave, so a full octave band holds 10*log10(1/~9.97).
        let pink = tilt_curve(0.0, &[0.0; 12]);
        let octaves = (20000f64 / 20.0).log2();
        assert!((pink[1] - 10.0 * (1.0 / octaves).log10()).abs() < 1e-6);
    }

    #[test]
    fn ids_are_unique() {
        let ps = builtin_presets();
        let mut ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ps.len());
    }
}
