//! Streaming platforms: how each will turn a track up or down, how loud it then plays next to
//! other tracks, and whether it meets their peak and delivery requirements.

use crate::codec::CodecResult;
use crate::report::{Finding, Kind, Report, Severity};
use serde::{Deserialize, Serialize};

/// Spotify's limiter (Loud mode) engages at -1 dB.
const LIMITER_DBTP: f64 = -1.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    /// Loud tracks are turned down and quiet ones up (boosts limited by peak headroom).
    UpAndDown,
    /// Loud tracks are turned down; quiet ones play as they are.
    DownOnly,
}

/// Where a platform's figures come from.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Published by the platform.
    Documented,
    /// Measured or reported by third parties; the platform doesn't publish it.
    Reported,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlatformMode {
    pub name: String,
    pub target_lufs: f64,
    /// A limiter catches peaks instead of the boost being capped.
    pub limiter: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Platform {
    pub id: String,
    pub name: String,
    pub target_lufs: f64,
    pub normalization: Normalization,
    /// Boosts stop once the true peak would reach this (dBTP).
    pub boost_ceiling_dbtp: Option<f64>,
    /// Recommended maximum true peak of the master (dBTP).
    pub ceiling_dbtp: f64,
    /// Stricter ceiling for masters louder than the target, if the platform asks for one.
    pub loud_ceiling_dbtp: Option<f64>,
    /// Listener-selectable alternatives to the default (e.g. Spotify Loud/Quiet).
    pub modes: Vec<PlatformMode>,
    /// Encode presets (`codec::PRESETS` ids) that stand in for this platform's streams.
    pub codecs: Vec<String>,
    /// Offers a hi-res lossless tier.
    pub hires: bool,
    pub confidence: Confidence,
    pub note: String,
}

impl Platform {
    /// Ceiling that applies to a master at `integrated` LUFS.
    pub fn ceiling_for(&self, integrated: f64) -> f64 {
        if integrated > self.target_lufs + 0.05 {
            self.loud_ceiling_dbtp.unwrap_or(self.ceiling_dbtp)
        } else {
            self.ceiling_dbtp
        }
    }

    pub fn playback(&self, r: &Report) -> Playback {
        playback(r.loudness.integrated_lufs, r.loudness.true_peak_dbtp, self.target_lufs, self.normalization, self.boost_ceiling_dbtp, false)
    }
}

/// Built-in platforms. Spotify's rules are from its "Loudness normalization" artist support
/// article (-14 LUFS; boosts stop at -1 dBTP; Loud -11 with a limiter at -1 dB; Quiet -19).
/// The others don't publish their rules: these are commonly reported figures from mastering
/// guides, which partly disagree. Retune them here as better data appears.
pub fn builtin_platforms() -> Vec<Platform> {
    let p = |id: &str, name: &str, target: f64, norm, boost: Option<f64>, ceiling: f64, codecs: &[&str], hires, confidence, note: &str| Platform {
        id: id.into(),
        name: name.into(),
        target_lufs: target,
        normalization: norm,
        boost_ceiling_dbtp: boost,
        ceiling_dbtp: ceiling,
        loud_ceiling_dbtp: None,
        modes: vec![],
        codecs: codecs.iter().map(|c| c.to_string()).collect(),
        hires,
        confidence,
        note: note.into(),
    };
    use Confidence::*;
    use Normalization::*;
    let mut spotify = p(
        "spotify",
        "Spotify",
        -14.0,
        UpAndDown,
        Some(-1.0),
        -1.0,
        &["ogg_q5", "ogg_q9"],
        false,
        Documented,
        "From Spotify's loudness normalization guide. Premium listeners can choose Loud or Quiet, or turn normalization off. Albums played in order are normalized as a whole.",
    );
    spotify.loud_ceiling_dbtp = Some(-2.0);
    spotify.modes = vec![
        PlatformMode { name: "Loud".into(), target_lufs: -11.0, limiter: true },
        PlatformMode { name: "Quiet".into(), target_lufs: -19.0, limiter: false },
    ];
    let amazon = p(
        "amazon",
        "Amazon Music",
        -14.0,
        DownOnly,
        None,
        -2.0,
        &["aac_256"],
        true,
        Reported,
        "Amazon doesn't publish its rules. Commonly reported: -14 LUFS, loud tracks turned down only, and a stricter -2 dBTP ceiling.",
    );
    vec![
        spotify,
        p(
            "apple",
            "Apple Music",
            -16.0,
            UpAndDown,
            Some(-1.0),
            -1.0,
            &["aac_256"],
            true,
            Reported,
            "Sound Check (a listener setting). Apple doesn't publish a target; -16 LUFS is the widely reported figure. Apple Digital Masters asks for 24-bit masters with true peak at or below -1 dBTP.",
        ),
        p(
            "youtube",
            "YouTube",
            -14.0,
            DownOnly,
            None,
            -1.0,
            &["opus_128", "aac_128"],
            false,
            Reported,
            "Reported: loud uploads are turned down to about -14 LUFS; quieter ones play as they are. 'Stats for nerds' shows the content loudness.",
        ),
        amazon,
        p(
            "tidal",
            "Tidal",
            -14.0,
            DownOnly,
            None,
            -1.0,
            &["aac_256"],
            true,
            Reported,
            "Reported: -14 LUFS, normalized per album, and quiet masters are not boosted.",
        ),
        p(
            "deezer",
            "Deezer",
            -15.0,
            DownOnly,
            None,
            -1.0,
            &["mp3_320"],
            false,
            Reported,
            "The least certain figure here: -15 LUFS is reported by few sources, and whether quiet tracks are boosted isn't clear.",
        ),
    ]
}

/// What a platform does to a track's volume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playback {
    pub target_lufs: f64,
    /// Gain needed to reach the target (dB).
    pub wanted_gain_db: f64,
    /// Gain actually applied (dB). Negative = turned down.
    pub gain_db: f64,
    pub playback_lufs: f64,
    /// How much quieter than the target it ends up (dB, 0 when it reaches it).
    pub quieter_by_db: f64,
    /// Peak reduction by a playback limiter (dB).
    pub limiter_db: f64,
    /// The boost was cut short by peak headroom.
    pub capped_by_peaks: bool,
}

pub fn playback(integrated: f64, true_peak: f64, target: f64, norm: Normalization, boost_ceiling: Option<f64>, limiter: bool) -> Playback {
    let wanted = target - integrated;
    let (gain, capped) = if wanted <= 0.0 {
        (wanted, false)
    } else {
        match norm {
            Normalization::DownOnly => (0.0, false),
            Normalization::UpAndDown if limiter => (wanted, false),
            Normalization::UpAndDown => {
                let room = boost_ceiling.map_or(wanted, |c| (c - true_peak).max(0.0));
                (wanted.min(room), room < wanted - 0.05)
            }
        }
    };
    let playback_lufs = integrated + gain;
    Playback {
        target_lufs: target,
        wanted_gain_db: wanted,
        gain_db: gain,
        playback_lufs,
        quieter_by_db: (target - playback_lufs).max(0.0),
        limiter_db: if limiter { (true_peak + gain - LIMITER_DBTP).max(0.0) } else { 0.0 },
        capped_by_peaks: capped,
    }
}

/// One plain sentence about the volume change.
pub fn describe(p: &Platform, pb: &Playback, true_peak: f64) -> String {
    let (name, g) = (&p.name, pb.gain_db);
    if pb.wanted_gain_db < -0.05 {
        format!("{name} turns it down {:.1} dB to {:.1} LUFS, level with other tracks.", -g, pb.playback_lufs)
    } else if pb.wanted_gain_db <= 0.05 {
        format!("Already at {name}'s {:.0} LUFS target: plays unchanged.", pb.target_lufs)
    } else if p.normalization == Normalization::DownOnly {
        format!(
            "{name} only turns loud tracks down, so this plays as it is, {:.1} dB quieter than tracks at {:.0} LUFS.",
            pb.quieter_by_db, pb.target_lufs
        )
    } else if pb.capped_by_peaks && g < 0.05 {
        format!(
            "{name} can't turn it up: the true peak is already {:.1} dBTP, so it plays {:.1} dB quieter than other tracks.",
            true_peak, pb.quieter_by_db
        )
    } else if pb.capped_by_peaks {
        format!(
            "{name} turns it up {:.1} dB, short of the {:.1} dB needed because peaks would pass {:.0} dBTP, so it plays {:.1} dB quieter than other tracks.",
            g,
            pb.wanted_gain_db,
            p.boost_ceiling_dbtp.unwrap_or(0.0),
            pb.quieter_by_db
        )
    } else {
        format!("{name} turns it up {:.1} dB to {:.0} LUFS, level with other tracks.", g, pb.target_lufs)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModePlayback {
    pub name: String,
    pub playback: Playback,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlatformResult {
    pub id: String,
    pub name: String,
    pub confidence: Confidence,
    pub note: String,
    pub normalization: Normalization,
    pub hires: bool,
    /// Ceiling that applies to this master (the stricter one when it's louder than the target).
    pub ceiling_dbtp: f64,
    pub playback: Playback,
    pub modes: Vec<ModePlayback>,
    /// Worst true peak among this platform's encodes, if the encode check has run.
    pub encoded_peak_dbtp: Option<f64>,
    /// Peak after encoding (or the master's, without an encode check) plus the applied gain.
    pub peak_after_dbtp: f64,
    pub summary: String,
    pub checks: Vec<Finding>,
    pub status: Severity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamingReport {
    pub kind: Kind,
    /// Post-encode peaks were available.
    pub encoded: bool,
    /// Clipping and delivery-format checks that apply everywhere.
    pub general: Vec<Finding>,
    pub platforms: Vec<PlatformResult>,
}

fn f(sev: Severity, code: &str, title: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding { severity: sev, code: code.into(), title: title.into(), detail: detail.into() }
}

fn general(r: &Report, kind: Kind, encoded: Option<&[CodecResult]>) -> Vec<Finding> {
    let l = &r.loudness;
    let mut out = vec![];
    match kind {
        Kind::Mix => out.push(f(Severity::Info, "mix", "This is a mix", "Mastering will change these numbers. Shown as if it were released as it is.")),
        Kind::Reference => out.push(f(Severity::Info, "reference", "Reference track", "Shown for how it plays on each platform.")),
        Kind::Master => {}
    }

    // ---- clipping ----
    let before = out.len();
    if l.samples_over_full_scale > 0 {
        out.push(f(
            Severity::Error,
            "over_full_scale",
            "Samples above 0 dBFS",
            format!("{} samples exceed full scale. They clip as soon as the file is converted to 16- or 24-bit.", l.samples_over_full_scale),
        ));
    }
    if l.flat_top_runs > 0 && l.sample_peak_dbfs > -0.5 {
        out.push(f(
            Severity::Error,
            "clipping",
            "Hard clipping",
            format!("{} runs of 3+ samples pinned at {:.2} dBFS: the waveform is already flat-topped.", l.flat_top_runs, l.sample_peak_dbfs),
        ));
    }
    if l.true_peak_dbtp > 0.0 {
        out.push(f(
            Severity::Warn,
            "intersample",
            "Peaks between samples go over 0 dBTP",
            format!("True peak is {:+.1} dBTP. These overs clip in playback converters and lossy encoders.", l.true_peak_dbtp),
        ));
    }
    if let Some(enc) = encoded {
        let over: Vec<String> = enc
            .iter()
            .filter_map(|c| c.true_peak_dbtp.filter(|&p| p > 0.0).map(|p| format!("{} {:+.1} dBTP", c.label, p)))
            .collect();
        if !over.is_empty() {
            out.push(f(
                Severity::Error,
                "encode_clipping",
                "Clips after lossy encoding",
                format!(
                    "{}. Platforms that turn the track down remove some of this; ones that play it as it is don't. Lower the limiter ceiling until encoded peaks stay below 0 dBTP.",
                    over.join(", ")
                ),
            ));
        }
    }
    if out.len() == before {
        if l.samples_near_full_scale > 0 {
            out.push(f(
                Severity::Info,
                "near_full_scale",
                "Peaks touch full scale",
                format!("{} samples at or above -0.1 dBFS. Not clipping, but there's no margin left for encoding.", l.samples_near_full_scale),
            ));
        } else {
            out.push(f(
                Severity::Ok,
                "no_clipping",
                "No clipping",
                if encoded.is_some() { "Nothing goes over full scale, before or after encoding." } else { "Nothing goes over full scale. Run the encode check to test lossy streams too." },
            ));
        }
    }

    // ---- delivery format ----
    let ext = r.file.file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    let lossy = ["mp3", "m4a", "aac", "ogg", "opus"].contains(&ext.as_str());
    let float = r.file.sample_format.starts_with("float");
    let sr = r.file.sample_rate;
    let khz = |sr: u32| format!("{} kHz", sr as f64 / 1000.0);
    let before = out.len();
    if lossy {
        out.push(f(
            Severity::Warn,
            "lossy_source",
            "Lossy source file",
            "Upload a WAV or FLAC master. Distributors and platforms re-encode it, adding a second generation of lossy artifacts.",
        ));
    }
    if sr < 44100 {
        out.push(f(Severity::Warn, "sample_rate", "Sample rate below 44.1 kHz", format!("{} is below what distributors accept; export at 44.1 kHz or higher.", khz(sr))));
    }
    if float && !lossy {
        out.push(f(Severity::Info, "float", "Floating-point file", "Most distributors want 16- or 24-bit integer WAV or FLAC. Export a 24-bit copy."));
    }
    if let Some(d) = &r.dither {
        if d.bits <= 16 && d.samples_checked > 0 && !d.detected && d.nonzero_fraction < 0.01 {
            out.push(f(Severity::Info, "dither", "No dither visible", d.note.clone()));
        }
    }
    let bits = r.file.bits_per_sample.unwrap_or(0);
    if !lossy && !float && bits >= 24 && sr > 44100 {
        out.push(f(
            Severity::Info,
            "hires",
            "Eligible for hi-res tiers",
            format!("{bits}-bit at {} can stream in hi-res lossless on Apple Music, Amazon Music and Tidal.", khz(sr)),
        ));
    }
    if out.len() == before || (out.len() == before + 1 && out[before].code == "hires") {
        out.insert(before, f(Severity::Ok, "format", "Delivery format", format!("{bits}-bit at {} meets distributor requirements.", khz(sr))));
    }
    out
}

pub fn assess(r: &Report, kind: Kind, platforms: &[Platform], encoded: Option<&[CodecResult]>) -> StreamingReport {
    let l = &r.loudness;
    let tp = l.true_peak_dbtp;
    let results = platforms
        .iter()
        .map(|p| {
            let pb = p.playback(r);
            let ceiling = p.ceiling_for(l.integrated_lufs);
            let summary = describe(p, &pb, tp);
            let mut checks = vec![];

            if tp > ceiling + 0.05 {
                checks.push(f(
                    Severity::Warn,
                    "tp_ceiling",
                    format!("True peak above {}'s ceiling", p.name),
                    format!(
                        "{:.1} dBTP vs the recommended {:.0} dBTP{}.",
                        tp,
                        ceiling,
                        if ceiling < p.ceiling_dbtp { " (stricter because the master is louder than the target)" } else { "" }
                    ),
                ));
            } else {
                checks.push(f(Severity::Ok, "tp_ceiling", "True peak within the ceiling", format!("{:.1} dBTP vs {:.0} dBTP.", tp, ceiling)));
            }

            // Peak listeners get: worst encode of this platform's streams, plus the applied gain.
            let encoded_peak = encoded.and_then(|e| {
                e.iter()
                    .filter(|c| p.codecs.contains(&c.codec))
                    .filter_map(|c| c.true_peak_dbtp.map(|v| (v, c.label.clone())))
                    .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            });
            let (src, label) = match &encoded_peak {
                Some((v, label)) => (*v, format!("encoding to {label}")),
                None => (tp, "before encoding".to_string()),
            };
            let after = src + pb.gain_db;
            let gain_note = if pb.gain_db.abs() >= 0.05 { format!(" and {:+.1} dB of gain", pb.gain_db) } else { String::new() };
            if after > 0.0 {
                checks.push(f(
                    Severity::Error,
                    "playback_clip",
                    format!("Clips on {}", p.name),
                    format!("Peaks reach {:+.1} dBTP after {label}{gain_note}: listeners hear clipping.", after),
                ));
            } else if after > -0.5 {
                checks.push(f(
                    Severity::Warn,
                    "playback_peak",
                    format!("Little peak margin on {}", p.name),
                    format!("Peaks reach {:.1} dBTP after {label}{gain_note}.", after),
                ));
            } else if encoded_peak.is_some() {
                checks.push(f(
                    Severity::Ok,
                    "playback_peak",
                    "Encoded peaks stay clear",
                    format!("{:.1} dBTP after {label}{gain_note}.", after),
                ));
            }

            let modes: Vec<ModePlayback> = p
                .modes
                .iter()
                .map(|m| ModePlayback {
                    name: m.name.clone(),
                    playback: playback(l.integrated_lufs, tp, m.target_lufs, p.normalization, p.boost_ceiling_dbtp, m.limiter),
                })
                .collect();
            for m in &modes {
                if m.playback.limiter_db > 0.5 {
                    checks.push(f(
                        Severity::Info,
                        "mode_limiter",
                        format!("{} {} mode limits peaks", p.name, m.name),
                        format!(
                            "At {:.0} LUFS it's raised {:.1} dB and the limiter compresses peaks by up to {:.1} dB.",
                            m.playback.target_lufs, m.playback.gain_db, m.playback.limiter_db
                        ),
                    ));
                }
            }

            let status = checks.iter().map(|c| c.severity).max().unwrap_or(Severity::Ok);
            PlatformResult {
                id: p.id.clone(),
                name: p.name.clone(),
                confidence: p.confidence,
                note: p.note.clone(),
                normalization: p.normalization,
                hires: p.hires,
                ceiling_dbtp: ceiling,
                playback: pb,
                modes,
                encoded_peak_dbtp: encoded_peak.map(|e| e.0),
                peak_after_dbtp: after,
                summary,
                checks,
                status,
            }
        })
        .collect();
    StreamingReport { kind, encoded: encoded.is_some(), general: general(r, kind, encoded), platforms: results }
}

impl StreamingReport {
    pub fn to_ai_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        crate::report::round_floats(&mut v);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Normalization::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn spotify_example() {
        // Spotify's own example: -20 LUFS with a -5 dBTP peak is only lifted to -16 LUFS.
        let pb = playback(-20.0, -5.0, -14.0, UpAndDown, Some(-1.0), false);
        assert!(close(pb.gain_db, 4.0) && close(pb.playback_lufs, -16.0) && close(pb.quieter_by_db, 2.0));
        assert!(pb.capped_by_peaks);
        // Plenty of headroom: reaches the target.
        let pb = playback(-16.0, -6.0, -14.0, UpAndDown, Some(-1.0), false);
        assert!(close(pb.gain_db, 2.0) && close(pb.quieter_by_db, 0.0) && !pb.capped_by_peaks);
    }

    #[test]
    fn turn_down_only() {
        let pb = playback(-10.0, -1.0, -14.0, DownOnly, None, false);
        assert!(close(pb.gain_db, -4.0) && close(pb.playback_lufs, -14.0));
        let pb = playback(-18.0, -3.0, -14.0, DownOnly, None, false);
        assert!(close(pb.gain_db, 0.0) && close(pb.quieter_by_db, 4.0));
    }

    #[test]
    fn loud_mode_limits() {
        let pb = playback(-14.0, -1.0, -11.0, UpAndDown, Some(-1.0), true);
        assert!(close(pb.gain_db, 3.0) && close(pb.limiter_db, 3.0));
    }

    fn report() -> Report {
        let fs = 48000;
        let x: Vec<f32> = (0..fs as usize * 10).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin()).collect();
        let a = crate::audio::from_planar(fs, x.clone(), x, "song_master.wav");
        crate::analyze::analyze(&a, &crate::Targets::default()).unwrap()
    }

    fn enc(codec: &str, tp: f64) -> CodecResult {
        CodecResult { codec: codec.into(), label: codec.to_uppercase(), true_peak_dbtp: Some(tp), integrated_lufs: None, error: None }
    }

    #[test]
    fn clipping_checks() {
        let mut r = report();
        let s = assess(&r, Kind::Master, &builtin_platforms(), None);
        assert!(s.general.iter().any(|f| f.code == "no_clipping"));
        r.loudness.samples_over_full_scale = 10;
        let encoded = [enc("aac_128", 0.4), enc("ogg_q9", -1.2)];
        let s = assess(&r, Kind::Master, &builtin_platforms(), Some(&encoded));
        assert!(s.general.iter().any(|f| f.code == "over_full_scale" && f.severity == Severity::Error));
        let e = s.general.iter().find(|f| f.code == "encode_clipping").unwrap();
        assert!(e.detail.contains("AAC_128") && !e.detail.contains("OGG_Q9"));
    }

    #[test]
    fn encoded_peaks_follow_the_gain() {
        let mut r = report();
        (r.loudness.integrated_lufs, r.loudness.true_peak_dbtp) = (-10.0, -0.5);
        let encoded = [enc("ogg_q5", 0.3), enc("ogg_q9", 0.4), enc("opus_128", 0.6)];
        let s = assess(&r, Kind::Master, &builtin_platforms(), Some(&encoded));
        let spotify = s.platforms.iter().find(|p| p.id == "spotify").unwrap();
        // Turned down 4 dB, so the worst Ogg peak (+0.4) ends at -3.6 dBTP.
        assert!(close(spotify.playback.gain_db, -4.0));
        assert_eq!(spotify.encoded_peak_dbtp, Some(0.4));
        assert!((spotify.peak_after_dbtp - (-3.6)).abs() < 1e-9);
        // Louder than the target: Spotify's stricter -2 dBTP ceiling applies.
        assert_eq!(spotify.ceiling_dbtp, -2.0);
        assert!(spotify.checks.iter().any(|c| c.code == "tp_ceiling" && c.severity == Severity::Warn));
        assert_eq!(spotify.modes.len(), 2);
    }

    #[test]
    fn platform_wording_in_findings() {
        let mut r = report();
        (r.loudness.integrated_lufs, r.loudness.true_peak_dbtp) = (-18.0, -3.0);
        let youtube = builtin_platforms().into_iter().find(|p| p.id == "youtube").unwrap();
        let t = crate::Targets { kind: Kind::Master, target_lufs: -14.0, platform: Some(youtube), ..Default::default() };
        crate::refresh_findings(&mut r, &t);
        let low = r.findings.iter().find(|f| f.code == "loudness_low").unwrap();
        assert!(low.detail.contains("doesn't turn quiet tracks up") && low.detail.contains("4.0 dB quieter"), "{}", low.detail);
        // Without a platform the wording is the generic one.
        crate::refresh_findings(&mut r, &crate::Targets { kind: Kind::Master, ..Default::default() });
        assert!(r.findings.iter().find(|f| f.code == "loudness_low").unwrap().detail.starts_with("-18.0 LUFS vs -14 LUFS. Platforms"));
    }

    #[test]
    fn platform_data_is_consistent() {
        let ps = builtin_platforms();
        let mut ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ps.len());
        let presets: Vec<&str> = crate::codec::PRESETS.iter().map(|p| p.0).collect();
        for p in &ps {
            assert!(!p.codecs.is_empty(), "{}", p.id);
            assert!(p.codecs.iter().all(|c| presets.contains(&c.as_str())), "{} maps an unknown codec", p.id);
        }
    }
}
