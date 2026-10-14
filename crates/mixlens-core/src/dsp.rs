//! Small, allocation-light DSP building blocks.

use realfft::RealFftPlanner;
use std::f64::consts::PI;

/// Convert a linear amplitude to dB, floored so silence stays finite.
#[inline]
pub fn amp_db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// Convert a power (mean square) to dB, floored.
#[inline]
pub fn pow_db(x: f64) -> f64 {
    10.0 * x.max(1e-24).log10()
}

/// Direct-form-I biquad with f64 state.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    pub fn new(b: [f64; 3], a: [f64; 3]) -> Self {
        let a0 = a[0];
        Self {
            b0: b[0] / a0,
            b1: b[1] / a0,
            b2: b[2] / a0,
            a1: a[1] / a0,
            a2: a[2] / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// RBJ low-pass.
    pub fn lowpass(fs: f64, f0: f64, q: f64) -> Self {
        let w0 = 2.0 * PI * f0 / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        Self::new(
            [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0],
            [1.0 + alpha, -2.0 * c, 1.0 - alpha],
        )
    }

    /// RBJ high-pass.
    pub fn highpass(fs: f64, f0: f64, q: f64) -> Self {
        let w0 = 2.0 * PI * f0 / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        Self::new(
            [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
            [1.0 + alpha, -2.0 * c, 1.0 - alpha],
        )
    }

    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Q values for a 4th-order Butterworth built from two biquads.
const BW4_Q: [f64; 2] = [0.541_196_100_146_197, 1.306_562_964_876_376_6];

/// A cascade of biquads.
#[derive(Clone, Debug)]
pub struct Chain(pub Vec<Biquad>);

impl Chain {
    pub fn butter4_lowpass(fs: f64, f0: f64) -> Self {
        Chain(BW4_Q.iter().map(|&q| Biquad::lowpass(fs, f0, q)).collect())
    }
    pub fn butter4_highpass(fs: f64, f0: f64) -> Self {
        Chain(BW4_Q.iter().map(|&q| Biquad::highpass(fs, f0, q)).collect())
    }
    /// 4th-order Butterworth band-pass approximated as HP(lo) + LP(hi), each 4th order.
    pub fn bandpass(fs: f64, lo: f64, hi: f64) -> Self {
        let mut v = Self::butter4_highpass(fs, lo).0;
        v.extend(Self::butter4_lowpass(fs, hi).0);
        Chain(v)
    }
    /// ITU-R BS.1770 K-weighting (pre-filter + RLB), coefficients derived for any sample rate.
    pub fn k_weighting(fs: f64) -> Self {
        let f0 = 1681.974450955533;
        let g = 3.999843853973347;
        let q = 0.7071752369554196;
        let k = (PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        let pre = Biquad::new(
            [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        let f0 = 38.13547087602444;
        let q = 0.5003270373238773;
        let k = (PI * f0 / fs).tan();
        let d = 1.0 + k / q + k * k;
        let rlb = Biquad::new([1.0, -2.0, 1.0], [1.0, 2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d]);
        Chain(vec![pre, rlb])
    }

    #[inline]
    pub fn tick(&mut self, mut x: f64) -> f64 {
        for b in self.0.iter_mut() {
            x = b.tick(x);
        }
        x
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        input.iter().map(|&x| self.tick(x as f64) as f32).collect()
    }
}

/// Welch power spectral density (Hann window, 50% overlap, one-sided density scaling like SciPy).
/// Returns (frequencies, psd).
pub fn welch(x: &[f32], fs: f64, nperseg: usize) -> (Vec<f64>, Vec<f64>) {
    let n = nperseg.min(x.len().next_power_of_two()).max(256);
    let n = if x.len() < n { x.len().max(2) } else { n };
    let hop = n / 2;
    let win: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()).collect();
    let wss: f64 = win.iter().map(|w| w * w).sum();
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut inbuf = fft.make_input_vec();
    let mut spec = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let bins = n / 2 + 1;
    let mut acc = vec![0.0f64; bins];
    let mut count = 0usize;
    let mut start = 0usize;
    while start + n <= x.len() {
        let seg = &x[start..start + n];
        let mean: f64 = seg.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
        for i in 0..n {
            inbuf[i] = (seg[i] as f64 - mean) * win[i];
        }
        fft.process_with_scratch(&mut inbuf, &mut spec, &mut scratch).expect("fft");
        for (a, c) in acc.iter_mut().zip(spec.iter()) {
            *a += c.norm_sqr();
        }
        count += 1;
        start += hop;
    }
    let scale = 1.0 / (fs * wss * count.max(1) as f64);
    let freqs: Vec<f64> = (0..bins).map(|i| i as f64 * fs / n as f64).collect();
    let psd: Vec<f64> = acc
        .iter()
        .enumerate()
        .map(|(i, &a)| {
            let one_sided = if i == 0 || (n % 2 == 0 && i == bins - 1) { 1.0 } else { 2.0 };
            a * scale * one_sided
        })
        .collect();
    (freqs, psd)
}

/// Pearson correlation of two equal-length slices.
pub fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let (mut sa, mut sb) = (0.0f64, 0.0f64);
    for i in 0..n {
        sa += a[i] as f64;
        sb += b[i] as f64;
    }
    let (ma, mb) = (sa / n as f64, sb / n as f64);
    let (mut sab, mut saa, mut sbb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let da = a[i] as f64 - ma;
        let db = b[i] as f64 - mb;
        sab += da * db;
        saa += da * da;
        sbb += db * db;
    }
    if saa <= 0.0 || sbb <= 0.0 {
        return 0.0;
    }
    sab / (saa.sqrt() * sbb.sqrt())
}

pub fn mean_square(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = (p / 100.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

pub fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.retain(|x| x.is_finite());
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// Least-squares line fit, returns (slope, intercept).
pub fn linfit(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (slope, my - slope * mx)
}

/// Note name for a frequency, e.g. 61.7 Hz -> "B1".
pub fn note_name(hz: f64) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let midi = (69.0 + 12.0 * (hz / 440.0).log2()).round() as i32;
    let name = NAMES[midi.rem_euclid(12) as usize];
    let octave = midi / 12 - 1;
    format!("{name}{octave}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes() {
        assert_eq!(note_name(440.0), "A4");
        assert_eq!(note_name(61.5), "B1");
        assert_eq!(note_name(82.0), "E2");
    }

    #[test]
    fn corr_identity() {
        let a: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        assert!((correlation(&a, &a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn lowpass_attenuates() {
        let fs = 48000.0;
        let mut f = Chain::butter4_lowpass(fs, 120.0);
        let x: Vec<f32> = (0..48000).map(|i| (2.0 * std::f32::consts::PI * 2000.0 * i as f32 / 48000.0).sin()).collect();
        let y = f.process(&x);
        let ms = mean_square(&y[24000..]);
        assert!(pow_db(ms) < -80.0);
    }
}
