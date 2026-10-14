//! Deterministic rules that turn measurements into findings. Fast, offline, and the
//! AI mode builds on them rather than replacing them.

use crate::key::Role;
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

fn band_label(lo: f64, hi: f64) -> String {
    let k = |x: f64| if x >= 1000.0 { format!("{}k", (x / 100.0).round() / 10.0) } else { format!("{x:.0}") };
    format!("{}–{} Hz", k(lo), k(hi))
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
            format!("{} samples exceed full scale. They will clip as soon as this is converted to a fixed-point format.", l.samples_over_full_scale),
        ));
    }
    if l.flat_top_runs > 0 && l.sample_peak_dbfs > -0.5 {
        out.push(f(
            Severity::Error,
            "clipping",
            "Hard clipping",
            format!("{} runs of 3+ samples pinned at the peak ({:.2} dBFS).", l.flat_top_runs, l.sample_peak_dbfs),
        ));
    }

    match t.kind {
        Kind::Mix => {
            if l.true_peak_dbtp > -1.0 {
                out.push(f(
                    Severity::Warn,
                    "mix_headroom",
                    "Little headroom for mastering",
                    format!("True peak is {:.1} dBTP. Leave roughly 3–6 dB so the master has room to work.", l.true_peak_dbtp),
                ));
            } else {
                out.push(f(Severity::Ok, "mix_headroom", "Headroom", format!("True peak {:.1} dBTP.", l.true_peak_dbtp)));
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
            let ceiling = if loud { t.loud_ceiling_dbtp } else { t.ceiling_dbtp };
            if l.true_peak_dbtp > ceiling + 0.05 {
                out.push(f(
                    Severity::Error,
                    "tp_ceiling",
                    "True peak above streaming ceiling",
                    format!(
                        "True peak {:.1} dBTP vs a {:.0} dBTP ceiling{}. Expect overs after lossy encoding.",
                        l.true_peak_dbtp,
                        ceiling,
                        if loud { " (stricter because the master is louder than the target)" } else { "" }
                    ),
                ));
            } else {
                out.push(f(Severity::Ok, "tp_ceiling", "True peak", format!("{:.1} dBTP, within the {:.0} dBTP ceiling.", l.true_peak_dbtp, ceiling)));
            }
            let diff = l.integrated_lufs - t.target_lufs;
            let headroom_to_boost = t.ceiling_dbtp - l.true_peak_dbtp;
            if diff < -1.0 {
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
    if e.tail_truncated {
        out.push(f(
            Severity::Warn,
            "tail_truncated",
            "File ends before the sound does",
            format!(
                "The last 50 ms still peak at {:.0} dBFS. Extend the bounce range so the tail decays to silence{}.",
                e.tail_last50ms_dbfs,
                if t.kind == Kind::Master { "" } else { "; mastering gain will raise it" }
            ),
        ));
    }
    for gap in &e.internal_silences {
        out.push(f(
            Severity::Error,
            "internal_silence",
            "Digital silence inside the song",
            format!(
                "{:.1}–{:.1} s is silent. If that isn't intentional, tracks may have been muted or soloed during export.",
                gap.start_s, gap.end_s
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
        out.push(f(Severity::Warn, "lr_balance", "Left/right imbalance", format!("{:+.1} dB (L vs R).", l.lr_balance_db)));
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
                "{} loud window(s) with negative correlation, first at {:.1} s ({:.2}).",
                loud_neg.len(),
                loud_neg[0].t,
                loud_neg[0].corr
            ),
        ));
    }
    if s.mono_loss_lu < -1.5 {
        out.push(f(Severity::Warn, "mono", "Loses level in mono", format!("{:.1} LU quieter when folded to mono.", s.mono_loss_lu)));
    } else {
        out.push(f(Severity::Ok, "mono", "Mono compatibility", format!("{:+.2} LU when folded to mono.", s.mono_loss_lu)));
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
                    format!("{:.0} Hz sits {:+.1} dB above the overall tilt; can read as boxy or muddy.", b.fc, b.dev_db),
                ),
            });
        }
    }
    if let Some(b) = dev(2000.0, 5000.0).into_iter().min_by(|a, b| a.dev_db.partial_cmp(&b.dev_db).unwrap()) {
        if b.dev_db < -3.5 {
            out.push(f(Severity::Info, "presence_dip", "Presence dip", format!("{:.0} Hz is {:.1} dB below the tilt.", b.fc, b.dev_db)));
        }
    }
    if let Some(b) = sp.third_octave.iter().find(|b| (b.fc - 12700.0).abs() < 400.0) {
        if b.dev_db < -5.0 {
            out.push(f(
                Severity::Info,
                "dark_top",
                "Dark top end",
                format!("12.7 kHz is {:.1} dB below the tilt. Fine for warm genres; a gentle shelf adds air.", b.dev_db),
            ));
        }
    }
    if sp.sibilance.p99_db > th.sibilance_p99_max {
        out.push(f(
            Severity::Warn,
            "sibilance",
            "Strong 5–10 kHz peaks",
            format!("In the loudest 1% of moments 5–10 kHz is within {:.1} dB of the full band. Check S's and hats.", -sp.sibilance.p99_db),
        ));
    }
    if sp.sub30_share_db > -20.0 {
        out.push(f(Severity::Warn, "subsonic", "Heavy sub-30 Hz energy", format!("{:.1} dB of total energy below 30 Hz eats limiter headroom.", sp.sub30_share_db)));
    }

    // ---- genre ----
    if let (Some(g), Some(m)) = (&t.genre, &r.genre_match) {
        let excess = |b: &&crate::genre::GenreBand| b.delta_db.abs() - b.tolerance_db;
        let mut off: Vec<_> = m.bands.iter().filter(|b| !b.within()).collect();
        off.sort_by(|a, b| excess(b).partial_cmp(&excess(a)).unwrap());
        if off.is_empty() {
            out.push(f(Severity::Ok, "genre_tone", "Tonal balance", format!("Every octave is within tolerance of a typical {} balance.", g.name)));
        } else {
            let list: Vec<String> = off.iter().take(3).map(|b| format!("{} {:+.1} dB", band_label(b.lo, b.hi), b.delta_db)).collect();
            out.push(f(
                Severity::Warn,
                "genre_tone",
                format!("Tonal balance differs from typical {}", g.name),
                format!("{} versus the genre target. Check against a reference track before reaching for EQ.", list.join(", ")),
            ));
        }
        let tilt = sp.tilt_db_per_oct;
        if tilt < g.tilt_range.0 || tilt > g.tilt_range.1 {
            out.push(f(
                Severity::Info,
                "genre_tilt",
                if tilt < g.tilt_range.0 { "Darker than typical for the genre" } else { "Brighter than typical for the genre" },
                format!("Tilt is {:.1} dB/oct; {} usually sits between {:.1} and {:.1}.", tilt, g.name, g.tilt_range.0, g.tilt_range.1),
            ));
        }
        if t.kind == Kind::Master {
            let (lo, hi) = g.master_lufs_range;
            if l.integrated_lufs < lo || l.integrated_lufs > hi {
                out.push(f(
                    Severity::Info,
                    "genre_loudness",
                    if l.integrated_lufs < lo { "Quieter than typical genre masters" } else { "Louder than typical genre masters" },
                    format!(
                        "{:.1} LUFS; {} masters usually land between {:.0} and {:.0} LUFS. Streaming platforms still normalize to about {:.0} LUFS either way.",
                        l.integrated_lufs, g.name, lo, hi, t.target_lufs
                    ),
                ));
            }
            let mut notes = vec![];
            if l.plr_db < g.master_plr_range.0 || l.plr_db > g.master_plr_range.1 {
                notes.push(format!("PLR {:.1} dB (typical {:.0}–{:.0})", l.plr_db, g.master_plr_range.0, g.master_plr_range.1));
            }
            if l.lra_lu < g.lra_range.0 || l.lra_lu > g.lra_range.1 {
                notes.push(format!("loudness range {:.1} LU (typical {:.0}–{:.0})", l.lra_lu, g.lra_range.0, g.lra_range.1));
            }
            if !notes.is_empty() {
                out.push(f(Severity::Info, "genre_dynamics", "Dynamics outside the genre's usual range", format!("{} for {}.", notes.join(", "), g.name)));
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
