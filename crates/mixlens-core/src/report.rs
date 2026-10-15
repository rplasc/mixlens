//! Serializable analysis report. Everything the UI and the AI see comes from here.

use crate::audio::FileInfo;
use crate::genre::{GenreMatch, GenrePreset};
use crate::key::{KeyAnalysis, MusicalKey};
use crate::streaming::Platform;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Whether a file is a pre-master mix, a finished master, or someone else's finished track
/// used as a reference. Changes which rules apply.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Mix,
    Master,
    Reference,
}

/// Rules context for findings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Targets {
    pub kind: Kind,
    /// Streaming loudness target, LUFS (Spotify default -14).
    pub target_lufs: f64,
    /// Max true peak for masters at or below the target (dBTP).
    pub ceiling_dbtp: f64,
    /// Max true peak for masters louder than the target (dBTP).
    pub loud_ceiling_dbtp: f64,
    /// The song's genre preset, if set: tonal target and genre-specific thresholds.
    #[serde(default)]
    pub genre: Option<GenrePreset>,
    /// The song's key, if set: labels tonal peaks and estimates tuning.
    #[serde(default)]
    pub key: Option<MusicalKey>,
    /// Main streaming platform, if one was chosen; `target_lufs` and the ceilings come from it.
    #[serde(default)]
    pub platform: Option<Platform>,
}

impl Default for Targets {
    fn default() -> Self {
        Self { kind: Kind::Mix, target_lufs: -14.0, ceiling_dbtp: -1.0, loud_ceiling_dbtp: -2.0, genre: None, key: None, platform: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema: u32,
    pub file: FileInfo,
    pub loudness: Loudness,
    pub stereo: Stereo,
    pub spectrum: Spectrum,
    pub edges: Edges,
    pub sections: Vec<Section>,
    pub dither: Option<Dither>,
    pub findings: Vec<Finding>,
    /// Comparison with the song's genre preset (derived; set by `refresh_findings`).
    #[serde(default)]
    pub genre_match: Option<GenreMatch>,
    /// Tonal peaks interpreted in the song's key (derived; set by `refresh_findings`).
    #[serde(default)]
    pub key_analysis: Option<KeyAnalysis>,
    /// Where clipping and true-peak overs are, for the current rules (derived; set by `refresh_findings`).
    #[serde(default)]
    pub peak_marks: Vec<PeakMark>,
    pub analysis_ms: u64,
    /// K-weighted mean-square energy per 100 ms sub-block (sum of channels).
    /// Lets section loudness be computed later without re-decoding.
    #[serde(default)]
    pub kblocks: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Loudness {
    pub integrated_lufs: f64,
    pub lra_lu: f64,
    pub true_peak_dbtp: f64,
    pub sample_peak_dbfs: f64,
    pub sample_peak_l_dbfs: f64,
    pub sample_peak_r_dbfs: f64,
    /// RMS of non-silent audio (dBFS).
    pub rms_dbfs: f64,
    /// Sample peak minus non-silent RMS (dB).
    pub crest_db: f64,
    /// Peak-to-loudness ratio: true peak minus integrated (dB).
    pub plr_db: f64,
    pub max_short_term_lufs: f64,
    pub max_momentary_lufs: f64,
    /// Samples at or above -0.1 dBFS.
    pub samples_near_full_scale: u64,
    /// Samples beyond full scale (only possible in float files).
    pub samples_over_full_scale: u64,
    /// Runs of 3+ consecutive samples pinned at the file's peak value (hard clipping/flat tops).
    pub flat_top_runs: u64,
    pub dc_offset: [f64; 2],
    pub lr_balance_db: f64,
    /// Short-term loudness every `short_term_hop_s` seconds (value at end of each 3 s window).
    pub short_term: Vec<f32>,
    pub short_term_hop_s: f64,
    /// Where the peaks are, per 100 ms block. `None` in reports analyzed before it was recorded.
    #[serde(default)]
    pub peak_map: Option<PeakMap>,
}

/// Peak positions in 100 ms blocks (`analyze::KBLOCK_S`), kept so markers can follow the rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeakMap {
    /// True peak per block (dBTP, louder channel).
    pub tp_blocks: Vec<f32>,
    /// Blocks holding samples above full scale (capped).
    pub over_fs_blocks: Vec<u32>,
    /// Blocks where a flat-top run of 3+ samples starts (capped).
    pub flat_top_blocks: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeakKind {
    OverFullScale,
    Clipping,
    TruePeak,
}

/// A stretch of the song with clipping or true peak over the ceiling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeakMark {
    pub start_s: f64,
    pub end_s: f64,
    pub kind: PeakKind,
    /// Highest true peak in the stretch (dBTP).
    pub peak_db: f64,
    pub severity: Severity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stereo {
    pub correlation: f64,
    pub side_mid_db: f64,
    /// Correlation below 120 Hz.
    pub low_correlation: f64,
    pub low_side_mid_db: f64,
    pub mono_fold_lufs: f64,
    /// Loudness change when folded to mono (negative = quieter).
    pub mono_loss_lu: f64,
    pub window_corr_min: f64,
    pub window_corr_p5: f64,
    pub window_corr_median: f64,
    pub negative_window_fraction: f64,
    /// 400 ms windows with correlation below 0.35 (capped list).
    pub low_corr_windows: Vec<CorrWindow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorrWindow {
    pub t: f64,
    pub corr: f64,
    pub level_dbfs: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spectrum {
    /// Fitted energy-per-octave slope (dB/oct). Pink noise = 0, typical mixes ≈ -1.5 to -3.
    pub tilt_db_per_oct: f64,
    pub third_octave: Vec<BandDev>,
    pub octave_share: Vec<OctaveShare>,
    pub lf_peaks: Vec<LfPeak>,
    /// Narrow peaks between 200 and 800 Hz, for telling chord tones from low-mid build-up.
    #[serde(default)]
    pub mid_peaks: Vec<LfPeak>,
    pub sub30_share_db: f64,
    pub sibilance: Sibilance,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BandDev {
    pub fc: f64,
    pub level_db: f64,
    /// Deviation from the fitted tilt line (dB). Positive = bump.
    pub dev_db: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OctaveShare {
    pub lo: f64,
    pub hi: f64,
    /// Band energy relative to total 20 Hz–20 kHz energy (dB).
    pub share_db: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LfPeak {
    pub hz: f64,
    /// Peak level above the median of the searched range (dB).
    pub above_median_db: f64,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sibilance {
    /// 5–10 kHz energy relative to full band in the louder half of the song (dB).
    pub median_db: f64,
    pub p99_db: f64,
    pub max_db: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edges {
    pub first_sound_s: f64,
    pub last_sound_s: f64,
    pub leading_silence_s: f64,
    pub trailing_silence_s: f64,
    pub tail_last50ms_dbfs: f64,
    pub ends_on_silence: bool,
    /// File ends while the signal is still audibly decaying.
    pub tail_truncated: bool,
    pub fade_out_detected: bool,
    /// Runs of digital silence (> 2 s) between first and last sound.
    pub internal_silences: Vec<TimeRange>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_s: f64,
    pub end_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    Silence,
    Quiet,
    Loud,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Section {
    pub start_s: f64,
    pub end_s: f64,
    pub kind: SectionKind,
    pub lufs: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dither {
    pub bits: u32,
    /// 0 when there was too little quiet audio to judge (the other fields are then empty too).
    pub samples_checked: u64,
    pub distinct_codes: u32,
    pub nonzero_fraction: f64,
    pub level_dbfs: f64,
    pub detected: bool,
    pub note: String,
}

impl Dither {
    /// A 16-bit (or lower) file whose quiet passages are pure digital zero.
    pub fn missing(&self) -> bool {
        self.bits <= 16 && self.samples_checked > 0 && !self.detected && self.nonzero_fraction < 0.01
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: String,
    pub title: String,
    pub detail: String,
}

impl Finding {
    pub fn new(severity: Severity, code: &str, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Finding { severity, code: code.into(), title: title.into(), detail: detail.into() }
    }
}

impl Report {
    /// Compact JSON for the AI: drops the bulky per-block arrays.
    pub fn to_ai_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("kblocks");
            if let Some(l) = o.get_mut("loudness").and_then(|l| l.as_object_mut()) {
                // Downsample short-term to 3 s resolution, rounded.
                if let Some(st) = l.remove("short_term") {
                    let st: Vec<f64> = serde_json::from_value(st).unwrap_or_default();
                    let step = (3.0 / self.loudness.short_term_hop_s).round().max(1.0) as usize;
                    let ds: Vec<f64> = st.iter().step_by(step).map(|x| (x * 10.0).round() / 10.0).collect();
                    l.insert("short_term_every_3s".into(), serde_json::json!(ds));
                }
                l.remove("short_term_hop_s");
                l.remove("peak_map");
            }
        }
        round_floats(&mut v);
        v
    }
}

/// Round floats to 2 decimals to save tokens.
pub fn round_floats(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if n.is_f64() {
                    let r = (f * 100.0).round() / 100.0;
                    if let Some(nn) = serde_json::Number::from_f64(r) {
                        *n = nn;
                    }
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(round_floats),
        serde_json::Value::Object(o) => o.values_mut().for_each(round_floats),
        _ => {}
    }
}
