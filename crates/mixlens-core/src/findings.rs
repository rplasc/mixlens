//! Deterministic rules that turn measurements into findings. Fast, offline, and the
//! AI mode builds on them rather than replacing them.

use crate::report::*;

fn f(sev: Severity, code: &str, title: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding { severity: sev, code: code.into(), title: title.into(), detail: detail.into() }
}

pub fn evaluate(r: &Report, t: &Targets) -> Vec<Finding> {
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
            if l.plr_db < 9.0 {
                out.push(f(
                    Severity::Warn,
                    "mix_overcompressed",
                    "Mix bus may be over-compressed",
                    format!("Peak-to-loudness ratio is {:.1} dB. Mixes usually sit above about 10 dB before mastering.", l.plr_db),
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
    if s.low_correlation < 0.8 {
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
            out.push(f(
                Severity::Warn,
                "low_mid_buildup",
                "Low-mid build-up",
                format!("{:.0} Hz sits {:+.1} dB above the overall tilt; can read as boxy or muddy.", b.fc, b.dev_db),
            ));
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
    if sp.sibilance.p99_db > -6.0 {
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
