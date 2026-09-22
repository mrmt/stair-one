//! WaveShaperNode。曲線の線形補間は Web Audio 仕様の式、oversample '4x' は 2倍 FIR を2段重ねる
//! 4x は Chromium の UpSampler / DownSampler と同じ窓付き sinc (128 / 256 タップ) で、192 サンプル遅れる。
//! Web Audio はこの遅れを補償せず、旧版は dry と混ぜているので、遅れが櫛形フィルタとして音色に効いている

use super::PI;
use libm::{cos, sin, tanh};

/// 仕様の曲線参照。範囲外は端の値
#[inline]
pub fn lookup(curve: &[f64], x: f64) -> f64 {
    let n = curve.len();
    let v = (n - 1) as f64 * 0.5 * (x + 1.0);
    if v <= 0.0 {
        return curve[0];
    }
    if v >= (n - 1) as f64 {
        return curve[n - 1];
    }
    let k = v as usize;
    let f = v - k as f64;
    curve[k] + (curve[k + 1] - curve[k]) * f
}

pub const DRIVE_LEN: usize = 2048;

/// ディストーションの曲線 (旧 driveCurve)
pub fn drive_curve(a: f64, c: &mut [f64; DRIVE_LEN]) {
    let k = 1.0 + a * a * 60.0;
    let b = 0.15 * a;
    let off = tanh(k * b);
    let norm = tanh(k);
    for (i, y) in c.iter_mut().enumerate() {
        let x = i as f64 / (DRIVE_LEN - 1) as f64 * 2.0 - 1.0;
        // Float32Array に入れていたので f32 に丸める
        *y = ((tanh(k * (x + b)) - off) / norm) as f32 as f64;
    }
}

/// Blackman 窓 (alpha = 0.16)
fn blackman(x: f64) -> f64 {
    0.42 - 0.5 * cos(2.0 * PI * x) + 0.08 * cos(4.0 * PI * x)
}

/// 直近 N 個を連続した slice で読めるリング (2倍の長さに二重書き)
#[derive(Clone, Copy)]
struct Ring<const N: usize, const N2: usize> {
    buf: [f64; N2],
    w: usize,
}

impl<const N: usize, const N2: usize> Ring<N, N2> {
    fn new() -> Self {
        Ring { buf: [0.0; N2], w: 0 }
    }
    #[inline]
    fn push(&mut self, x: f64) {
        self.w = (self.w + 1) % N;
        self.buf[self.w] = x;
        self.buf[self.w + N] = x;
    }
    /// [0] が最新、[k] が k サンプル前
    #[inline]
    fn recent(&self) -> impl Iterator<Item = &f64> {
        self.buf[self.w + 1..self.w + N + 1].iter().rev()
    }
    #[inline]
    fn ago(&self, k: usize) -> f64 {
        self.buf[self.w + N - k]
    }
}

const UP: usize = 128;
const DOWN: usize = 256;

/// Chromium UpSampler: 偶数番目は入力を 64 サンプル遅らせたもの、奇数番目は半サンプルずれた sinc で補間
#[derive(Clone, Copy)]
struct Up2x {
    k: [f64; UP],
    x: Ring<UP, { UP * 2 }>,
}

impl Up2x {
    fn new() -> Self {
        let mut k = [0.0; UP];
        for (i, v) in k.iter_mut().enumerate() {
            let s = PI * (i as f64 - (UP / 2) as f64 + 0.5);
            let sinc = if s == 0.0 { 1.0 } else { sin(s) / s };
            *v = sinc * blackman((i as f64 + 0.5) / UP as f64);
        }
        Up2x { k, x: Ring::new() }
    }
    #[inline]
    fn process(&mut self, x: f64) -> [f64; 2] {
        self.x.push(x);
        let odd = self.k.iter().zip(self.x.recent()).map(|(a, b)| a * b).sum();
        [self.x.ago(UP / 2), odd]
    }
}

/// Chromium DownSampler: 256 タップのハーフバンド。0 でない奇数タップと中央 0.5 だけ計算する
#[derive(Clone, Copy)]
struct Down2x {
    k: [f64; DOWN / 2],
    x: Ring<{ DOWN + 2 }, { (DOWN + 2) * 2 }>,
}

impl Down2x {
    fn new() -> Self {
        let mut k = [0.0; DOWN / 2];
        for (j, v) in k.iter_mut().enumerate() {
            let i = (2 * j + 1) as f64;
            let s = 0.5 * PI * (i - (DOWN / 2) as f64);
            let sinc = 0.5 * if s == 0.0 { 1.0 } else { sin(s) / s };
            *v = sinc * blackman(i / DOWN as f64);
        }
        Down2x { k, x: Ring::new() }
    }
    #[inline]
    fn process(&mut self, xs: [f64; 2]) -> f64 {
        self.x.push(xs[0]);
        self.x.push(xs[1]);
        // 中心は偶数番目 (xs[0]) の 128 サンプル前。そこから奇数個ずれた位置にだけ 0 でないタップがある
        let mut y = 0.0;
        for (j, k) in self.k.iter().enumerate() {
            y += k * self.x.ago(2 * j + 2);
        }
        y + 0.5 * self.x.ago(DOWN / 2 + 1)
    }
}

/// Chromium の 4x (UpSampler 64 + DownSampler 64 + 2段目 (64 + 64) / 2)
pub const LATENCY_4X: usize = 192;

/// 1チャンネル分の 4倍オーバーサンプリング波形整形
#[derive(Clone, Copy)]
pub struct Oversampled4x {
    up1: Up2x,
    up2: Up2x,
    down2: Down2x,
    down1: Down2x,
}

impl Default for Oversampled4x {
    fn default() -> Self {
        Oversampled4x { up1: Up2x::new(), up2: Up2x::new(), down2: Down2x::new(), down1: Down2x::new() }
    }
}

impl Oversampled4x {
    pub fn process(&mut self, x: f64, curve: &[f64]) -> f64 {
        let a = self.up1.process(x);
        let mut mid = [0.0; 2];
        for (i, &xa) in a.iter().enumerate() {
            let b = self.up2.process(xa);
            mid[i] = self.down2.process([lookup(curve, b[0]), lookup(curve, b[1])]);
        }
        self.down1.process(mid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 四倍オーバーサンプリングの遅れはchromiumと同じ192サンプル() {
        let identity = [-1.0, 1.0];
        let mut os = Oversampled4x::default();
        let out: Vec<f64> = (0..400).map(|i| os.process(if i == 0 { 0.5 } else { 0.0 }, &identity)).collect();
        let peak = out.iter().enumerate().max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap()).unwrap();
        assert_eq!(peak.0, LATENCY_4X);
        // Chromium の実測: 山 0.496、両隣 ±0.004
        assert!((out[LATENCY_4X] - 0.496).abs() < 0.002, "{}", out[LATENCY_4X]);
        assert!((out.iter().sum::<f64>() - 0.5).abs() < 1e-3);
    }
}
