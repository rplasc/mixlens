//! On-demand measurements over a time range (used by the UI and by AI tool calls).

use crate::analyze::{octave_levels, octave_shares, range_loudness, third_octave};
use crate::audio::Audio;
use crate::dsp;
use crate::report::{BandDev, OctaveShare, Report};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionLoudness {
    pub start_s: f64,
    pub end_s: f64,
    pub lufs: f64,
    /// Loudness of this range minus the whole song's integrated loudness.
    pub vs_integrated_lu: f64,
}

/// Uses stored K-weighted blocks; no audio needed.
pub fn section_loudness(r: &Report, t0: f64, t1: f64) -> SectionLoudness {
    let lufs = range_loudness(&r.kblocks, t0, t1);
    SectionLoudness { start_s: t0, end_s: t1, lufs, vs_integrated_lu: lufs - r.loudness.integrated_lufs }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BandEnergy {
    pub start_s: f64,
    pub end_s: f64,
    /// Absolute octave band energy (dB, arbitrary but consistent reference) for comparing versions.
    pub octave_level_db: Vec<f64>,
    pub octave_share: Vec<OctaveShare>,
    pub tilt_db_per_oct: f64,
    pub third_octave: Vec<BandDev>,
}

pub fn band_energy(a: &Audio, t0: f64, t1: f64) -> BandEnergy {
    let fs = a.sample_rate as f64;
    let mid = a.mid();
    let s = a.seconds_to_index(t0);
    let e = a.seconds_to_index(t1).max(s);
    let x = &mid[s..e];
    let (f, p) = dsp::welch(x, fs, 16384);
    let (tilt, third) = third_octave(&f, &p, fs);
    BandEnergy { start_s: t0, end_s: t1, octave_level_db: octave_levels(&f, &p), octave_share: octave_shares(&f, &p), tilt_db_per_oct: tilt, third_octave: third }
}
