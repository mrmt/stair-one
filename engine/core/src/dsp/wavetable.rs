//! OscillatorNode の帯域制限波形。Chromium の PeriodicWave と同じ作り方:
//! 1/3 オクターブごとに倍音数を減らしたテーブルを逆 FFT で作り、基音に応じて隣り合う2枚を混ぜる

use super::PI;
use crate::patches::Wave;
use libm::{cos, log2, pow, sin};

const BANDS_PER_OCTAVE: f64 = 3.0;
const CENTS_PER_RANGE: f64 = 1200.0 / BANDS_PER_OCTAVE;

pub struct Wavetables {
    n: usize,
    ranges: usize,
    lowest: f64,
    saw: Vec<f64>,
    square: Vec<f64>,
}

/// 複素 FFT (基数2、その場)。inverse = true で逆変換 (1/n はかけない)
fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * 2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = (cos(ang * k as f64), sin(ang * k as f64));
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
}

impl Wavetables {
    pub fn new(sr: f64) -> Self {
        let n = if sr <= 24000.0 { 2048 } else if sr <= 88200.0 { 4096 } else { 16384 };
        let ranges = (BANDS_PER_OCTAVE * log2(n as f64)).round() as usize;
        let half = n / 2;
        let build = |coef: &dyn Fn(usize) -> f64| -> Vec<f64> {
            let mut out = vec![0.0; ranges * n];
            let mut scale = 1.0;
            for r in 0..ranges {
                // 範囲 r では上から r × 400 cent 分の倍音を削る
                let partials = (pow(2.0, -(r as f64) * CENTS_PER_RANGE / 1200.0) * half as f64) as usize;
                let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
                // x(t) = Σ b_k sin(kωt)。sin 成分は X[k] = -i n b_k / 2 と共役
                for k in 1..half.min(partials + 1) {
                    let b = coef(k);
                    im[k] = -b * 0.5;
                    im[n - k] = b * 0.5;
                }
                fft(&mut re, &mut im, true);
                // 倍音が一番多い最初の表で最大値を 1 にし、その倍率を全範囲に使う
                if r == 0 {
                    let m = re.iter().fold(0.0f64, |a, v| a.max(v.abs()));
                    if m > 0.0 {
                        scale = 1.0 / m;
                    }
                }
                for (o, v) in out[r * n..(r + 1) * n].iter_mut().zip(&re) {
                    *o = v * scale;
                }
            }
            out
        };
        let saw = build(&|k| (if k % 2 == 1 { 1.0 } else { -1.0 }) * 2.0 / (PI * k as f64));
        let square = build(&|k| if k % 2 == 1 { 4.0 / (PI * k as f64) } else { 0.0 });
        Wavetables { n, ranges, lowest: sr / n as f64, saw, square }
    }

    /// phase は 0..1、freq は今の基音 (Hz)
    #[inline]
    pub fn sample(&self, wave: Wave, phase: f64, freq: f64) -> f64 {
        let table = match wave {
            Wave::Sine => return sin(2.0 * PI * phase),
            Wave::Saw => &self.saw,
            Wave::Square => &self.square,
        };
        let ratio = if freq > 0.0 { freq / self.lowest } else { 0.5 };
        // 1つ上の範囲に丸めて、折り返す前に倍音を落とす
        let pr = (1.0 + log2(ratio) * 1200.0 / CENTS_PER_RANGE).clamp(0.0, (self.ranges - 1) as f64);
        let r1 = pr as usize;
        let r2 = if r1 < self.ranges - 1 { r1 + 1 } else { r1 };
        let f = pr - r1 as f64;
        let x = phase * self.n as f64;
        let i = (x as usize) % self.n;
        let j = (i + 1) % self.n;
        let t = x - libm::floor(x);
        let hi = &table[r1 * self.n..];
        let lo = &table[r2 * self.n..];
        let a = hi[i] + (hi[j] - hi[i]) * t;
        let b = lo[i] + (lo[j] - lo[i]) * t;
        a * (1.0 - f) + b * f
    }
}
