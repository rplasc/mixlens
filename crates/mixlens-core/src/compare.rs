//! Version-to-version comparison: alignment, null test, and per-section deltas.

use crate::analyze::{octave_shares, range_loudness, OCTAVE_BANDS};
use crate::audio::{from_planar, Audio};
use crate::dsp::{self, pow_db};
use crate::report::{Report, SectionKind};
use crate::{Error, Result};
use realfft::RealFftPlanner;
use rubato::{FftFixedIn, Resampler};
use serde::{Deserialize, Serialize};

const WINDOW_S: f64 = 2.0;
const CHANGED_DB: f64 = -20.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comparison {
    pub a_file: String,
    pub b_file: String,
    /// b was resampled to a's rate before comparing.
    pub resampled: bool,
    /// b is offset by this much relative to a (positive = b starts later).
    pub lag_ms: f64,
    /// Least-squares gain of b relative to a.
    pub gain_db: f64,
    /// Residual after level matching, relative to a (dB). Below about -60 dB means effectively identical.
    pub residual_rel_db: f64,
    pub residual_dbfs: f64,
    pub window_s: f64,
    /// Per-window difference b - a relative to a's level (dB, no level matching). -inf-like values mean identical.
    pub windows: Vec<f32>,
    pub changed_regions: Vec<ChangedRegion>,
    pub section_deltas: Vec<SectionDelta>,
    pub metric_deltas: Vec<MetricDelta>,
    pub third_octave_delta: Vec<BandDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangedRegion {
    pub start_s: f64,
    pub end_s: f64,
    pub mean_diff_rel_db: f64,
    pub lufs_a: f64,
    pub lufs_b: f64,
    /// b minus a energy per octave band (dB), bands as in `octave_bands`.
    pub octave_delta_db: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionDelta {
    pub start_s: f64,
    pub end_s: f64,
    pub kind: SectionKind,
    pub lufs_a: f64,
    pub lufs_b: f64,
    pub delta_lu: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricDelta {
    pub name: String,
    pub a: f64,
    pub b: f64,
    pub delta: f64,
    pub unit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BandDelta {
    pub fc: f64,
    pub dev_a: f64,
    pub dev_b: f64,
}

pub fn octave_band_labels() -> Vec<String> {
    OCTAVE_BANDS.iter().map(|(lo, hi)| format!("{}-{}", *lo as i64, *hi as i64)).collect()
}

/// High-quality resample of both channels.
pub fn resample(a: &Audio, to: u32) -> Result<Audio> {
    if a.sample_rate == to {
        return Ok(a.clone());
    }
    let chunk = 8192;
    let mut rs = FftFixedIn::<f32>::new(a.sample_rate as usize, to as usize, chunk, 2, 2)
        .map_err(|e| Error::Analysis(format!("resampler: {e}")))?;
    let delay = rs.output_delay();
    let n = a.len();
    let (mut ol, mut or) = (Vec::with_capacity(n), Vec::with_capacity(n));
    let mut pos = 0;
    let mut push = |rs: &mut FftFixedIn<f32>, l: Vec<f32>, r: Vec<f32>| -> Result<()> {
        let out = rs.process(&[l, r], None).map_err(|e| Error::Analysis(format!("resample: {e}")))?;
        ol.extend_from_slice(&out[0]);
        or.extend_from_slice(&out[1]);
        Ok(())
    };
    while pos < n {
        let need = rs.input_frames_next();
        let end = (pos + need).min(n);
        let mut l = a.left[pos..end].to_vec();
        let mut r = a.right[pos..end].to_vec();
        l.resize(need, 0.0);
        r.resize(need, 0.0);
        push(&mut rs, l, r)?;
        pos = end;
    }
    let need = rs.input_frames_next();
    push(&mut rs, vec![0.0; need], vec![0.0; need])?;
    let expected = (n as f64 * to as f64 / a.sample_rate as f64).round() as usize;
    let s = delay.min(ol.len());
    let e = (delay + expected).min(ol.len());
    let mut out = from_planar(to, ol[s..e].to_vec(), or[s..e].to_vec(), &a.info.file_name);
    out.info = a.info.clone();
    out.info.sample_rate = to;
    Ok(out)
}

/// Lag (in samples) such that b[i + lag] lines up with a[i].
fn find_lag(a: &[f32], b: &[f32], center: usize, seg: usize, max_lag: usize) -> isize {
    let s = center.saturating_sub(seg / 2).min(a.len().saturating_sub(seg));
    let seg = seg.min(a.len() - s);
    let bs = s.saturating_sub(max_lag);
    let be = (s + seg + max_lag).min(b.len());
    if be <= bs || seg == 0 {
        return 0;
    }
    let aseg = &a[s..s + seg];
    let bwin = &b[bs..be];
    let n = (aseg.len() + bwin.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let mut xa = fwd.make_input_vec();
    let mut xb = fwd.make_input_vec();
    for (i, &v) in aseg.iter().enumerate() {
        xa[i] = v as f64;
    }
    for (i, &v) in bwin.iter().enumerate() {
        xb[i] = v as f64;
    }
    let mut fa = fwd.make_output_vec();
    let mut fb = fwd.make_output_vec();
    fwd.process(&mut xa, &mut fa).expect("fft");
    fwd.process(&mut xb, &mut fb).expect("fft");
    let mut prod: Vec<_> = fa.iter().zip(&fb).map(|(x, y)| x.conj() * y).collect();
    // DC and Nyquist bins must be real for the inverse transform.
    prod[0].im = 0.0;
    if let Some(last) = prod.last_mut() {
        last.im = 0.0;
    }
    let mut c = inv.make_output_vec();
    inv.process(&mut prod, &mut c).expect("ifft");
    let kmax = bwin.len().saturating_sub(aseg.len());
    let best = (0..=kmax).max_by(|&i, &j| c[i].partial_cmp(&c[j]).unwrap()).unwrap_or(0);
    best as isize - (s - bs) as isize
}

fn region_octaves(x: &[f32], fs: f64) -> Vec<f64> {
    if x.len() < 1024 {
        return vec![f64::NAN; OCTAVE_BANDS.len()];
    }
    let (f, p) = dsp::welch(x, fs, 16384);
    OCTAVE_BANDS
        .iter()
        .map(|&(lo, hi)| pow_db(f.iter().zip(&p).filter(|(&fr, _)| fr >= lo && fr < hi).map(|(_, &v)| v).sum::<f64>()))
        .collect()
}

pub fn compare(a: &Audio, ra: &Report, b_in: &Audio, rb: &Report) -> Result<Comparison> {
    let resampled = a.sample_rate != b_in.sample_rate;
    let b = if resampled { resample(b_in, a.sample_rate)? } else { b_in.clone() };
    let fs = a.sample_rate as f64;
    let am = a.mid();
    let bm = b.mid();

    // Align on the loudest 10 s of a.
    let hop = ra.loudness.short_term_hop_s;
    let loudest = ra
        .loudness
        .short_term
        .iter()
        .enumerate()
        .max_by(|x, y| x.1.partial_cmp(y.1).unwrap())
        .map(|(i, _)| ((i + 1) as f64 * hop - 1.5).max(0.0))
        .unwrap_or(0.0);
    let lag = find_lag(&am, &bm, (loudest * fs) as usize, (10.0 * fs) as usize, (2.0 * fs) as usize);

    let idx_b = |i: usize| -> Option<usize> {
        let j = i as isize + lag;
        if j >= 0 && (j as usize) < b.len() {
            Some(j as usize)
        } else {
            None
        }
    };

    // Gain match and residual over the overlap.
    let (mut sab, mut saa, mut sbb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..a.len() {
        if let Some(j) = idx_b(i) {
            for (x, y) in [(a.left[i], b.left[j]), (a.right[i], b.right[j])] {
                sab += x as f64 * y as f64;
                saa += x as f64 * x as f64;
                sbb += y as f64 * y as f64;
            }
        }
    }
    let g = if saa > 0.0 { sab / saa } else { 1.0 };
    // ||b - g a||^2 = sbb - 2 g sab + g^2 saa
    let res = (sbb - 2.0 * g * sab + g * g * saa).max(0.0);
    let overlap_n = (0..a.len()).filter(|&i| idx_b(i).is_some()).count().max(1) as f64 * 2.0;
    let residual_rel_db = pow_db(res) - pow_db(saa);
    let residual_dbfs = pow_db(res / overlap_n);

    // Per-window difference without level matching (level changes are real changes).
    let w = (WINDOW_S * fs) as usize;
    let mut windows = vec![];
    let mut i0 = 0;
    while i0 < a.len() {
        let i1 = (i0 + w).min(a.len());
        let (mut ea, mut ed, mut eb) = (0.0f64, 0.0f64, 0.0f64);
        for i in i0..i1 {
            let (xl, xr) = (a.left[i] as f64, a.right[i] as f64);
            let (yl, yr) = match idx_b(i) {
                Some(j) => (b.left[j] as f64, b.right[j] as f64),
                None => (0.0, 0.0),
            };
            ea += xl * xl + xr * xr;
            eb += yl * yl + yr * yr;
            ed += (yl - xl).powi(2) + (yr - xr).powi(2);
        }
        let n = ((i1 - i0) * 2) as f64;
        let v = if pow_db(ea / n) < -70.0 && pow_db(eb / n) < -70.0 {
            -120.0
        } else if pow_db(ea / n) < -70.0 {
            20.0
        } else {
            (pow_db(ed) - pow_db(ea)).min(20.0)
        };
        windows.push(v as f32);
        i0 = i1;
    }

    // Changed regions: contiguous windows above the threshold (1-window gaps bridged).
    let mut regions: Vec<(usize, usize)> = vec![];
    for (k, &v) in windows.iter().enumerate() {
        if (v as f64) > CHANGED_DB {
            match regions.last_mut() {
                Some(r) if k <= r.1 + 1 => r.1 = k + 1,
                _ => regions.push((k, k + 1)),
            }
        }
    }
    regions.sort_by_key(|r| std::cmp::Reverse(r.1 - r.0));
    regions.truncate(8);
    regions.sort_by_key(|r| r.0);
    let lag_s = lag as f64 / fs;
    let changed_regions = regions
        .iter()
        .map(|&(k0, k1)| {
            let start_s = k0 as f64 * WINDOW_S;
            let end_s = (k1 as f64 * WINDOW_S).min(a.len() as f64 / fs);
            let mean = windows[k0..k1].iter().map(|&v| v as f64).sum::<f64>() / (k1 - k0) as f64;
            let ia = (start_s * fs) as usize..((end_s * fs) as usize).min(am.len());
            let jb0 = ((start_s + lag_s) * fs).max(0.0) as usize;
            let jb1 = (((end_s + lag_s) * fs) as usize).min(bm.len());
            let oa = region_octaves(&am[ia], fs);
            let ob = if jb1 > jb0 { region_octaves(&bm[jb0..jb1], fs) } else { vec![f64::NAN; OCTAVE_BANDS.len()] };
            ChangedRegion {
                start_s,
                end_s,
                mean_diff_rel_db: mean,
                lufs_a: range_loudness(&ra.kblocks, start_s, end_s),
                lufs_b: range_loudness(&rb.kblocks, start_s + lag_s, end_s + lag_s),
                octave_delta_db: ob.iter().zip(&oa).map(|(x, y)| x - y).collect(),
            }
        })
        .collect();

    let section_deltas = ra
        .sections
        .iter()
        .map(|s| {
            let lb = range_loudness(&rb.kblocks, s.start_s + lag_s, s.end_s + lag_s);
            SectionDelta { start_s: s.start_s, end_s: s.end_s, kind: s.kind.clone(), lufs_a: s.lufs, lufs_b: lb, delta_lu: lb - s.lufs }
        })
        .collect();

    let (la, lb) = (&ra.loudness, &rb.loudness);
    let md = |name: &str, a: f64, b: f64, unit: &str| MetricDelta { name: name.into(), a, b, delta: b - a, unit: unit.into() };
    let metric_deltas = vec![
        md("integrated", la.integrated_lufs, lb.integrated_lufs, "LUFS"),
        md("true_peak", la.true_peak_dbtp, lb.true_peak_dbtp, "dBTP"),
        md("lra", la.lra_lu, lb.lra_lu, "LU"),
        md("plr", la.plr_db, lb.plr_db, "dB"),
        md("crest", la.crest_db, lb.crest_db, "dB"),
        md("correlation", ra.stereo.correlation, rb.stereo.correlation, ""),
        md("low_correlation", ra.stereo.low_correlation, rb.stereo.low_correlation, ""),
        md("mono_loss", ra.stereo.mono_loss_lu, rb.stereo.mono_loss_lu, "LU"),
        md("duration", ra.file.duration_s, rb.file.duration_s, "s"),
        md("trailing_silence", ra.edges.trailing_silence_s, rb.edges.trailing_silence_s, "s"),
        md("tilt", ra.spectrum.tilt_db_per_oct, rb.spectrum.tilt_db_per_oct, "dB/oct"),
        md("sibilance_p99", ra.spectrum.sibilance.p99_db, rb.spectrum.sibilance.p99_db, "dB"),
    ];
    let third_octave_delta = ra
        .spectrum
        .third_octave
        .iter()
        .filter_map(|x| rb.spectrum.third_octave.iter().find(|y| (y.fc - x.fc).abs() < 0.5).map(|y| BandDelta { fc: x.fc, dev_a: x.dev_db, dev_b: y.dev_db }))
        .collect();

    // Keep `octave_shares` linked for callers wanting whole-song shares.
    let _ = octave_shares;

    Ok(Comparison {
        a_file: a.info.file_name.clone(),
        b_file: b_in.info.file_name.clone(),
        resampled,
        lag_ms: lag_s * 1000.0,
        gain_db: 20.0 * g.abs().max(1e-12).log10(),
        residual_rel_db,
        residual_dbfs,
        window_s: WINDOW_S,
        windows,
        changed_regions,
        section_deltas,
        metric_deltas,
        third_octave_delta,
    })
}

impl Comparison {
    pub fn to_ai_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            // Summarize the window array as runs to save tokens.
            o.remove("windows");
            o.insert("octave_bands".into(), serde_json::json!(octave_band_labels()));
        }
        crate::report::round_floats(&mut v);
        v
    }
}
