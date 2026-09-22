//! 全ボイス共有の素材 (旧 makeBuffers)

use crate::dsp::wavetable::Wavetables;
use crate::rng::Rng;
use libm::{pow, sin, tanh};

pub struct Tables {
    pub sr: f64,
    /// ホワイトノイズ 2秒
    pub noise: Vec<f64>,
    /// sample & hold: 1秒に 64 段の階段 (旧 shBuf)
    pub sh_buf: Vec<f64>,
    /// グラニュラー用の素材テープ 4秒 (正 / 逆)
    pub tape: Vec<f64>,
    pub tape_rev: Vec<f64>,
    /// 帯域制限の saw / square
    pub wt: Wavetables,
    /// karplus のループ内飽和 (旧 SAT_CURVE: tanh(i / 512 - 1)、1025 点)
    pub sat: Vec<f64>,
}

impl Tables {
    pub fn new(sr: f64, rng: &mut Rng) -> Self {
        let n = (sr * 2.0) as usize;
        let noise = (0..n).map(|_| (rng.random() * 2.0 - 1.0) as f32 as f64).collect();

        let mut sh_buf = vec![0.0; sr as usize];
        let step = sr / 64.0;
        for s in 0..64 {
            let val = (rng.random() * 2.0 - 1.0) as f32 as f64;
            for v in sh_buf[(s as f64 * step) as usize..((s + 1) as f64 * step) as usize].iter_mut() {
                *v = val;
            }
        }

        // グライドするノコギリ、ノイズの塊、クリックを混ぜた汚い音
        let len = (sr * 4.0) as usize;
        let mut tape = vec![0.0; len];
        let mut saws: [(f64, f64, f64); 3] = [(0.0, 0.0, 0.0); 3];
        for s in saws.iter_mut() {
            *s = (rng.rnd(60.0, 400.0), rng.rnd(-0.3, 0.3), 0.0);
        }
        let (mut burst, mut lp) = (0.0, 0.0);
        for (i, y) in tape.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let mut x = 0.0;
            for s in saws.iter_mut() {
                s.2 = (s.2 + s.0 * pow(2.0, s.1 * t) / sr) % 1.0;
                x += (s.2 * 2.0 - 1.0) * 0.25;
            }
            if rng.random() < 3.0 / sr {
                burst = 1.0;
            }
            burst *= 0.9996;
            lp += (rng.random() * 2.0 - 1.0 - lp) * 0.2;
            x += lp * burst * 1.2;
            if rng.random() < 8.0 / sr {
                x += rng.rnd(-1.0, 1.0);
            }
            x *= 0.6 + 0.4 * sin(t * 2.3) * sin(t * 0.7);
            *y = tanh(x) as f32 as f64;
        }
        let tape_rev = tape.iter().rev().copied().collect();
        // WaveShaper の曲線は Float32Array なので f32 に丸めておく
        let sat = (0..1025).map(|i| tanh(i as f64 / 512.0 - 1.0) as f32 as f64).collect();
        Tables { sr, noise, sh_buf, tape, tape_rev, wt: Wavetables::new(sr), sat }
    }
}
