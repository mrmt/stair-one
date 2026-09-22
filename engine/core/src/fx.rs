//! エフェクト列 (旧 ensureAudio / applyFx)
//!   voiceBus → Distortion → BitCrusher → Phaser ─┬→ sum
//!                                              └→ StereoDelay → sum
//!   → Compressor → master(volume) → tanh ソフトクリップ(±0.95)

use crate::dsp::biquad::Biquad;
use crate::dsp::compressor::Compressor;
use crate::dsp::delay::{Delay, Quantum};
use crate::dsp::param::Param;
use crate::dsp::shaper::{drive_curve, lookup, Oversampled4x, DRIVE_LEN};
use crate::dsp::{cents, clamp, frac, PI};
use crate::params::Values;
use crate::patches::FType;
use crate::rng::{Rng, Walk};
use libm::{floor, pow, sin, tanh};

/// bitcrush はつまみを持たず、軽くかけたままにする
const CRUSH: f64 = 0.2;
const PH_FREQS: [f64; 6] = [250.0, 520.0, 900.0, 1500.0, 2400.0, 3600.0];
const K: f64 = 0.05;

/// 検証用: 途中の段の出力をそのまま返す (旧版の capture-legacy --tap と比べる)
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Tap {
    #[default]
    Out,
    Voice,
    Dist,
    Crush,
    Phaser,
}

pub struct Fx {
    pub tap: Tap,
    stereo_seen: bool,
    sr: f64,
    dist_dry: Param,
    dist_wet: Param,
    shaper: [Oversampled4x; 2],
    curve: Box<[f64; DRIVE_LEN]>,
    last_drive: f64,

    bits: Param,
    down: Param,
    crush_c: [f64; 2],
    crush_h: [f64; 2],
    crush_dry: Param,
    crush_wet: Param,

    ph_phase: f64,
    ph_freq: Param,
    ph_depth: Param,
    ph_stages: [[Biquad; 6]; 2],
    ph_fb: Param,
    ph_fbd: [Delay; 2],
    /// 閉路 (最後の allpass → phFb) の 128 サンプル遅れ
    ph_q: [Quantum; 2],
    ph_wet: Param,
    ph_dry: Param,
    ph_count: u32,

    dl: Delay,
    dr: Delay,
    lpl: Biquad,
    lpr: Biquad,
    /// 閉路 (dL → lpL) の 128 サンプル遅れ
    dq: Quantum,
    fbl: Param,
    fbr: Param,
    dwet: Param,
    dtl: Param,
    dtr: Param,
    walk_l: Walk,
    walk_r: Walk,

    comp: Box<Compressor>,
    master: Param,
    /// 出力段のソフトクリップ曲線 (旧版の WaveShaper と同じ 1024 点)
    clip: Vec<f64>,
}

impl Fx {
    pub fn new(sr: f64) -> Self {
        let mut lpl = Biquad::default();
        // 既定 Q (1dB) の山とフィードバック最大 0.92 でループ利得が 1 を超えるので山を消す
        lpl.set(FType::Lowpass, 4500.0, -6.0, sr);
        let lpr = lpl;
        Fx {
            tap: Tap::Out,
            stereo_seen: false,
            sr,
            dist_dry: Param::new(1.0),
            dist_wet: Param::new(0.0),
            shaper: [Oversampled4x::default(); 2],
            curve: Box::new([0.0; DRIVE_LEN]),
            last_drive: -1.0,
            bits: Param::new(16.0),
            down: Param::new(1.0),
            crush_c: [0.0; 2],
            crush_h: [0.0; 2],
            crush_dry: Param::new(1.0),
            crush_wet: Param::new(0.0),
            ph_phase: 0.0,
            ph_freq: Param::new(0.3),
            ph_depth: Param::new(1800.0),
            ph_stages: [[Biquad::default(); 6]; 2],
            ph_fb: Param::new(0.0),
            ph_fbd: [Delay::new(1024), Delay::new(1024)],
            ph_q: [Quantum::default(); 2],
            ph_wet: Param::new(0.0),
            ph_dry: Param::new(1.0),
            ph_count: 0,
            dl: Delay::new((sr * 2.5) as usize + 4),
            dr: Delay::new((sr * 2.5) as usize + 4),
            lpl,
            lpr,
            dq: Quantum::default(),
            fbl: Param::new(0.0),
            fbr: Param::new(0.0),
            dwet: Param::new(0.0),
            dtl: Param::new(0.0),
            dtr: Param::new(0.0),
            walk_l: Walk::new(0.0, 0.004, 0.5),
            walk_r: Walk::new(0.0, 0.004, 0.5),
            comp: Compressor::new(sr, -18.0, 12.0, 6.0, 0.005, 0.2),
            master: Param::new(0.0),
            clip: (0..1024)
                .map(|i| {
                    let x = i as f64 / 1023.0 * 2.0 - 1.0;
                    (0.95 * tanh(2.0 * x) / tanh(2.0)) as f32 as f64
                })
                .collect(),
        }
    }

    /// つまみの値をエフェクトに反映する (旧 applyFx)
    pub fn apply(&mut self, p: &Values) {
        let sr = self.sr;
        let d = p.drive();
        if d != self.last_drive {
            drive_curve(d, &mut self.curve);
            self.last_drive = d;
        }
        let dm = (d * 3.0).min(1.0);
        self.dist_dry.set_target(1.0 - dm, K, sr);
        self.dist_wet.set_target(dm * 0.5, K, sr);

        let c = CRUSH;
        self.bits.set_target(16.0 - c * 14.0, K, sr);
        self.down.set_target(1.0 + c * c * 40.0, K, sr);
        let cm = (c * 3.0).min(1.0);
        self.crush_dry.set_target(1.0 - cm, K, sr);
        self.crush_wet.set_target(cm, K, sr);

        let ph = p.phaser();
        self.ph_freq.set_target(0.08 + ph * 1.5, K, sr);
        self.ph_depth.set_target(1200.0 + ph * 2400.0, K, sr);
        self.ph_fb.set_target(ph * 0.7, K, sr);
        self.ph_wet.set_target(ph, K, sr);
        self.ph_dry.set_target(1.0 - ph * 0.5, K, sr);

        self.fbl.set_target(p.dfb(), K, sr);
        self.fbr.set_target(p.dfb(), K, sr);
        self.dwet.set_target(p.dmix(), K, sr);

        let v = p.volume();
        self.master.set_target(v * v * 1.6, K, sr);
    }

    /// ディレイタイムもわずかに揺らしてテープっぽくする
    pub fn tick(&mut self, dt: f64, p: &Values, rng: &mut Rng) {
        let dtm = p.dtime();
        let wl = self.walk_l.step(dt, rng);
        let wr = self.walk_r.step(dt, rng);
        self.set_delay_times(dtm * (1.0 + wl), dtm * 0.62 * (1.0 + wr));
    }

    /// 検証用: ディレイタイムをすぐにその値にする
    pub fn jump_delay_times(&mut self, l: f64, r: f64) {
        self.dtl.set_value(l);
        self.dtr.set_value(r);
    }

    pub fn set_delay_times(&mut self, l: f64, r: f64) {
        self.dtl.set_target(l, 0.2, self.sr);
        self.dtr.set_target(r, 0.2, self.sr);
    }

    /// stereo: 鳴っているボイスがあるか。旧版の voiceBus は、ボイスが繋がっている間だけ 2ch になる
    /// (何も繋がっていない GainNode の出力は 1ch)。この違いが BitCrusher と StereoDelay に効く
    #[inline]
    pub fn process(&mut self, l: f64, r: f64, stereo: bool) -> (f64, f64) {
        self.stereo_seen |= stereo;
        let sr = self.sr;
        let mut x = [l * 0.6, r * 0.6];
        if self.tap == Tap::Voice {
            return (x[0], x[1]);
        }

        // ディストーション
        let (dd, dw) = (self.dist_dry.next(), self.dist_wet.next());
        for (ch, v) in x.iter_mut().enumerate() {
            let shaped = self.shaper[ch].process(*v, &self.curve[..]);
            *v = *v * dd + shaped * dw;
        }

        if self.tap == Tap::Dist {
            return (x[0], x[1]);
        }

        // ビットクラッシャー
        let (bits, down) = (self.bits.next(), self.down.next());
        let (cd, cw) = (self.crush_dry.next(), self.crush_wet.next());
        let q = pow(2.0, bits - 1.0);
        // AudioWorklet は入力と同じチャンネル数で動くので、1ch の間は右の数え上げが止まる。
        // そのため左右で標本化の位相がずれる
        let nch = if stereo { 2 } else { 1 };
        for (ch, v) in x.iter_mut().enumerate().take(nch) {
            self.crush_c[ch] += 1.0;
            if self.crush_c[ch] >= down {
                self.crush_c[ch] = 0.0;
                self.crush_h[ch] = floor(*v * q + 0.5) / q;
            }
            *v = *v * cd + self.crush_h[ch] * cw;
        }
        if !stereo {
            x[1] = x[1] * cd + self.crush_h[0] * cw;
        }

        if self.tap == Tap::Crush {
            return (x[0], x[1]);
        }

        // フェーザー: allpass 6段 + LFO(detune) + フィードバック
        let pf = self.ph_freq.next();
        let depth = self.ph_depth.next();
        let lfo = sin(2.0 * PI * self.ph_phase);
        self.ph_phase = frac(self.ph_phase + pf / sr);
        if self.ph_count == 0 {
            let ratio = cents(lfo * depth);
            for ch in 0..2 {
                for (i, st) in self.ph_stages[ch].iter_mut().enumerate() {
                    st.set(FType::Allpass, PH_FREQS[i] * ratio, 0.7, sr);
                }
            }
        }
        self.ph_count = (self.ph_count + 1) % 16;
        let (pfb, pw, pd) = (self.ph_fb.next(), self.ph_wet.next(), self.ph_dry.next());
        // フィードバックは 1ms + 閉路の 128 サンプル
        let fbd = 0.001 * sr;
        let mut out = [0.0; 2];
        for ch in 0..2 {
            let mut s = x[ch] + self.ph_fbd[ch].read(fbd);
            for st in self.ph_stages[ch].iter_mut() {
                s = st.process(s);
            }
            self.ph_fbd[ch].push(self.ph_q[ch].process(s) * pfb);
            out[ch] = x[ch] * pd + s * pw;
        }

        if self.tap == Tap::Phaser {
            return (out[0], out[1]);
        }

        // ステレオディレイ: L/R 別時間 + 交差フィードバック。出力はディレイごとにモノラルへまとめる (ChannelMerger)。
        // 線形なので入力を先にモノラルにしても同じ
        let mono = (out[0] + out[1]) * 0.5;
        // Chromium の DelayNode は delayTime の自動変化をチャンネルごとに読み進めるので、
        // ステレオ入力だと setTargetAtTime の追従が 2 倍速になる (実測で合わせた)。
        // フェーザーとディレイは閉路なので、最初の発音で 2ch になった後はずっと 2ch のまま
        let tl = clamp(self.dtl.next(), 0.0, 2.5) * sr;
        let tr = clamp(self.dtr.next(), 0.0, 2.5) * sr;
        if self.stereo_seen {
            self.dtl.next();
            self.dtr.next();
        }
        let yl = self.dl.read(tl);
        let yr = self.dr.read(tr);
        let (fl, fr) = (self.fbl.next(), self.fbr.next());
        // 左 → 右の辺だけ閉路の 128 サンプル遅れが乗る
        let to_r = self.lpl.process(self.dq.process(yl)) * fl;
        let to_l = self.lpr.process(yr) * fr;
        self.dl.push(mono + to_l);
        self.dr.push(mono * 0.7 + to_r);
        let wet = self.dwet.next();
        let sl = out[0] + yl * wet;
        let sr_ = out[1] + yr * wet;

        // 出力: コンプ → 音量 → tanh ソフトクリップ (ピークを 0.95 未満に抑える)
        let (cl, cr) = self.comp.process(sl, sr_);
        let m = self.master.next();
        (lookup(&self.clip, cl * m), lookup(&self.clip, cr * m))
    }
}

