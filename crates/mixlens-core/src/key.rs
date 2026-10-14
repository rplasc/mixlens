//! Song key context: tells bass notes from resonances, estimates tuning, and lists
//! in-key frequencies. Works on the tonal peaks already stored in a report.

use crate::dsp;
use crate::report::{LfPeak, Report};
use serde::{Deserialize, Serialize};

const MAJOR_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const MINOR_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B"];
/// Peaks further than this from the song's tuning grid aren't on any note.
const BETWEEN_NOTES_CENTS: f64 = 35.0;
/// Below this the tuning estimate is too uncertain to apply to the safe frequencies.
const APPLY_TUNING_CENTS: f64 = 15.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MusicalKey {
    /// Pitch class of the tonic, 0 = C … 11 = B.
    pub tonic: u8,
    pub mode: Mode,
}

/// How a pitch relates to the song's key.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Root,
    Fifth,
    Third,
    ScaleTone,
    OutOfKey,
    /// Not close to any note of the song's tuning: a resonance or noise, not a pitch.
    BetweenNotes,
}

impl Role {
    pub fn musical(self) -> bool {
        !matches!(self, Role::OutOfKey | Role::BetweenNotes)
    }
    pub fn label(self) -> &'static str {
        match self {
            Role::Root => "root",
            Role::Fifth => "fifth",
            Role::Third => "third",
            Role::ScaleTone => "in key",
            Role::OutOfKey => "out of key",
            Role::BetweenNotes => "between notes",
        }
    }
}

impl MusicalKey {
    /// Parses "F#m", "F# minor", "Gb", "Bbmaj", "Am", "A minor".
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let mut chars = s.chars();
        let base: i32 = match chars.next()?.to_ascii_uppercase() {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let mut rest = chars.as_str();
        let mut acc = 0;
        if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
            acc = 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
            acc = -1;
            rest = r;
        }
        let rest = rest.trim();
        let mode = match rest {
            "" | "M" => Mode::Major,
            "m" => Mode::Minor,
            _ => match rest.to_lowercase().as_str() {
                "maj" | "major" => Mode::Major,
                "min" | "minor" => Mode::Minor,
                _ => return None,
            },
        };
        Some(MusicalKey { tonic: (base + acc).rem_euclid(12) as u8, mode })
    }

    pub fn name(&self) -> String {
        let t = (self.tonic % 12) as usize;
        match self.mode {
            Mode::Major => format!("{} major", MAJOR_NAMES[t]),
            Mode::Minor => format!("{} minor", MINOR_NAMES[t]),
        }
    }

    /// Role of a pitch class. Minor includes the raised 7th (harmonic minor's leading tone).
    pub fn role(&self, pitch_class: u8) -> Role {
        let i = (pitch_class as i32 - self.tonic as i32).rem_euclid(12);
        let (third, scale): (i32, &[i32]) = match self.mode {
            Mode::Major => (4, &[0, 2, 4, 5, 7, 9, 11]),
            Mode::Minor => (3, &[0, 2, 3, 5, 7, 8, 10, 11]),
        };
        match i {
            0 => Role::Root,
            7 => Role::Fifth,
            x if x == third => Role::Third,
            x if scale.contains(&x) => Role::ScaleTone,
            _ => Role::OutOfKey,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyedPeak {
    pub hz: f64,
    /// Nearest note on the song's tuning grid.
    pub note: String,
    /// Distance from that note (cents, after allowing for the song's tuning offset).
    pub cents: f64,
    pub above_median_db: f64,
    pub role: Role,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyFreq {
    pub note: String,
    pub hz: f64,
    pub role: Role,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyAnalysis {
    pub key: MusicalKey,
    pub key_name: String,
    /// How far the tonal peaks sit from A=440 tuning (cents). None when there are too few
    /// peaks or they disagree.
    pub tuning_offset_cents: Option<f64>,
    pub a4_hz: f64,
    /// 30–300 Hz peaks with their role in the key.
    pub peaks: Vec<KeyedPeak>,
    /// 200–800 Hz peaks with their role in the key.
    pub mid_peaks: Vec<KeyedPeak>,
    /// Root and fifth frequencies between 30 and 300 Hz: safe places to boost.
    pub safe_freqs: Vec<KeyFreq>,
}

fn midi(hz: f64) -> f64 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

fn midi_hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

/// Prominence-weighted circular mean of the peaks' offsets from the A=440 grid, so +48 and
/// −48 cents (nearly the same tuning) don't cancel out.
pub fn tuning_offset(peaks: &[&LfPeak]) -> Option<f64> {
    if peaks.len() < 2 {
        return None;
    }
    let (mut x, mut y, mut w_sum) = (0.0, 0.0, 0.0);
    for p in peaks {
        let m = midi(p.hz);
        let a = (m - m.round()) * std::f64::consts::TAU;
        let w = p.above_median_db.max(0.1);
        x += w * a.cos();
        y += w * a.sin();
        w_sum += w;
    }
    // Peaks that disagree (resultant length < 0.5) give no usable estimate.
    if (x * x + y * y).sqrt() / w_sum < 0.5 {
        return None;
    }
    Some(y.atan2(x) / std::f64::consts::TAU * 100.0)
}

fn keyed(p: &LfPeak, key: &MusicalKey, offset: f64) -> KeyedPeak {
    let m = midi(p.hz) - offset / 100.0;
    let nearest = m.round();
    let cents = (m - nearest) * 100.0;
    let role = if cents.abs() > BETWEEN_NOTES_CENTS { Role::BetweenNotes } else { key.role((nearest as i64).rem_euclid(12) as u8) };
    KeyedPeak { hz: p.hz, note: dsp::note_name(midi_hz(nearest)), cents, above_median_db: p.above_median_db, role }
}

pub fn analyze(r: &Report, key: &MusicalKey) -> KeyAnalysis {
    let sp = &r.spectrum;
    // The two ranges overlap at 200–300 Hz; count each peak once.
    let all: Vec<&LfPeak> = sp.lf_peaks.iter().chain(sp.mid_peaks.iter().filter(|p| p.hz >= 300.0)).collect();
    let tuning = tuning_offset(&all);
    let offset = tuning.unwrap_or(0.0);
    let grid = if offset.abs() > APPLY_TUNING_CENTS { offset } else { 0.0 };
    let safe_freqs = (24..=74)
        .filter_map(|m: i32| {
            let role = key.role(m.rem_euclid(12) as u8);
            let hz = midi_hz(m as f64 + grid / 100.0);
            (matches!(role, Role::Root | Role::Fifth) && (30.0..=300.0).contains(&hz))
                .then(|| KeyFreq { note: dsp::note_name(midi_hz(m as f64)), hz, role })
        })
        .collect();
    KeyAnalysis {
        key: *key,
        key_name: key.name(),
        tuning_offset_cents: tuning,
        a4_hz: 440.0 * 2f64.powf(offset / 1200.0),
        peaks: sp.lf_peaks.iter().map(|p| keyed(p, key, offset)).collect(),
        mid_peaks: sp.mid_peaks.iter().map(|p| keyed(p, key, offset)).collect(),
        safe_freqs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(hz: f64, db: f64) -> LfPeak {
        LfPeak { hz, above_median_db: db, note: dsp::note_name(hz) }
    }

    #[test]
    fn parses_keys() {
        let k = |s: &str| MusicalKey::parse(s);
        assert_eq!(k("F#m"), Some(MusicalKey { tonic: 6, mode: Mode::Minor }));
        assert_eq!(k("F# minor"), Some(MusicalKey { tonic: 6, mode: Mode::Minor }));
        assert_eq!(k("Gb"), Some(MusicalKey { tonic: 6, mode: Mode::Major }));
        assert_eq!(k("Bbmaj"), Some(MusicalKey { tonic: 10, mode: Mode::Major }));
        assert_eq!(k("am"), Some(MusicalKey { tonic: 9, mode: Mode::Minor }));
        assert_eq!(k("Cb"), Some(MusicalKey { tonic: 11, mode: Mode::Major }));
        assert_eq!(k("H"), None);
        assert_eq!(k("C dorian"), None);
        assert_eq!(k("Ab major").unwrap().name(), "Ab major");
    }

    #[test]
    fn roles_in_f_minor() {
        let k = MusicalKey::parse("F minor").unwrap();
        assert_eq!(k.role(5), Role::Root); // F
        assert_eq!(k.role(0), Role::Fifth); // C
        assert_eq!(k.role(8), Role::Third); // Ab
        assert_eq!(k.role(4), Role::ScaleTone); // E, raised 7th
        assert_eq!(k.role(10), Role::ScaleTone); // Bb
        assert_eq!(k.role(11), Role::OutOfKey); // B
    }

    #[test]
    fn estimates_tuning_offset() {
        let sharp = |m: f64| midi_hz(m + 0.3);
        let ps = [peak(sharp(29.0), 10.0), peak(sharp(36.0), 8.0), peak(sharp(41.0), 7.0)];
        let refs: Vec<&LfPeak> = ps.iter().collect();
        let off = tuning_offset(&refs).unwrap();
        assert!((off - 30.0).abs() < 0.5, "{off}");
        // Offsets near ±50 cents wrap instead of averaging to zero.
        let ps = [peak(midi_hz(36.48), 8.0), peak(midi_hz(43.52), 8.0)];
        let off = tuning_offset(&ps.iter().collect::<Vec<_>>()).unwrap();
        assert!(off.abs() > 45.0, "{off}");
        assert_eq!(tuning_offset(&[&ps[0]]), None);
    }

    #[test]
    fn labels_peaks_after_tuning() {
        let k = MusicalKey::parse("F minor").unwrap();
        // F1 sharp by 30 cents is still the root once the offset is removed.
        assert_eq!(keyed(&peak(midi_hz(29.3), 9.0), &k, 30.0).role, Role::Root);
        assert_eq!(keyed(&peak(midi_hz(35.0), 9.0), &k, 0.0).role, Role::OutOfKey); // B1
        assert_eq!(keyed(&peak(midi_hz(36.5), 9.0), &k, 0.0).role, Role::BetweenNotes);
    }
}
