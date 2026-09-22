//! 音の指標とスペクトログラム。Claude が「聴く」代わりに見るもの

use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
use std::path::Path;

const WIN: usize = 2048;
/// 指標の時間分解能 (秒)
const STEP: f64 = 0.05;

#[derive(Serialize)]
pub struct Metrics {
    pub sample_rate: u32,
    pub seconds: f64,
    pub peak: f64,
    pub rms: f64,
    /// |x| >= 0.95 のサンプル数 (出力段のソフトクリップ上限)
    pub clipped: usize,
    pub nan_or_inf: usize,
    pub dc: f64,
    /// 左右の相関 (1 = モノラル, 0 = 無相関)
    pub stereo_corr: f64,
    /// 区間 [start, end) の要約 (発音中 / リリース後など)
    pub sections: Vec<Section>,
    /// STEP 秒ごとの推移
    pub curve: Curve,
}

#[derive(Serialize)]
pub struct Section {
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub rms: f64,
    pub peak: f64,
    /// スペクトル重心 (Hz)。明るさの目安
    pub centroid_mean: f64,
    pub centroid_std: f64,
    /// 推定基音 (Hz) と周期性 (0–1)。ノイズ系は周期性が低い
    pub f0: f64,
    pub periodicity: f64,
    /// 音量の揺れ (RMS 推移の変動係数)。トレモロ / チョップの目安
    pub rms_cv: f64,
    /// オクターブ帯域ごとの平均パワー (dB)。中心周波数は BANDS
    pub bands_db: Vec<f64>,
}

/// オクターブ帯域の中心周波数
pub const BANDS: [f64; 10] = [31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

fn bands(specs: &[Vec<f64>], sr: f64) -> Vec<f64> {
    let bin_hz = sr / WIN as f64;
    BANDS
        .iter()
        .map(|fc| {
            let (lo, hi) = ((fc / 2f64.sqrt() / bin_hz) as usize, ((fc * 2f64.sqrt() / bin_hz) as usize).max(1));
            let mut e = 0.0;
            for p in specs {
                e += p[lo.min(p.len())..hi.min(p.len())].iter().sum::<f64>();
            }
            let db = 10.0 * (e / specs.len().max(1) as f64 + 1e-20).log10();
            (db * 10.0).round() / 10.0
        })
        .collect()
}

#[derive(Serialize)]
pub struct Curve {
    pub step: f64,
    pub rms_db: Vec<f64>,
    pub centroid: Vec<f64>,
}

fn mono(l: &[f32], r: &[f32]) -> Vec<f64> {
    l.iter().zip(r).map(|(a, b)| (*a as f64 + *b as f64) * 0.5).collect()
}

fn rms(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn hann(n: usize) -> Vec<f64> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()).collect()
}

/// 各窓のパワースペクトル
fn spectra(x: &[f64], hop: usize) -> Vec<Vec<f64>> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(WIN);
    let w = hann(WIN);
    let mut out = vec![];
    let mut pos = 0;
    while pos + WIN <= x.len() {
        let mut buf: Vec<Complex<f64>> = (0..WIN).map(|i| Complex::new(x[pos + i] * w[i], 0.0)).collect();
        fft.process(&mut buf);
        // 振幅 A の正弦波が A² になるよう正規化
        let k = 1.0 / (WIN as f64 / 4.0).powi(2);
        out.push(buf[..WIN / 2].iter().map(|c| c.norm_sqr() * k).collect());
        pos += hop;
    }
    out
}

fn centroid(p: &[f64], sr: f64) -> f64 {
    let (mut num, mut den) = (0.0, 0.0);
    for (i, v) in p.iter().enumerate() {
        let f = i as f64 * sr / WIN as f64;
        num += f * v;
        den += v;
    }
    if den <= 1e-20 { 0.0 } else { num / den }
}

/// 自己相関で基音を推定 (50Hz–2kHz)
fn f0(x: &[f64], sr: f64) -> (f64, f64) {
    let n = x.len().min((sr * 0.2) as usize);
    if n < 1024 {
        return (0.0, 0.0);
    }
    let x = &x[..n];
    let e0: f64 = x.iter().map(|v| v * v).sum();
    if e0 <= 1e-12 {
        return (0.0, 0.0);
    }
    let (lo, hi) = ((sr / 2000.0) as usize, (sr / 50.0) as usize);
    let (mut best, mut best_lag) = (0.0, 0);
    // 0 付近の山を避けるため、相関が一度 0 を下回ってから最大を探す
    let mut dipped = false;
    for lag in lo..hi.min(n / 2) {
        let c: f64 = (0..n - lag).map(|i| x[i] * x[i + lag]).sum::<f64>() / e0;
        if !dipped {
            dipped = c < 0.0;
            continue;
        }
        if c > best {
            best = c;
            best_lag = lag;
        }
    }
    if best_lag == 0 { (0.0, 0.0) } else { (sr / best_lag as f64, best) }
}

pub fn analyze(l: &[f32], r: &[f32], sr: u32, marks: &[(String, f64, f64)]) -> Metrics {
    let srf = sr as f64;
    let m = mono(l, r);
    let all: Vec<f64> = l.iter().chain(r.iter()).map(|v| *v as f64).collect();
    let peak = all.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    let clipped = all.iter().filter(|v| v.abs() >= 0.95).count();
    let nan_or_inf = all.iter().filter(|v| !v.is_finite()).count();

    let (sl, sr_): (f64, f64) = (l.iter().map(|v| (*v as f64).powi(2)).sum(), r.iter().map(|v| (*v as f64).powi(2)).sum());
    let slr: f64 = l.iter().zip(r).map(|(a, b)| *a as f64 * *b as f64).sum();
    let stereo_corr = if sl * sr_ > 0.0 { slr / (sl * sr_).sqrt() } else { 0.0 };

    let hop = (STEP * srf) as usize;
    let specs = spectra(&m, hop);
    let rms_db: Vec<f64> = (0..specs.len()).map(|i| 20.0 * rms(&m[i * hop..i * hop + WIN]).max(1e-6).log10()).collect();
    let cents: Vec<f64> = specs.iter().map(|p| centroid(p, srf)).collect();

    let sections = marks
        .iter()
        .map(|(name, a, b)| {
            let (i0, i1) = (((a * srf) as usize).min(m.len()), ((b * srf) as usize).min(m.len()));
            let seg = &m[i0..i1];
            let (k0, k1) = (((a / STEP) as usize).min(specs.len()), ((b / STEP) as usize).saturating_sub(1).min(specs.len()));
            let cs: Vec<f64> = cents[k0..k1.max(k0)].to_vec();
            let rs: Vec<f64> = rms_db[k0..k1.max(k0)].iter().map(|d| 10f64.powf(d / 20.0)).collect();
            let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
            let std = |v: &[f64]| {
                let mu = mean(v);
                (v.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt()
            };
            let mid = seg.len() / 2;
            let (f, p) = f0(&seg[mid.saturating_sub((srf * 0.1) as usize)..], srf);
            Section {
                name: name.clone(),
                start: *a,
                end: *b,
                rms: rms(seg),
                peak: seg.iter().fold(0.0f64, |a, v| a.max(v.abs())),
                centroid_mean: mean(&cs),
                centroid_std: std(&cs),
                f0: f,
                periodicity: p,
                rms_cv: if mean(&rs) > 0.0 { std(&rs) / mean(&rs) } else { 0.0 },
                bands_db: bands(&specs[k0..k1.max(k0)], srf),
            }
        })
        .collect();

    Metrics {
        sample_rate: sr,
        seconds: l.len() as f64 / srf,
        peak,
        rms: rms(&m),
        clipped,
        nan_or_inf,
        dc: m.iter().sum::<f64>() / m.len().max(1) as f64,
        stereo_corr,
        sections,
        curve: Curve { step: STEP, rms_db: round(rms_db, 1), centroid: round(cents, 0) },
    }
}

fn round(v: Vec<f64>, d: i32) -> Vec<f64> {
    let k = 10f64.powi(d);
    v.into_iter().map(|x| (x * k).round() / k).collect()
}

// ------------------------------------------------------------
// PNG: 上に波形、下に対数周波数のスペクトログラム (30Hz–16kHz)
// ------------------------------------------------------------
const H_WAVE: usize = 80;
const H_SPEC: usize = 300;
const F_LO: f64 = 30.0;
const F_HI: f64 = 16000.0;

fn colormap(t: f64) -> [u8; 3] {
    // 黒 → 紫 → 橙 → 黄白
    let t = t.clamp(0.0, 1.0);
    let stops = [(0.0, [0.0, 0.0, 0.0]), (0.35, [80.0, 20.0, 120.0]), (0.7, [230.0, 90.0, 40.0]), (1.0, [255.0, 250.0, 200.0])];
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let f = (t - a) / (b - a);
            return [0, 1, 2].map(|i| (ca[i] + (cb[i] - ca[i]) * f) as u8);
        }
    }
    [255, 250, 200]
}

pub fn write_png(path: &Path, l: &[f32], r: &[f32], sr: u32, marks: &[f64]) -> std::io::Result<()> {
    let srf = sr as f64;
    let m = mono(l, r);
    let hop = (srf * 0.01) as usize; // 10ms / 列
    let specs = spectra(&m, hop);
    let w = specs.len().max(1);
    let h = H_WAVE + H_SPEC;
    let mut img = vec![20u8; w * h * 3];
    let mut put = |x: usize, y: usize, c: [u8; 3]| {
        let i = (y * w + x) * 3;
        img[i..i + 3].copy_from_slice(&c);
    };

    // 波形 (列ごとの最小 / 最大)
    for x in 0..w {
        let seg = &m[(x * hop).min(m.len())..((x + 1) * hop).min(m.len())];
        let (lo, hi) = seg.iter().fold((0.0f64, 0.0f64), |(a, b), v| (a.min(*v), b.max(*v)));
        let ymid = H_WAVE as f64 / 2.0;
        let y0 = (ymid - hi * ymid).clamp(0.0, H_WAVE as f64 - 1.0) as usize;
        let y1 = (ymid - lo * ymid).clamp(0.0, H_WAVE as f64 - 1.0) as usize;
        for y in y0..=y1 {
            put(x, y, [120, 200, 220]);
        }
        put(x, ymid as usize, [60, 60, 60]);
    }

    // スペクトログラム
    let bin_hz = srf / WIN as f64;
    for (x, p) in specs.iter().enumerate() {
        for y in 0..H_SPEC {
            let fy = 1.0 - y as f64 / (H_SPEC - 1) as f64;
            let f = F_LO * (F_HI / F_LO).powf(fy);
            let k = ((f / bin_hz) as usize).min(p.len() - 1);
            let db = 10.0 * (p[k] + 1e-20).log10();
            // -90dB .. 0dB を 0..1 に
            put(x, H_WAVE + y, colormap((db + 90.0) / 90.0));
        }
    }

    // 周波数の目盛 (100Hz, 1kHz, 10kHz) と発音 / 離鍵の位置
    for f in [100.0, 1000.0, 10000.0] {
        let fy = (f / F_LO).ln() / (F_HI / F_LO).ln();
        let y = H_WAVE + ((1.0 - fy) * (H_SPEC - 1) as f64) as usize;
        for x in (0..w).step_by(4) {
            put(x, y, [90, 90, 90]);
        }
    }
    for t in marks {
        let x = ((t * srf) as usize / hop).min(w - 1);
        for y in 0..h {
            if y % 3 == 0 {
                put(x, y, [60, 220, 90]);
            }
        }
    }

    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(&img).map_err(std::io::Error::other)?;
    Ok(())
}
