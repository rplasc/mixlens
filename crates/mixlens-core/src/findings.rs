//! Deterministic rules that turn measurements into findings. Fast, offline, and the
//! AI mode builds on them rather than replacing them.

use crate::key::Role;
use crate::streaming::Normalization;
use crate::report::*;

fn f(sev: Severity, code: &str, title: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding { severity: sev, code: code.into(), title: title.into(), detail: detail.into() }
}

/// Rule limits that a genre preset can change. Defaults are the genre-neutral values.
struct Thresholds {
    mix_plr_min: f64,
    low_corr_min: f64,
    sibilance_p99_max: f64,
}

impl Thresholds {
    fn new(t: &Targets) -> Self {
        match &t.genre {
            Some(g) => Self { mix_plr_min: g.mix_plr_min, low_corr_min: g.low_corr_min, sibilance_p99_max: g.sibilance_p99_max },
            None => Self { mix_plr_min: 9.0, low_corr_min: 0.8, sibilance_p99_max: -6.0 },
        }
    }
}

/// "20–40 Hz", "500 Hz–1 kHz", "16–20 kHz".
fn band_label(lo: f64, hi: f64) -> String {
    let k = |x: f64| ((x / 100.0).round() / 10.0).to_string();
    match (lo >= 1000.0, hi >= 1000.0) {
        (true, _) => format!("{}–{} kHz", k(lo), k(hi)),
        (false, true) => format!("{lo:.0} Hz–{} kHz", k(hi)),
        _ => format!("{lo:.0}–{hi:.0} Hz"),
    }
}

/// Seconds as a DAW-style position, "1:03.2".
fn clock(s: f64) -> String {
    let m = (s / 60.0).floor();
    format!("{}:{:04.1}", m, s - m * 60.0)
}

/// The true-peak limit the rules hold a version to: headroom for a mix, the streaming ceiling
/// for a master (stricter when it's louder than the target). References have none.
pub fn tp_ceiling(l: &Loudness, t: &Targets) -> Option<f64> {
    match t.kind {
        Kind::Mix => Some(-1.0),
        Kind::Master => Some(if l.integrated_lufs > t.target_lufs + 0.05 { t.loud_ceiling_dbtp } else { t.ceiling_dbtp }),
        Kind::Reference => None,
    }
}

/// Most marks kept per kind; the loudest win.
const MAX_MARKS: usize = 40;

/// Runs of block indices (sorted), merging runs one block apart. Returns inclusive (first, last).
fn block_runs(blocks: impl Iterator<Item = usize>) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = vec![];
    for b in blocks {
        match runs.last_mut() {
            Some(r) if b <= r.1 + 2 => r.1 = b,
            _ => runs.push((b, b)),
        }
    }
    runs
}

/// Where clipping and true-peak overs are, under the same rules as the findings that report them.
pub fn peak_marks(r: &Report, t: &Targets) -> Vec<PeakMark> {
    let l = &r.loudness;
    let (Some(map), Some(ceiling)) = (&l.peak_map, tp_ceiling(l, t)) else { return vec![] };
    let tp = &map.tp_blocks;
    let block = crate::analyze::KBLOCK_S;
    let peak = |a: usize, b: usize| tp.get(a..=b.min(tp.len().saturating_sub(1))).and_then(|s| s.iter().copied().reduce(f32::max)).map_or(l.true_peak_dbtp, f64::from);
    let marks = |blocks: Vec<usize>, kind: PeakKind, severity: Severity| {
        let mut m: Vec<PeakMark> = block_runs(blocks.into_iter())
            .into_iter()
            .map(|(a, b)| PeakMark { start_s: a as f64 * block, end_s: (b + 1) as f64 * block, kind, peak_db: peak(a, b), severity })
            .collect();
        m.sort_by(|x, y| y.peak_db.total_cmp(&x.peak_db));
        m.truncate(MAX_MARKS);
        m
    };
    let mut out = marks(map.over_fs_blocks.iter().map(|&b| b as usize).collect(), PeakKind::OverFullScale, Severity::Error);
    // Same gate as the "clipping" finding: flat tops only matter near full scale.
    if l.sample_peak_dbfs > -0.5 {
        out.extend(marks(map.flat_top_blocks.iter().map(|&b| b as usize).collect(), PeakKind::Clipping, Severity::Error));
    }
    let over: Vec<usize> = tp.iter().enumerate().filter(|(_, &v)| f64::from(v) > ceiling + 0.05).map(|(i, _)| i).collect();
    let sev = if t.kind == Kind::Master { Severity::Error } else { Severity::Warn };
    out.extend(marks(over, PeakKind::TruePeak, sev));
    out.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    out
}

/// " Loudest at 2:14.3; 7 places in all." for one kind of mark, or "" when none were located.
fn located(marks: &[PeakMark], kind: PeakKind) -> String {
    let m: Vec<&PeakMark> = marks.iter().filter(|m| m.kind == kind).collect();
    let Some(top) = m.iter().max_by(|a, b| a.peak_db.total_cmp(&b.peak_db)) else { return String::new() };
    match m.len() {
        1 => format!(" It happens at {}.", clock(top.start_s)),
        n if n >= MAX_MARKS => format!(" Loudest at {}; at least {n} places in all.", clock(top.start_s)),
        n => format!(" Loudest at {}; {n} places in all.", clock(top.start_s)),
    }
}

pub fn evaluate(r: &Report, t: &Targets) -> Vec<Finding> {
    if t.kind == Kind::Reference {
        return vec![f(Severity::Ok, "reference", "Reference track", "Used as a reference: not checked against mix or master rules.")];
    }
    let th = Thresholds::new(t);
    let genre_name = t.genre.as_ref().map(|g| g.name.as_str());
    let mut out = vec![];
    let l = &r.loudness;
    let s = &r.stereo;
    let e = &r.edges;
    let sp = &r.spectrum;

    // ---- clipping / peaks ----
    if l.samples_over_full_scale > 0 {
        out.push(f(
            Severity::Error,
            "over_full_scale",
            "Samples above 0 dBFS",
            format!(
                "{} samples exceed full scale. They will clip as soon as this is converted to a fixed-point format.{}",
                l.samples_over_full_scale,
                located(&r.peak_marks, PeakKind::OverFullScale)
            ),
        ));
    }
    if l.flat_top_runs > 0 && l.sample_peak_dbfs > -0.5 {
        out.push(f(
            Severity::Error,
            "clipping",
            "Hard clipping",
            format!(
                "{} runs of 3+ samples pinned at the peak ({:.2} dBFS).{}",
                l.flat_top_runs,
                l.sample_peak_dbfs,
                located(&r.peak_marks, PeakKind::Clipping)
            ),
        ));
    }

    match t.kind {
        Kind::Mix => {
            if l.true_peak_dbtp > -1.0 {
                out.push(f(
                    Severity::Warn,
                    "mix_headroom",
                    "Little headroom for mastering",
                    format!(
                        "True peak is {:.1} dBTP. Leave roughly 3–6 dB so the master has room to work.{}",
                        l.true_peak_dbtp,
                        located(&r.peak_marks, PeakKind::TruePeak)
                    ),
                ));
            } else {
                out.push(f(Severity::Ok, "mix_headroom", "Room for mastering", format!("True peak {:.1} dBTP leaves the master room to work.", l.true_peak_dbtp)));
            }
            if l.plr_db < th.mix_plr_min {
                out.push(f(
                    Severity::Warn,
                    "mix_overcompressed",
                    "Mix bus may be over-compressed",
                    match genre_name {
                        Some(g) => format!("Peak-to-loudness ratio is {:.1} dB. {g} mixes usually sit above about {:.0} dB before mastering.", l.plr_db, th.mix_plr_min),
                        None => format!("Peak-to-loudness ratio is {:.1} dB. Mixes usually sit above about 10 dB before mastering.", l.plr_db),
                    },
                ));
            }
        }
        Kind::Master => {
            let loud = l.integrated_lufs > t.target_lufs + 0.05;
            let ceiling = tp_ceiling(l, t).unwrap_or(t.ceiling_dbtp);
            if l.true_peak_dbtp > ceiling + 0.05 {
                out.push(f(
                    Severity::Error,
                    "tp_ceiling",
                    "True peak above streaming ceiling",
                    format!(
                        "True peak {:.1} dBTP vs a {:.0} dBTP ceiling{}. Expect overs after lossy encoding.{}",
                        l.true_peak_dbtp,
                        ceiling,
                        if loud { " (stricter because the master is louder than the target)" } else { "" },
                        located(&r.peak_marks, PeakKind::TruePeak)
                    ),
                ));
            } else {
                out.push(f(Severity::Ok, "tp_ceiling", "True peak under the ceiling", format!("{:.1} dBTP against a {:.0} dBTP ceiling.", l.true_peak_dbtp, ceiling)));
            }
            let diff = l.integrated_lufs - t.target_lufs;
            let headroom_to_boost = t.ceiling_dbtp - l.true_peak_dbtp;
            if let Some(p) = &t.platform {
                // Platform-specific wording, including platforms that never turn tracks up.
                let pb = p.playback(r);
                let name = &p.name;
                if diff < -1.0 {
                    let (sev, detail) = match p.normalization {
                        Normalization::DownOnly => (
                            Severity::Warn,
                            format!(
                                "{:.1} LUFS vs {name}'s {:.0} LUFS. {name} doesn't turn quiet tracks up, so it plays {:.1} dB quieter than other tracks.",
                                l.integrated_lufs, t.target_lufs, pb.quieter_by_db
                            ),
                        ),
                        Normalization::UpAndDown if pb.quieter_by_db >= 0.5 => (
                            Severity::Warn,
                            format!(
                                "{:.1} LUFS vs {name}'s {:.0} LUFS. {name} raises it only {:.1} dB (limited by peak headroom), so it plays {:.1} dB quieter than other tracks.",
                                l.integrated_lufs, t.target_lufs, pb.gain_db, pb.quieter_by_db
                            ),
                        ),
                        Normalization::UpAndDown => (
                            Severity::Info,
                            format!(
                                "{:.1} LUFS vs {name}'s {:.0} LUFS. {name} turns it up {:.1} dB; there's enough peak headroom to reach the target.",
                                l.integrated_lufs, t.target_lufs, pb.gain_db
                            ),
                        ),
                    };
                    out.push(f(sev, "loudness_low", format!("Quieter than {name}'s target"), detail));
                } else if diff > 2.0 {
                    out.push(f(
                        Severity::Info,
                        "loudness_high",
                        format!("Louder than {name}'s target"),
                        format!("{:.1} LUFS will be turned down {:.1} dB on {name}. Extra limiting buys nothing there.", l.integrated_lufs, -pb.gain_db),
                    ));
                } else {
                    out.push(f(Severity::Ok, "loudness", "Loudness on target", format!("{:.1} LUFS ({name} target {:.0}).", l.integrated_lufs, t.target_lufs)));
                }
            } else if diff < -1.0 {
                let boost = (-diff).min(headroom_to_boost.max(0.0));
                out.push(f(
                    Severity::Warn,
                    "loudness_low",
                    "Quieter than the streaming target",
                    format!(
                        "{:.1} LUFS vs {:.0} LUFS. Platforms only raise it by about {:.1} dB (limited by peak headroom), so it will play about {:.1} dB quieter than other tracks.",
                        l.integrated_lufs,
                        t.target_lufs,
                        boost,
                        -diff - boost
                    ),
                ));
            } else if diff > 2.0 {
                out.push(f(
                    Severity::Info,
                    "loudness_high",
                    "Louder than the streaming target",
                    format!("{:.1} LUFS will be turned down {:.1} dB. Extra limiting buys nothing on normalized platforms.", l.integrated_lufs, diff),
                ));
            } else {
                out.push(f(Severity::Ok, "loudness", "Loudness on target", format!("{:.1} LUFS (target {:.0}).", l.integrated_lufs, t.target_lufs)));
            }
        }
        Kind::Reference => {}
    }

    // ---- file edges ----
    if e.leading_silence_s > 1.0 {
        out.push(f(Severity::Warn, "leading_silence", "Long silence at the start", format!("First sound at {:.1} s.", e.leading_silence_s)));
    }
    if e.trailing_silence_s > 10.0 {
        out.push(f(
            Severity::Error,
            "trailing_silence",
            "Long silence at the end",
            format!("{:.0} s of silence after the last sound. The track length will include it.", e.trailing_silence_s),
        ));
    } else if e.trailing_silence_s > 4.0 {
        out.push(f(Severity::Info, "trailing_silence", "Silence at the end", format!("{:.1} s after the last sound.", e.trailing_silence_s)));
    }
    // Above -60 dBFS (the level that counts as sound) the ending audibly stops; below it, only a faint tail is cut.
    if e.tail_truncated && e.tail_last50ms_dbfs > -60.0 {
        out.push(f(
            Severity::Warn,
            "tail_truncated",
            "Sound cuts off at the end",
            format!(
                "The last 50 ms still peak at {:.0} dBFS, so the ending stops abruptly. Extend the bounce range until the tail decays to silence.",
                e.tail_last50ms_dbfs
            ),
        ));
    } else if e.tail_truncated {
        out.push(f(
            Severity::Info,
            "tail_truncated",
            "Quiet tail cut off at the end",
            format!(
                "The last 50 ms peak at {:.0} dBFS: very quiet, but not silent. {}",
                e.tail_last50ms_dbfs,
                if t.kind == Kind::Master {
                    "A short fade at the very end rules out a click."
                } else {
                    "Mastering gain will raise it, so bounce a second longer and let it fade out."
                }
            ),
        ));
    }
    for gap in &e.internal_silences {
        out.push(f(
            Severity::Error,
            "internal_silence",
            "Digital silence inside the song",
            format!(
                "{} to {} is silent. If that isn't intentional, tracks may have been muted or soloed during export.",
                clock(gap.start_s),
                clock(gap.end_s)
            ),
        ));
    }
    if t.kind == Kind::Mix && e.fade_out_detected {
        out.push(f(
            Severity::Info,
            "mix_fade",
            "Fade-out baked into the mix",
            "Mastering limiters reshape fades. Consider sending an unfaded mix and fading after the limiter.",
        ));
    }

    // ---- DC / balance ----
    let dc = l.dc_offset[0].abs().max(l.dc_offset[1].abs());
    if dc > 1e-3 {
        out.push(f(Severity::Warn, "dc_offset", "DC offset", format!("{:.4} (≈{:.0} dBFS). A high-pass at ~10 Hz removes it.", dc, 20.0 * dc.log10())));
    }
    if l.lr_balance_db.abs() > 1.5 {
        out.push(f(
            Severity::Warn,
            "lr_balance",
            "Left/right imbalance",
            format!(
                "The {} channel is {:.1} dB louder. Check panning and any effect or send that only feeds one side.",
                if l.lr_balance_db > 0.0 { "left" } else { "right" },
                l.lr_balance_db.abs()
            ),
        ));
    }

    // ---- stereo ----
    if s.low_correlation < th.low_corr_min {
        out.push(f(
            Severity::Warn,
            "low_end_width",
            "Wide low end",
            format!("Correlation below 120 Hz is {:.2}. Keep bass close to mono for club systems and vinyl.", s.low_correlation),
        ));
    }
    let loud_neg: Vec<&CorrWindow> = s.low_corr_windows.iter().filter(|w| w.corr < 0.0 && w.level_dbfs > -40.0).collect();
    if !loud_neg.is_empty() {
        out.push(f(
            Severity::Warn,
            "phase",
            "Out-of-phase moments",
            format!(
                "{} loud {} where left and right partly cancel (correlation below 0), first at {} ({:.2}). Check for a polarity-flipped mic or a stereo widener.",
                loud_neg.len(),
                if loud_neg.len() == 1 { "moment" } else { "moments" },
                clock(loud_neg[0].t),
                loud_neg[0].corr
            ),
        ));
    }
    if s.mono_loss_lu < -1.5 {
        out.push(f(
            Severity::Warn,
            "mono",
            "Loses level in mono",
            format!("{:.1} LU quieter when folded to mono, so phone speakers and mono club systems lose part of the mix.", -s.mono_loss_lu),
        ));
    } else {
        out.push(f(Severity::Ok, "mono", "Holds up in mono", format!("Level changes {:+.2} LU when folded to mono.", s.mono_loss_lu)));
    }

    // ---- tone ----
    let dev = |lo: f64, hi: f64| sp.third_octave.iter().filter(|b| b.fc >= lo && b.fc <= hi).collect::<Vec<_>>();
    if let Some(b) = dev(200.0, 800.0).into_iter().max_by(|a, b| a.dev_db.partial_cmp(&b.dev_db).unwrap()) {
        if b.dev_db > 3.5 {
            // A bump centred on the key's root or fifth is partly the music itself.
            let (lo, hi) = (b.fc / 2f64.powf(1.0 / 6.0), b.fc * 2f64.powf(1.0 / 6.0));
            let musical = r.key_analysis.as_ref().and_then(|k| {
                k.mid_peaks
                    .iter()
                    .filter(|p| p.hz >= lo && p.hz <= hi && matches!(p.role, Role::Root | Role::Fifth))
                    .max_by(|a, b| a.above_median_db.partial_cmp(&b.above_median_db).unwrap())
                    .map(|p| (p, k.key_name.as_str()))
            });
            out.push(match musical {
                Some((p, key)) => f(
                    Severity::Info,
                    "low_mid_buildup",
                    "Low-mid build-up on a key note",
                    format!(
                        "{:.0} Hz sits {:+.1} dB above the overall tilt, centred on {} ({:.0} Hz), the {} of {key}, so part of it is the music. If it still sounds boxy, cut narrowly at {:.0} Hz rather than a broad dip.",
                        b.fc, b.dev_db, p.note, p.hz, p.role.label(), p.hz
                    ),
                ),
                None => f(
                    Severity::Warn,
                    "low_mid_buildup",
                    "Low-mid build-up",
                    format!("{:.0} Hz sits {:.1} dB above the overall tilt, which can sound boxy or muddy.", b.fc, b.dev_db),
                ),
            });
        }
    }
    if let Some(b) = dev(2000.0, 5000.0).into_iter().min_by(|a, b| a.dev_db.partial_cmp(&b.dev_db).unwrap()) {
        if b.dev_db < -3.5 {
            out.push(f(Severity::Info, "presence_dip", "Presence dip", format!("{:.0} Hz sits {:.1} dB below the overall tilt, which can push vocals and leads back.", b.fc, -b.dev_db)));
        }
    }
    if let Some(b) = sp.third_octave.iter().find(|b| (b.fc - 12700.0).abs() < 400.0) {
        if b.dev_db < -5.0 {
            out.push(f(
                Severity::Info,
                "dark_top",
                "Dark top end",
                format!("12.7 kHz sits {:.1} dB below the overall tilt. Fine for warm genres; a gentle high shelf adds air.", -b.dev_db),
            ));
        }
    }
    if sp.sibilance.p99_db > th.sibilance_p99_max {
        out.push(f(
            Severity::Warn,
            "sibilance",
            "Strong 5–10 kHz peaks",
            format!("In the loudest 1% of moments, 5–10 kHz comes within {:.1} dB of the full band. Check S's and hi-hats.", -sp.sibilance.p99_db),
        ));
    }
    if sp.sub30_share_db > -20.0 {
        out.push(f(
            Severity::Warn,
            "subsonic",
            "Heavy sub-30 Hz energy",
            format!(
                "Energy below 30 Hz is only {:.1} dB under the total. Most speakers can't play it, but it still uses limiter headroom; a high-pass around 25–30 Hz frees it.",
                -sp.sub30_share_db
            ),
        ));
    }

    // ---- genre ----
    if let (Some(g), Some(m)) = (&t.genre, &r.genre_match) {
        let excess = |b: &&crate::genre::GenreBand| b.delta_db.abs() - b.tolerance_db;
        let mut off: Vec<_> = m.bands.iter().filter(|b| !b.within()).collect();
        off.sort_by(|a, b| excess(b).partial_cmp(&excess(a)).unwrap());
        if off.is_empty() {
            out.push(f(Severity::Ok, "genre_tone", format!("Tonal balance fits {}", g.name), format!("Every octave is within tolerance of a typical {} balance.", g.name)));
        } else {
            let list: Vec<String> = off
                .iter()
                .take(3)
                .map(|b| format!("{} {:.1} dB {}", band_label(b.lo, b.hi), b.delta_db.abs(), if b.delta_db < 0.0 { "lower" } else { "higher" }))
                .collect();
            out.push(f(
                Severity::Warn,
                "genre_tone",
                format!("Tonal balance differs from typical {}", g.name),
                format!(
                    "Against a typical {} balance: {}. Genre targets are broad averages, so compare with a reference track before reaching for EQ.",
                    g.name,
                    list.join(", ")
                ),
            ));
        }
        let tilt = sp.tilt_db_per_oct;
        if tilt < g.tilt_range.0 || tilt > g.tilt_range.1 {
            out.push(f(
                Severity::Info,
                "genre_tilt",
                if tilt < g.tilt_range.0 { format!("Darker than typical {}", g.name) } else { format!("Brighter than typical {}", g.name) },
                format!("Tilt is {:.1} dB per octave; {} usually sits between {:.1} and {:.1}.", tilt, g.name, g.tilt_range.0, g.tilt_range.1),
            ));
        }
        if t.kind == Kind::Master {
            let (lo, hi) = g.master_lufs_range;
            if l.integrated_lufs < lo || l.integrated_lufs > hi {
                let quieter = l.integrated_lufs < lo;
                let range = format!("{:.1} LUFS; {} masters usually land between {:.0} and {:.0} LUFS.", l.integrated_lufs, g.name, lo, hi);
                let platform = t.platform.as_ref().map_or("Streaming platforms", |p| p.name.as_str());
                // Below the streaming target, the loudness finding already explains playback.
                let detail = if quieter && l.integrated_lufs < t.target_lufs - 1.0 {
                    range
                } else if quieter {
                    format!(
                        "{range} {platform} turns those down to about {:.0} LUFS, so this won't sound quieter there. Push louder only if you want the denser sound of a heavily limited master.",
                        t.target_lufs
                    )
                } else {
                    format!(
                        "{range} {platform} turns it down to about {:.0} LUFS, so the extra limiting costs dynamics without making it louder there.",
                        t.target_lufs
                    )
                };
                out.push(f(
                    Severity::Info,
                    "genre_loudness",
                    format!("{} than most {} masters", if quieter { "Quieter" } else { "Louder" }, g.name),
                    detail,
                ));
            }
            // Each note, and whether the value is above the genre's range.
            let mut notes: Vec<(String, bool)> = vec![];
            if l.plr_db < g.master_plr_range.0 || l.plr_db > g.master_plr_range.1 {
                notes.push((
                    format!("Peak-to-loudness ratio is {:.1} dB ({} usually {:.0}–{:.0})", l.plr_db, g.name, g.master_plr_range.0, g.master_plr_range.1),
                    l.plr_db > g.master_plr_range.1,
                ));
            }
            if l.lra_lu < g.lra_range.0 || l.lra_lu > g.lra_range.1 {
                let lead = if notes.is_empty() { "Loudness range" } else { "loudness range" };
                notes.push((format!("{lead} is {:.1} LU (usually {:.0}–{:.0})", l.lra_lu, g.lra_range.0, g.lra_range.1), l.lra_lu > g.lra_range.1));
            }
            if !notes.is_empty() {
                let text: Vec<&str> = notes.iter().map(|(n, _)| n.as_str()).collect();
                let (title, why) = if notes.iter().all(|(_, above)| *above) {
                    (format!("More dynamic than most {} masters", g.name), " Fine if intended; next to typical tracks in the genre it will sound less dense.")
                } else if notes.iter().all(|(_, above)| !*above) {
                    (format!("Less dynamic than most {} masters", g.name), " Heavy limiting flattens transients, and platforms turn loud masters down anyway.")
                } else {
                    (format!("Dynamics differ from typical {}", g.name), "")
                };
                out.push(f(Severity::Info, "genre_dynamics", title, format!("{}.{why}", text.join(", and "))));
            }
        }
    }

    // ---- key ----
    if let Some(k) = &r.key_analysis {
        let mut off: Vec<_> = k.peaks.iter().filter(|p| !p.role.musical() && p.above_median_db >= 8.0).collect();
        off.sort_by(|a, b| b.above_median_db.partial_cmp(&a.above_median_db).unwrap());
        if !off.is_empty() {
            let list: Vec<String> = off.iter().take(3).map(|p| format!("{:.0} Hz ({}, {}) {:+.0} dB", p.hz, p.note, p.role.label(), p.above_median_db)).collect();
            out.push(f(
                Severity::Warn,
                "lf_resonance",
                "Bass peaks outside the key",
                format!(
                    "{} above the 30–300 Hz median. Peaks that aren't notes of {} are more likely room or EQ resonances or a clashing note than the bassline. Try a narrow cut (Q 6–10) at {:.0} Hz and listen.",
                    list.join(", "),
                    k.key_name,
                    off[0].hz
                ),
            ));
        }
        if let Some(c) = k.tuning_offset_cents.filter(|c| c.abs() > 20.0) {
            out.push(f(
                Severity::Info,
                "tuning",
                format!("Tuned {:+.0} cents from A=440", c),
                format!(
                    "Tonal peaks sit about {:+.0} cents from standard tuning (A4 ≈ {:.1} Hz). If that isn't intended, a sample or instrument may be detuned. Note labels here already allow for it.",
                    c, k.a4_hz
                ),
            ));
        }
    }

    // ---- format ----
    if let Some(d) = &r.dither {
        if d.bits <= 16 && d.samples_checked > 0 && !d.detected && d.nonzero_fraction < 0.01 {
            out.push(f(Severity::Info, "dither", "No dither visible", d.note.clone()));
        }
    }
    if r.file.sample_format == "float32" || r.file.sample_format == "float64" {
        if t.kind == Kind::Master {
            out.push(f(Severity::Info, "format", "Floating-point master", "Most distributors want 16- or 24-bit integer WAV. Export a dithered integer copy."));
        }
    }

    out.sort_by(|a, b| b.severity.cmp(&a.severity));
    out
}
