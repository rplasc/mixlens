//! The single-file analysis pass. Independent analyzers run in parallel over the decoded buffers.

use crate::audio::Audio;
use crate::dsp::{self, amp_db, pow_db, Chain};
use crate::report::*;
use crate::{Error, Result};
use ebur128::{EbuR128, Mode};
use std::time::Instant;

pub const KBLOCK_S: f64 = 0.1;
const ST_HOP_S: f64 = 0.5;
const SILENCE_DBFS: f64 = -60.0;
/// Most blocks of each clipping kind kept in a [`PeakMap`].
const MAX_CLIP_BLOCKS: usize = 4000;

pub fn analyze(audio: &Audio, targets: &Targets) -> Result<Report> {
    let t0 = Instant::now();
    let ((ebu, mono), ((kblocks, stereo), ((spectrum, sibilance), edges_etc))) = rayon::join(
        || rayon::join(|| ebu_main(audio), || mono_fold(audio)),
        || {
            rayon::join(
                || rayon::join(|| kblocks(audio), || stereo(audio)),
                || rayon::join(|| rayon::join(|| spectrum(audio), || sibilance(audio)), || edges_and_levels(audio)),
            )
        },
    );
    let ebu = ebu?;
    let mono = mono?;
    let (edges, levels, dither, frames) = edges_etc;
    let mut stereo = stereo;
    stereo.mono_fold_lufs = mono;
    stereo.mono_loss_lu = mono - ebu.integrated;
    let mut spectrum = spectrum;
    spectrum.sibilance = sibilance;

    let true_peak = amp_db(ebu.true_peak);
    let peak_l = amp_db(levels.peak[0]);
    let peak_r = amp_db(levels.peak[1]);
    let sample_peak = peak_l.max(peak_r);
    let loudness = Loudness {
        integrated_lufs: ebu.integrated,
        lra_lu: ebu.lra,
        true_peak_dbtp: true_peak,
        sample_peak_dbfs: sample_peak,
        sample_peak_l_dbfs: peak_l,
        sample_peak_r_dbfs: peak_r,
        rms_dbfs: levels.gated_rms_db,
        crest_db: sample_peak - levels.gated_rms_db,
        plr_db: true_peak - ebu.integrated,
        max_short_term_lufs: ebu.max_st,
        max_momentary_lufs: ebu.max_m,
        samples_near_full_scale: levels.near_fs,
        samples_over_full_scale: levels.over_fs,
        flat_top_runs: levels.flat_runs,
        dc_offset: levels.dc,
        lr_balance_db: levels.balance_db,
        short_term: ebu.short_term,
        short_term_hop_s: ST_HOP_S,
        peak_map: Some(PeakMap { tp_blocks: ebu.tp_blocks, over_fs_blocks: levels.over_fs_blocks, flat_top_blocks: levels.flat_top_blocks }),
    };

    let mut edges = edges;
    edges.fade_out_detected = detect_fade(&loudness.short_term, edges.last_sound_s);
    let sections = sections(&loudness.short_term, ebu.integrated, &kblocks, audio.info.duration_s, &frames);

    let mut report = Report {
        schema: SCHEMA_VERSION,
        file: audio.info.clone(),
        loudness,
        stereo,
        spectrum,
        edges,
        sections,
        dither,
        findings: vec![],
        genre_match: None,
        key_analysis: None,
        peak_marks: vec![],
        analysis_ms: 0,
        kblocks,
    };
    crate::refresh_findings(&mut report, targets);
    report.analysis_ms = t0.elapsed().as_millis() as u64;
    Ok(report)
}

// ---------- loudness ----------

struct EbuOut {
    integrated: f64,
    lra: f64,
    true_peak: f64,
    max_st: f64,
    max_m: f64,
    short_term: Vec<f32>,
    /// True peak per 100 ms block (dBTP).
    tp_blocks: Vec<f32>,
}

fn finite_or(x: f64, d: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        d
    }
}

fn ebu_main(a: &Audio) -> Result<EbuOut> {
    let sr = a.sample_rate;
    let mode = Mode::I | Mode::LRA | Mode::TRUE_PEAK | Mode::S | Mode::M;
    let mut m = EbuR128::new(2, sr, mode).map_err(|e| Error::Analysis(format!("ebur128: {e:?}")))?;
    let hop = ((sr as f64) * KBLOCK_S).round() as usize;
    let per_st = (ST_HOP_S / KBLOCK_S).round() as usize;
    let n = a.len();
    let mut st = Vec::with_capacity(n / (hop * per_st) + 1);
    let mut tp_blocks = Vec::with_capacity(n / hop + 1);
    let (mut max_st, mut max_m) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut i = 0usize;
    let mut k = 0usize;
    while i < n {
        let end = (i + hop).min(n);
        m.add_frames_planar_f32(&[&a.left[i..end], &a.right[i..end]])
            .map_err(|e| Error::Analysis(format!("ebur128: {e:?}")))?;
        let tp = m.prev_true_peak(0).unwrap_or(0.0).max(m.prev_true_peak(1).unwrap_or(0.0));
        tp_blocks.push(amp_db(tp) as f32);
        let mo = m.loudness_momentary().unwrap_or(f64::NEG_INFINITY);
        if mo.is_finite() && end - i == hop {
            max_m = max_m.max(mo);
        }
        k += 1;
        if k % per_st == 0 || end == n {
            let s = m.loudness_shortterm().unwrap_or(f64::NEG_INFINITY);
            if s.is_finite() {
                max_st = max_st.max(s);
            }
            st.push(finite_or(s, -120.0).max(-120.0) as f32);
        }
        i = end;
    }
    let integrated = finite_or(m.loudness_global().unwrap_or(f64::NEG_INFINITY), -120.0);
    let lra = finite_or(m.loudness_range().unwrap_or(0.0), 0.0);
    let tp = m.true_peak(0).unwrap_or(0.0).max(m.true_peak(1).unwrap_or(0.0));
    Ok(EbuOut {
        integrated,
        lra,
        true_peak: tp,
        max_st: finite_or(max_st, -120.0),
        max_m: finite_or(max_m, -120.0),
        short_term: st,
        tp_blocks,
    })
}

fn mono_fold(a: &Audio) -> Result<f64> {
    let mid = a.mid();
    let mut m = EbuR128::new(2, a.sample_rate, Mode::I).map_err(|e| Error::Analysis(format!("{e:?}")))?;
    let hop = a.sample_rate as usize;
    for c in mid.chunks(hop) {
        m.add_frames_planar_f32(&[c, c]).map_err(|e| Error::Analysis(format!("{e:?}")))?;
    }
    Ok(finite_or(m.loudness_global().unwrap_or(f64::NEG_INFINITY), -120.0))
}

/// K-weighted energy per 100 ms sub-block, summed over both channels.
fn kblocks(a: &Audio) -> Vec<f32> {
    let fs = a.sample_rate as f64;
    let hop = (fs * KBLOCK_S).round() as usize;
    let (kl, kr) = rayon::join(
        || Chain::k_weighting(fs).process(&a.left),
        || Chain::k_weighting(fs).process(&a.right),
    );
    kl.chunks(hop)
        .zip(kr.chunks(hop))
        .filter(|(c, _)| c.len() == hop)
        .map(|(l, r)| (dsp::mean_square(l) + dsp::mean_square(r)) as f32)
        .collect()
}

/// BS.1770 gated loudness over a time range, from stored sub-block energies.
pub fn range_loudness(kblocks: &[f32], t0: f64, t1: f64) -> f64 {
    let i0 = ((t0.max(0.0)) / KBLOCK_S).floor() as usize;
    let i1 = (((t1.max(0.0)) / KBLOCK_S).ceil() as usize).min(kblocks.len());
    if i1 < i0 + 4 {
        return -120.0;
    }
    let blocks: Vec<f64> = (i0..=i1 - 4)
        .map(|j| kblocks[j..j + 4].iter().map(|&v| v as f64).sum::<f64>() / 4.0)
        .collect();
    let lk = |z: f64| -0.691 + pow_db(z);
    let abs: Vec<f64> = blocks.iter().copied().filter(|&z| lk(z) > -70.0).collect();
    if abs.is_empty() {
        return -120.0;
    }
    let rel_thr = lk(abs.iter().sum::<f64>() / abs.len() as f64) - 10.0;
    let rel: Vec<f64> = abs.into_iter().filter(|&z| lk(z) > rel_thr).collect();
    if rel.is_empty() {
        return -120.0;
    }
    lk(rel.iter().sum::<f64>() / rel.len() as f64)
}

// ---------- stereo ----------

fn stereo(a: &Audio) -> Stereo {
    let fs = a.sample_rate as f64;
    let (l, r) = (&a.left, &a.right);
    let correlation = dsp::correlation(l, r);
    let (mut ms_m, mut ms_s) = (0.0f64, 0.0f64);
    for (&x, &y) in l.iter().zip(r) {
        let m = 0.5 * (x as f64 + y as f64);
        let s = 0.5 * (x as f64 - y as f64);
        ms_m += m * m;
        ms_s += s * s;
    }
    let side_mid_db = pow_db(ms_s) - pow_db(ms_m);

    let w = (0.4 * fs) as usize;
    let mut corrs = Vec::new();
    let mut lows = Vec::new();
    let mut i = 0;
    while i + w <= l.len() {
        let (a1, b1) = (&l[i..i + w], &r[i..i + w]);
        if dsp::mean_square(a1).sqrt() > 1e-3 {
            let c = dsp::correlation(a1, b1);
            corrs.push(c);
            if c < 0.35 {
                let mid_ms = a1.iter().zip(b1).map(|(x, y)| {
                    let m = 0.5 * (*x as f64 + *y as f64);
                    m * m
                }).sum::<f64>() / w as f64;
                lows.push(CorrWindow { t: i as f64 / fs, corr: c, level_dbfs: pow_db(mid_ms) });
            }
        }
        i += w;
    }
    // Keep the loudest low-correlation windows: those matter, quiet reverb tails don't.
    lows.sort_by(|a, b| b.level_dbfs.partial_cmp(&a.level_dbfs).unwrap());
    lows.truncate(20);
    lows.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    let neg = corrs.iter().filter(|&&c| c < 0.0).count() as f64 / corrs.len().max(1) as f64;
    let s = dsp::sorted(corrs);

    let (ll, rl) = rayon::join(
        || Chain::butter4_lowpass(fs, 120.0).process(l),
        || Chain::butter4_lowpass(fs, 120.0).process(r),
    );
    let low_correlation = dsp::correlation(&ll, &rl);
    let (mut lm, mut lsd) = (0.0f64, 0.0f64);
    for (&x, &y) in ll.iter().zip(&rl) {
        lm += (x as f64 + y as f64).powi(2);
        lsd += (x as f64 - y as f64).powi(2);
    }
    Stereo {
        correlation,
        side_mid_db,
        low_correlation,
        low_side_mid_db: pow_db(lsd) - pow_db(lm),
        mono_fold_lufs: 0.0,
        mono_loss_lu: 0.0,
        window_corr_min: s.first().copied().unwrap_or(1.0),
        window_corr_p5: dsp::percentile(&s, 5.0),
        window_corr_median: dsp::percentile(&s, 50.0),
        negative_window_fraction: neg,
        low_corr_windows: lows,
    }
}

// ---------- spectrum ----------

pub const OCTAVE_BANDS: [(f64, f64); 12] = [
    (20.0, 40.0),
    (40.0, 80.0),
    (80.0, 160.0),
    (160.0, 320.0),
    (320.0, 640.0),
    (640.0, 1250.0),
    (1250.0, 2500.0),
    (2500.0, 5000.0),
    (5000.0, 8000.0),
    (8000.0, 12000.0),
    (12000.0, 16000.0),
    (16000.0, 20000.0),
];

fn band_sum(f: &[f64], p: &[f64], lo: f64, hi: f64) -> f64 {
    f.iter().zip(p).filter(|(&fr, _)| fr >= lo && fr < hi).map(|(_, &v)| v).sum()
}

fn band_mean(f: &[f64], p: &[f64], lo: f64, hi: f64) -> f64 {
    let (mut s, mut n) = (0.0, 0usize);
    for (&fr, &v) in f.iter().zip(p) {
        if fr >= lo && fr < hi {
            s += v;
            n += 1;
        }
    }
    if n == 0 {
        f64::NAN
    } else {
        s / n as f64
    }
}

/// Third-octave deviation from the fitted tilt, plus the slope.
pub fn third_octave(f: &[f64], p: &[f64], fs: f64) -> (f64, Vec<BandDev>) {
    let nyq = fs / 2.0;
    let centers: Vec<f64> = (0..30).map(|i| 25.0 * 2f64.powf(i as f64 / 3.0)).filter(|&c| c < 18000.0 && c * 1.12 < nyq).collect();
    let levels: Vec<f64> = centers.iter().map(|&c| pow_db(band_mean(f, p, c / 2f64.powf(1.0 / 6.0), c * 2f64.powf(1.0 / 6.0)))).collect();
    let lc: Vec<f64> = centers.iter().map(|c| c.log2()).collect();
    let (lo, hi) = (2usize, centers.len().saturating_sub(3).max(3));
    let (slope, icpt) = dsp::linfit(&lc[lo..hi], &levels[lo..hi]);
    let bands = centers
        .iter()
        .zip(&levels)
        .zip(&lc)
        .map(|((&fc, &lv), &x)| BandDev { fc, level_db: lv, dev_db: lv - (slope * x + icpt) })
        .collect();
    (slope + 3.0103, bands)
}

pub fn octave_shares(f: &[f64], p: &[f64]) -> Vec<OctaveShare> {
    let total = band_sum(f, p, 20.0, 20000.0);
    OCTAVE_BANDS
        .iter()
        .map(|&(lo, hi)| OctaveShare { lo, hi, share_db: pow_db(band_sum(f, p, lo, hi) / total) })
        .collect()
}

fn spectrum(a: &Audio) -> Spectrum {
    let fs = a.sample_rate as f64;
    let mid = a.mid();
    let nper = if fs > 60000.0 { 32768 } else { 16384 };
    let (f, p) = dsp::welch(&mid, fs, nper);
    let (tilt, third) = third_octave(&f, &p, fs);
    let total = band_sum(&f, &p, 20.0, 20000.0);
    // Finer resolution (~0.7 Hz) so adjacent bass notes separate.
    let n = if fs > 60000.0 { 131072 } else { 65536 };
    let (fl, pl) = dsp::welch(&mid, fs, n);
    let lf_peaks = tonal_peaks(&fl, &pl, 30.0, 300.0);
    let mid_peaks = tonal_peaks(&fl, &pl, 200.0, 800.0);
    Spectrum {
        tilt_db_per_oct: tilt,
        third_octave: third,
        octave_share: octave_shares(&f, &p),
        lf_peaks,
        mid_peaks,
        sub30_share_db: pow_db(band_sum(&f, &p, 1.0, 30.0) / total),
        sibilance: Sibilance { median_db: 0.0, p99_db: 0.0, max_db: 0.0 },
    }
}

/// Narrow peaks between `lo` and `hi` Hz with ≥ 6 dB prominence, labelled with note names.
/// Keeps the 12 most prominent, in frequency order.
pub fn tonal_peaks(f: &[f64], p: &[f64], lo: f64, hi: f64) -> Vec<LfPeak> {
    let idx: Vec<usize> = (0..f.len()).filter(|&i| f[i] > lo && f[i] < hi).collect();
    if idx.len() < 3 {
        return vec![];
    }
    let d: Vec<f64> = idx.iter().map(|&i| pow_db(p[i])).collect();
    let med = dsp::percentile(&dsp::sorted(d.clone()), 50.0);
    let mut out = vec![];
    for k in 1..d.len() - 1 {
        if !(d[k] > d[k - 1] && d[k] >= d[k + 1]) {
            continue;
        }
        // Prominence: drop to the lowest point before reaching higher ground on each side.
        let mut lmin = d[k];
        let mut j = k;
        while j > 0 {
            j -= 1;
            if d[j] > d[k] {
                break;
            }
            lmin = lmin.min(d[j]);
        }
        let mut rmin = d[k];
        let mut j = k;
        while j + 1 < d.len() {
            j += 1;
            if d[j] > d[k] {
                break;
            }
            rmin = rmin.min(d[j]);
        }
        let prom = d[k] - lmin.max(rmin);
        if prom >= 6.0 {
            // Parabolic interpolation on the dB bins: sub-bin frequency, accurate enough for cents.
            let (a, b, c) = (d[k - 1], d[k], d[k + 1]);
            let den = a - 2.0 * b + c;
            let delta = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = f[idx[k]] + delta * (f[idx[k + 1]] - f[idx[k]]);
            let level = b - 0.25 * (a - c) * delta;
            out.push((prom, LfPeak { hz, above_median_db: level - med, note: dsp::note_name(hz) }));
        }
    }
    out.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
    out.truncate(12);
    let mut out: Vec<LfPeak> = out.into_iter().map(|(_, p)| p).collect();
    out.sort_by(|x, y| x.hz.partial_cmp(&y.hz).unwrap());
    out
}

fn sibilance(a: &Audio) -> Sibilance {
    let fs = a.sample_rate as f64;
    let mid = a.mid();
    let hs = Chain::bandpass(fs, 5000.0, (10000.0f64).min(fs * 0.45)).process(&mid);
    let n = (0.02 * fs) as usize;
    let mut e = vec![];
    let mut h = vec![];
    for (cm, ch) in mid.chunks_exact(n).zip(hs.chunks_exact(n)) {
        e.push(dsp::mean_square(cm).sqrt());
        h.push(dsp::mean_square(ch).sqrt());
    }
    // Only consider non-silent frames, then the louder half of those.
    let nonsilent: Vec<f64> = e.iter().copied().filter(|&v| amp_db(v) > SILENCE_DBFS).collect();
    let thr = dsp::percentile(&dsp::sorted(nonsilent), 50.0);
    let ratios: Vec<f64> = e
        .iter()
        .zip(&h)
        .filter(|(&ev, _)| thr.is_finite() && ev > thr)
        .map(|(&ev, &hv)| amp_db(hv) - amp_db(ev))
        .collect();
    let s = dsp::sorted(ratios);
    Sibilance {
        median_db: dsp::percentile(&s, 50.0),
        p99_db: dsp::percentile(&s, 99.0),
        max_db: s.last().copied().unwrap_or(f64::NAN),
    }
}

// ---------- edges, levels, dither ----------

struct Levels {
    peak: [f64; 2],
    gated_rms_db: f64,
    near_fs: u64,
    over_fs: u64,
    flat_runs: u64,
    over_fs_blocks: Vec<u32>,
    flat_top_blocks: Vec<u32>,
    dc: [f64; 2],
    balance_db: f64,
}

/// 100 ms frame RMS (dBFS, louder channel), reused for silence and section logic.
pub struct Frames {
    pub rms_db: Vec<f64>,
}

fn edges_and_levels(a: &Audio) -> (Edges, Levels, Option<Dither>, Frames) {
    let fs = a.sample_rate as f64;
    let n = a.len();
    let (l, r) = (&a.left, &a.right);

    let peak = [
        l.iter().fold(0f32, |m, &x| m.max(x.abs())) as f64,
        r.iter().fold(0f32, |m, &x| m.max(x.abs())) as f64,
    ];
    let pk = peak[0].max(peak[1]);
    let near = 10f64.powf(-0.1 / 20.0);
    let flat_thr = pk * 10f64.powf(-0.01 / 20.0);
    let (mut near_fs, mut over_fs, mut flat_runs) = (0u64, 0u64, 0u64);
    // 100 ms blocks, matching `KBLOCK_S`, so clipping can be located later.
    let hop = (fs * 0.1) as usize;
    let (mut over_blocks, mut flat_blocks) = (vec![], vec![]);
    for ch in [l, r] {
        let mut run = 0u32;
        for (i, &x) in ch.iter().enumerate() {
            let ax = x.abs() as f64;
            if ax >= near {
                near_fs += 1;
            }
            if ax > 1.0 {
                over_fs += 1;
                let b = (i / hop) as u32;
                if over_blocks.last() != Some(&b) {
                    over_blocks.push(b);
                }
            }
            if pk > 0.0 && ax >= flat_thr {
                run += 1;
                if run == 3 {
                    flat_runs += 1;
                    flat_blocks.push((i / hop) as u32);
                }
            } else {
                run = 0;
            }
        }
    }
    let dc = [
        l.iter().map(|&x| x as f64).sum::<f64>() / n as f64,
        r.iter().map(|&x| x as f64).sum::<f64>() / n as f64,
    ];
    let (msl, msr) = (dsp::mean_square(l), dsp::mean_square(r));

    // 100 ms frames.
    let mut rms_db = vec![];
    let mut gated = (0.0f64, 0usize);
    for (cl, cr) in l.chunks(hop).zip(r.chunks(hop)) {
        let (a1, b1) = (dsp::mean_square(cl), dsp::mean_square(cr));
        let db = pow_db(a1.max(b1));
        rms_db.push(db);
        if db > SILENCE_DBFS {
            gated.0 += 0.5 * (a1 + b1);
            gated.1 += 1;
        }
    }
    let gated_rms_db = if gated.1 > 0 { pow_db(gated.0 / gated.1 as f64) } else { -120.0 };

    // First/last sample above -60 dBFS.
    let thr = 10f32.powf(-60.0 / 20.0);
    let first = (0..n).find(|&i| l[i].abs() > thr || r[i].abs() > thr).unwrap_or(0);
    let last = (0..n).rev().find(|&i| l[i].abs() > thr || r[i].abs() > thr).unwrap_or(n.saturating_sub(1));
    let tail_n = ((0.05 * fs) as usize).min(n);
    let tail_pk = l[n - tail_n..].iter().chain(&r[n - tail_n..]).fold(0f32, |m, &x| m.max(x.abs())) as f64;
    let tail_db = amp_db(tail_pk);
    let ends_on_silence = tail_db < -80.0;

    // Internal digital silence (> 2 s below -70 dBFS) between first and last sound.
    let mut internal = vec![];
    let f0 = first / hop;
    let f1 = (last / hop).min(rms_db.len().saturating_sub(1));
    let mut run_start: Option<usize> = None;
    for i in f0..=f1 {
        let silent = rms_db[i] < -70.0;
        match (silent, run_start) {
            (true, None) => run_start = Some(i),
            (false, Some(s)) => {
                if (i - s) as f64 * 0.1 >= 2.0 {
                    internal.push(TimeRange { start_s: s as f64 * 0.1, end_s: i as f64 * 0.1 });
                }
                run_start = None;
            }
            _ => {}
        }
    }

    let dither = dither_check(a, &rms_db, hop);

    let edges = Edges {
        first_sound_s: first as f64 / fs,
        last_sound_s: last as f64 / fs,
        leading_silence_s: first as f64 / fs,
        trailing_silence_s: (n - 1 - last) as f64 / fs,
        tail_last50ms_dbfs: tail_db,
        ends_on_silence,
        tail_truncated: !ends_on_silence,
        fade_out_detected: false,
        internal_silences: internal,
    };
    let levels = Levels {
        peak,
        gated_rms_db,
        near_fs,
        over_fs,
        flat_runs,
        over_fs_blocks: block_list(over_blocks),
        flat_top_blocks: block_list(flat_blocks),
        dc,
        balance_db: pow_db(msl) - pow_db(msr),
    };
    (edges, levels, dither, Frames { rms_db })
}

/// Sorted, de-duplicated block indices, capped.
fn block_list(mut v: Vec<u32>) -> Vec<u32> {
    v.sort_unstable();
    v.dedup();
    v.truncate(MAX_CLIP_BLOCKS);
    v
}

fn dither_check(a: &Audio, rms_db: &[f64], hop: usize) -> Option<Dither> {
    let bits = a.info.bits_per_sample?;
    if !a.info.sample_format.starts_with("int") || bits > 24 {
        return None;
    }
    let scale = 2f64.powi(bits as i32 - 1);
    // Only look at frames within ~12 dB of the LSB: there, anything non-zero is noise, not music.
    let floor_db = -6.02 * bits as f64 + 12.0;
    let mut codes = std::collections::HashMap::<i64, u64>::new();
    let (mut checked, mut nonzero, mut ss) = (0u64, 0u64, 0.0f64);
    for (fi, &db) in rms_db.iter().enumerate() {
        if db > floor_db {
            continue;
        }
        let s = fi * hop;
        let e = (s + hop).min(a.len());
        for ch in [&a.left, &a.right] {
            for &x in &ch[s..e] {
                let c = (x as f64 * scale).round() as i64;
                *codes.entry(c).or_default() += 1;
                checked += 1;
                if c != 0 {
                    nonzero += 1;
                }
                ss += (x as f64) * (x as f64);
            }
        }
        if checked > 2_000_000 {
            break;
        }
    }
    if checked < 4800 {
        return Some(Dither {
            bits,
            samples_checked: 0,
            distinct_codes: 0,
            nonzero_fraction: 0.0,
            level_dbfs: -120.0,
            detected: false,
            note: "No quiet passage to check.".into(),
        });
    }
    let nz = nonzero as f64 / checked as f64;
    let max_code = codes.keys().map(|c| c.abs()).max().unwrap_or(0);
    let detected = nz > 0.2 && max_code <= 8 && codes.len() >= 3;
    let note = if detected {
        "Low-level noise toggling the last bits in silent passages: dither is present.".to_string()
    } else if nz < 0.01 {
        "Quiet passages are pure digital zero: either no dither or the exporter blanks silence.".to_string()
    } else {
        "Inconclusive: quiet passages contain real signal.".to_string()
    };
    Some(Dither {
        bits,
        samples_checked: checked,
        distinct_codes: codes.len() as u32,
        nonzero_fraction: nz,
        level_dbfs: pow_db(ss / checked as f64),
        detected,
        note,
    })
}

// ---------- structure ----------

fn st_time(i: usize) -> f64 {
    // Short-term value i is measured at the end of a 3 s window ending at (i+1)*hop; centre it.
    ((i + 1) as f64 * ST_HOP_S - 1.5).max(0.0)
}

fn detect_fade(st: &[f32], last_sound_s: f64) -> bool {
    let idx = |t: f64| ((t / ST_HOP_S).round() as isize - 1).max(0) as usize;
    let end_i = idx(last_sound_s - 1.0).min(st.len().saturating_sub(1));
    let (a, b) = (idx(last_sound_s - 25.0), idx(last_sound_s - 12.0));
    if b <= a || b >= st.len() {
        return false;
    }
    let reference = dsp::percentile(&dsp::sorted(st[a..b].iter().map(|&v| v as f64).collect()), 50.0);
    reference > -45.0 && (st[end_i] as f64) < reference - 15.0
}

fn sections(st: &[f32], integrated: f64, kblocks: &[f32], duration: f64, _frames: &Frames) -> Vec<Section> {
    if st.is_empty() {
        return vec![];
    }
    let classify = |v: f32| {
        if (v as f64) < -60.0 {
            SectionKind::Silence
        } else if (v as f64) >= integrated - 4.0 {
            SectionKind::Loud
        } else {
            SectionKind::Quiet
        }
    };
    let raw: Vec<SectionKind> = st.iter().map(|&v| classify(v)).collect();
    // Majority smoothing over 5 points (2.5 s).
    let sm: Vec<SectionKind> = (0..raw.len())
        .map(|i| {
            let lo = i.saturating_sub(2);
            let hi = (i + 3).min(raw.len());
            let w = &raw[lo..hi];
            let count = |k: &SectionKind| w.iter().filter(|x| *x == k).count();
            [SectionKind::Loud, SectionKind::Quiet, SectionKind::Silence]
                .into_iter()
                .max_by_key(|k| (count(k), *k == raw[i]))
                .unwrap()
        })
        .collect();
    // Runs.
    let mut runs: Vec<(usize, usize, SectionKind)> = vec![];
    for (i, k) in sm.into_iter().enumerate() {
        match runs.last_mut() {
            Some(r) if r.2 == k => r.1 = i + 1,
            _ => runs.push((i, i + 1, k)),
        }
    }
    // Merge runs shorter than 4 s into the previous run.
    let min_len = (4.0 / ST_HOP_S) as usize;
    let mut merged: Vec<(usize, usize, SectionKind)> = vec![];
    for r in runs {
        if r.1 - r.0 < min_len && !merged.is_empty() && r.2 != SectionKind::Silence {
            merged.last_mut().unwrap().1 = r.1;
        } else if let Some(last) = merged.last_mut().filter(|l| l.2 == r.2) {
            last.1 = r.1;
        } else {
            merged.push(r);
        }
    }
    merged
        .into_iter()
        .map(|(a, b, kind)| {
            let start_s = if a == 0 { 0.0 } else { st_time(a) };
            let end_s = if b >= st.len() { duration } else { st_time(b) };
            Section { start_s, end_s, lufs: range_loudness(kblocks, start_s, end_s), kind }
        })
        .collect()
}
