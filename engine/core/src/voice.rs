//! Voice: 1回の押下 = 1 Voice。旧 JS 版 noteOn() / noteOff() / tick() / spawnGrain() の移植
//!   layers → VCF (1段 or 2段直列) → amp (ゆらぎ) → vca (ADSR) → StereoPanner
//! メモリはエンジン生成時に確保し、発音中は確保しない

use crate::dsp::biquad::Biquad;
use crate::dsp::delay::{Delay, Quantum};
use crate::dsp::osc::Osc;
use crate::dsp::param::{tc, Param};
use crate::dsp::{cents, clamp, frac, pan_gains};
use crate::patches::{FType, Layer, Lfo, Num, Patch, Shape, Wave};
use crate::rng::{Rng, Walk};
use crate::tables::Tables;
use crate::dsp::shaper::lookup;
use libm::{exp, floor, log, pow, sin, sqrt};

const C0: f64 = 16.3516;
pub const MAX_OSC: usize = 8;
pub const MAX_NOISE: usize = 2;
pub const MAX_KARPLUS: usize = 2;
pub const MAX_GRAINS: usize = 64;
const MAX_ARP_EVENTS: usize = 16;
/// フィルタ係数を計算し直す間隔 (サンプル)
const COEF_EVERY: u32 = 16;

// ============================================================
// LFO: sine / square / sawtooth / sh (sample & hold)
// ============================================================
#[derive(Clone, Copy, Default)]
pub struct LfoUnit {
    shape: Shape,
    phase: f64,
    /// OscillatorNode.frequency、sh は AudioBufferSourceNode.playbackRate
    rate: Param,
    /// sh の playbackRate は k-rate (128 サンプルの先頭の値を使う)
    rate_k: f64,
    scale: f64,
    min: f64,
    rate_walk: Walk,
    depth: Param,
    depth_walk: Walk,
}

impl LfoUnit {
    fn new(shape: &[Shape], rate: Num, depth: Num, depth_scale: f64, min: f64, rng: &mut Rng, sr: f64) -> Self {
        let shape = rng.pick(shape);
        let rate = clamp(rate.sample(rng), min, 100.0);
        // sh は 1秒 64段の階段を playbackRate = rate / 64 でループ再生する
        // 開始オフセット Math.random() 秒は、Chromium ではサンプル位置に丸められる
        let (scale, phase) = if shape == Shape::Sh { (1.0 / 64.0, libm::round(rng.random() * sr) / sr) } else { (1.0, 0.0) };
        let depth = depth.sample(rng) * depth_scale;
        LfoUnit {
            shape,
            phase,
            rate: Param::new(rate * scale),
            rate_k: rate * scale,
            scale,
            min,
            // 発音中も周期と深さがふらつく
            rate_walk: Walk::new(log(rate), 0.12, 0.6),
            depth: Param::new(depth),
            depth_walk: Walk::new(depth, depth.abs() * 0.12 + 1e-6, 0.6),
        }
    }

    fn from_spec(spec: &Lfo, depth_scale: f64, rng: &mut Rng, sr: f64) -> Self {
        Self::new(spec.shape, spec.rate, spec.depth, depth_scale, 1.0, rng, sr)
    }

    fn tick(&mut self, dt: f64, rng: &mut Rng, sr: f64) {
        let x = self.rate_walk.step(dt, rng);
        self.rate.set_target(clamp(exp(x), self.min, 100.0) * self.scale, 0.1, sr);
        let d = self.depth_walk.step(dt, rng);
        self.depth.set_target(d, 0.1, sr);
    }

    /// LFO × depth
    #[inline]
    /// k: 128 サンプル境界か
    fn next(&mut self, t: &Tables, k: bool) -> f64 {
        let sr = t.sr;
        let mut r = self.rate.next();
        if self.shape == Shape::Sh {
            if k {
                self.rate_k = r;
            }
            r = self.rate_k;
        }
        let p = self.phase;
        // sine / square / sawtooth は OscillatorNode なので、LFO でも帯域制限の波形 (角にリンギング) になる
        let y = match self.shape {
            Shape::Sine => sin(2.0 * core::f64::consts::PI * p),
            Shape::Square => t.wt.sample(Wave::Square, p, r),
            Shape::Saw => t.wt.sample(Wave::Saw, p, r),
            // 1秒のバッファを小数位置で読むので、段の境目は線形補間でなだらかにつながる
            Shape::Sh => {
                let x = p * sr;
                let i = x as usize;
                let n = t.sh_buf.len();
                let a = t.sh_buf[i % n];
                let b = t.sh_buf[(i + 1) % n];
                a + (b - a) * (x - i as f64)
            }
        };
        self.phase = frac(p + r / sr);
        y * self.depth.next()
    }
}

// ============================================================
// レイヤー
// ============================================================
#[derive(Clone, Copy, Default)]
struct OscUnit {
    osc: Osc,
    wave: Wave,
    freq: f64,
    lg: f64,
    det: Param,
    det_walk: Walk,
    lfo: Option<LfoUnit>,
}

#[derive(Clone, Copy, Default)]
struct NoiseUnit {
    pos: usize,
    lg: Param,
    lg_walk: Walk,
    lfo: Option<LfoUnit>,
}

#[derive(Clone, Default)]
struct KarplusUnit {
    f: f64,
    pos: usize,
    eg: Param,
    ex_base: f64,
    ex_walk: Walk,
    dl: Delay,
    /// 閉路の dl → lp の辺は 128 サンプル遅れる (旧版の実際の周期は 1/f + 128 サンプル)
    q: Quantum,
    lp: Biquad,
    fb: Param,
    fb_walk: Walk,
    lg: f64,
    dtime: Param,
    lfo: Option<LfoUnit>,
    pluck: f64,
}

#[derive(Clone, Copy, Default)]
struct Grain {
    active: bool,
    start: f64,
    rev: bool,
    pos: f64,
    ratio: f64,
    t: f64,
    dur: f64,
    amp: f64,
    travelled: f64,
    limit: f64,
}

#[derive(Clone, Copy)]
struct GrainUnit {
    lg: f64,
    wow: LfoUnit,
    flut: LfoUnit,
    next: f64,
    dens: f64,
    dens_walk: Walk,
    rate: f64,
    spread: f64,
    dur: Num,
    rev: f64,
    grains: [Grain; MAX_GRAINS],
}

impl Default for GrainUnit {
    fn default() -> Self {
        GrainUnit {
            lg: 0.0,
            wow: LfoUnit::default(),
            flut: LfoUnit::default(),
            next: 0.0,
            dens: 1.0,
            dens_walk: Walk::default(),
            rate: 1.0,
            spread: 0.0,
            dur: Num::F(0.1),
            rev: 0.0,
            grains: [Grain::default(); MAX_GRAINS],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ArpUnit {
    scale: &'static [f64],
    range: f64,
    rate: f64,
    next: f64,
    rate_walk: Walk,
}

// ============================================================
// VCA: linearRamp → setTarget(sus) / cancelAndHold → setTarget(0)
// ============================================================
#[derive(Clone, Copy, Default, PartialEq)]
enum Stage {
    #[default]
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy, Default)]
struct Env {
    stage: Stage,
    v: f64,
    t: f64,
    a_len: f64,
    sus: f64,
    dcoef: f64,
    rcoef: f64,
}

impl Env {
    /// このサンプルの値を返して1サンプル進める (AudioParam と同じ)
    #[inline]
    fn next(&mut self) -> f64 {
        if self.stage == Stage::Attack {
            // linearRampToValueAtTime(1, t + a): 押した瞬間は 0
            self.v = (self.t / self.a_len).min(1.0);
            self.t += 1.0;
            if self.t > self.a_len {
                self.stage = Stage::Decay;
            }
            return self.v;
        }
        let v = self.v;
        match self.stage {
            Stage::Decay => self.v += (self.sus - self.v) * self.dcoef,
            _ => self.v -= self.v * self.rcoef,
        }
        v
    }
}

// ============================================================
// Voice
// ============================================================
pub struct Voice {
    pub active: bool,
    pub pad: usize,
    pub released: bool,
    pub born: u64,
    end_at: f64,
    root: f64,

    pitch_cv: Param,
    arp_cv: f64,
    arp_events: [(f64, f64); MAX_ARP_EVENTS],
    arp_n: usize,
    arp_now: f64,
    arp: Option<ArpUnit>,
    drift_dir: f64,
    drift_rate: f64,
    drift_max: f64,
    pitch: f64,
    pitch_walk: Walk,
    /// 最後の tick で決めた音程 (cent)
    cents: f64,

    ftype: FType,
    nf: usize,
    filt: [Biquad; 2],
    cut: f64,
    cut_dir: f64,
    cut_rate: f64,
    q_walk: Walk,
    freq_p: Param,
    q_p: Param,
    filt_lfo: Option<LfoUnit>,
    filt_lfo_v: f64,
    coef_count: u32,

    amp: Param,
    amp_walk: Walk,
    env: Env,
    pan: Param,
    pan_walk: Walk,

    oscs: [OscUnit; MAX_OSC],
    n_osc: usize,
    noises: [NoiseUnit; MAX_NOISE],
    n_noise: usize,
    kps: [KarplusUnit; MAX_KARPLUS],
    n_kp: usize,
    grain: Option<Box<GrainUnit>>,
    has_grain: bool,
    /// 層を作った順 (種類, 番号)。tick のゆらぎを旧版と同じ順で進めるため
    order: [(Kind, usize); MAX_LAYERS],
    n_order: usize,
    /// 粒の再生速度 (AudioBufferSourceNode の detune は 128 サンプルごとの k-rate)
    grain_rate: f64,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Kind {
    #[default]
    Osc,
    Noise,
    Karplus,
    Grain,
}

const MAX_LAYERS: usize = MAX_OSC + MAX_NOISE + MAX_KARPLUS + 1;

/// 検証・テスト用の中身 (旧版の window.stair.debug() と同じ項目)
#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceInfo {
    pub pad: usize,
    pub root: f64,
    pub cut: f64,
    pub pitch: f64,
    pub cents: f64,
    /// 音程 CV (pitchCV.offset) の今の値
    pub cv: f64,
    /// 1つ目の karplus の遅延時間 (秒)。無ければ NaN
    pub delay: f64,
    pub released: bool,
}

/// 発音時に読むつまみの値
pub struct NoteParams {
    pub attack: f64,
    pub decay: f64,
}

impl Voice {
    pub fn new(sr: f64) -> Self {
        // karplus の遅延は最長 0.5 秒 (1/f の上限 1 秒は 2Hz 以下でしか効かない)
        let kp_len = (sr * 0.5) as usize;
        let kp = || KarplusUnit { dl: Delay::new(kp_len), ..Default::default() };
        Voice {
            active: false,
            pad: 0,
            released: false,
            born: 0,
            end_at: f64::INFINITY,
            root: C0,
            pitch_cv: Param::new(0.0),
            arp_cv: 0.0,
            arp_events: [(0.0, 0.0); MAX_ARP_EVENTS],
            arp_n: 0,
            arp_now: 0.0,
            arp: None,
            drift_dir: 0.0,
            drift_rate: 0.0,
            drift_max: 1200.0,
            pitch: 0.0,
            pitch_walk: Walk::default(),
            cents: 0.0,
            ftype: FType::Lowpass,
            nf: 1,
            filt: [Biquad::default(); 2],
            cut: 1000.0,
            cut_dir: 1.0,
            cut_rate: 0.0,
            q_walk: Walk::default(),
            freq_p: Param::new(1000.0),
            q_p: Param::new(1.0),
            filt_lfo: None,
            filt_lfo_v: 0.0,
            coef_count: 0,
            amp: Param::new(0.0),
            amp_walk: Walk::default(),
            env: Env::default(),
            pan: Param::new(0.0),
            pan_walk: Walk::default(),
            oscs: [OscUnit::default(); MAX_OSC],
            n_osc: 0,
            noises: [NoiseUnit::default(); MAX_NOISE],
            n_noise: 0,
            kps: [kp(), kp()],
            n_kp: 0,
            grain: Some(Box::default()),
            has_grain: false,
            order: [(Kind::Osc, 0); MAX_LAYERS],
            n_order: 0,
            grain_rate: 1.0,
        }
    }

    /// noteOn: パッチの分布から今回の値を確定させる
    #[allow(clippy::too_many_arguments)]
    pub fn start(&mut self, pad: usize, pt: &Patch, now: f64, born: u64, np: &NoteParams, rng: &mut Rng, tables: &Tables) {
        let sr = tables.sr;
        self.active = true;
        self.pad = pad;
        self.released = false;
        self.born = born;
        self.end_at = f64::INFINITY;
        self.root = C0 * pow(2.0, pt.root.sample(rng) / 12.0);

        // 音程CV (cent)。ドリフト + ランダムウォーク / アルペジオ を全オシレータの detune に足す
        self.pitch_cv = Param::new(0.0);
        self.arp_cv = 0.0;
        self.arp_n = 0;
        self.arp_now = 0.0;
        let d = &pt.drift;
        let r = rng.random();
        self.drift_dir = if r < d.up { 1.0 } else if r < d.up + d.down { -1.0 } else { 0.0 };
        self.drift_rate = d.rate.sample(rng);
        self.drift_max = d.max;
        self.pitch = 0.0;
        self.cents = 0.0;
        self.pitch_walk = Walk::new(0.0, rng.rnd(3.0, 12.0), 2.0);

        // VCF
        let f = &pt.filter;
        self.ftype = rng.pick(f.ftype);
        self.nf = if rng.random() < f.series { 2 } else { 1 };
        self.cut = f.cut.sample(rng);
        self.cut_dir = if rng.random() < f.open { 1.0 } else { -1.0 };
        self.cut_rate = f.sweep.sample(rng);
        let q = f.q.sample(rng);
        self.q_walk = Walk::new(q, q * 0.1, 0.8);
        self.freq_p = Param::new(self.cut);
        self.q_p = Param::new(q);
        self.filt = [Biquad::default(); 2];
        self.filt_lfo = f.lfo.as_ref().map(|l| LfoUnit::from_spec(l, 1.0, rng, sr));
        self.filt_lfo_v = 0.0;
        self.coef_count = 0;
        self.update_coefs(sr);

        // VCA
        let level = pt.gain * if self.nf > 1 && self.ftype == FType::Lowpass { 0.5 } else { 1.0 } * rng.rnd(0.8, 1.1);
        self.amp = Param::new(level);
        self.amp_walk = Walk::new(level, level * 0.08, 3.0);
        self.pan = Param::new(0.0);
        self.pan_walk = Walk::new(rng.rnd(-0.6, 0.6), 0.25, 0.5);

        self.n_osc = 0;
        self.n_noise = 0;
        self.n_kp = 0;
        self.has_grain = false;
        self.n_order = 0;
        for layer in pt.layers {
            match *layer {
                Layer::Osc { wave, oct, count, spread, level, lfo } => {
                    let lvl = level.sample(rng);
                    let count = floor(count.sample(rng)) as usize;
                    let oct = oct.sample(rng);
                    let spread = spread.sample(rng);
                    let lg = lvl / sqrt(count as f64) * 0.5;
                    for k in 0..count {
                        if self.n_osc >= MAX_OSC {
                            break;
                        }
                        let wave = rng.pick(wave);
                        // 複数本を少しずつずらしてモアレ / うなりを作る
                        let off = if count > 1 { (k as f64 / (count - 1) as f64 - 0.5) * 2.0 * spread } else { 0.0 }
                            + rng.gauss() * spread * 0.2;
                        // オシレータごとに別の設定の LFO
                        let lfo = lfo.as_ref().map(|l| LfoUnit::from_spec(l, 1.0, rng, sr));
                        self.oscs[self.n_osc] = OscUnit {
                            osc: Osc::default(),
                            wave,
                            freq: self.root * oct,
                            lg,
                            det: Param::new(off),
                            det_walk: Walk::new(off, spread * 0.15 + 1.0, 0.7),
                            lfo,
                        };
                        self.order[self.n_order] = (Kind::Osc, self.n_osc);
                        self.n_order += 1;
                        self.n_osc += 1;
                    }
                }
                Layer::Noise { level, lfo } => {
                    if self.n_noise >= MAX_NOISE {
                        continue;
                    }
                    let lvl = level.sample(rng);
                    let pos = noise_start(rng, tables);
                    let base = lvl * 0.35;
                    // ノイズには音程がないので LFO は音量 (チョップ / トレモロ) にかける
                    let lfo = lfo.as_ref().map(|l| LfoUnit::from_spec(l, base, rng, sr));
                    self.noises[self.n_noise] = NoiseUnit { pos, lg: Param::new(base), lg_walk: Walk::new(base, base * 0.15, 1.0), lfo };
                    self.order[self.n_order] = (Kind::Noise, self.n_noise);
                    self.n_order += 1;
                    self.n_noise += 1;
                }
                Layer::Karplus { oct, level, excite, pluck, fb, damp, lfo } => {
                    if self.n_kp >= MAX_KARPLUS {
                        continue;
                    }
                    // ノイズ励起 → Delay(1/f) → LPF → tanh → feedback → Delay のくし形共鳴
                    // 旧版は 300Hz で頭打ちにしている (ループ内の遅延は 128 サンプル未満にできないと考えていたため)
                    let lvl = level.sample(rng);
                    let f = (self.root * oct.sample(rng)).min(300.0);
                    let pos = noise_start(rng, tables);
                    let ex = excite.sample(rng);
                    let mut lp = Biquad::default();
                    // lowpass の Q は dB 指定で、既定 1dB だと山が 1 を超えループが発散する。山を消す
                    lp.set(FType::Lowpass, damp.sample(rng), -6.0, sr);
                    let fb0 = fb.sample(rng);
                    let lfo = lfo.as_ref().map(|l| LfoUnit::from_spec(l, 1.0 / f / 1731.0, rng, sr));
                    let kp = &mut self.kps[self.n_kp];
                    kp.f = f;
                    kp.pos = pos;
                    // 初回の弾き
                    kp.eg = Param::new(0.8);
                    kp.eg.set_target_after((0.004 * sr) as u32, ex, 0.01, sr);
                    kp.ex_base = ex;
                    kp.ex_walk = Walk::new(ex, ex * 0.3, 1.0);
                    kp.dl.clear();
                    kp.q.clear();
                    kp.lp = lp;
                    kp.fb = Param::new(fb0);
                    kp.fb_walk = Walk::new(fb0, 0.003, 1.0);
                    kp.lg = lvl * 0.5;
                    kp.dtime = Param::new(1.0 / f);
                    kp.lfo = lfo;
                    kp.pluck = pluck.sample(rng);
                    self.order[self.n_order] = (Kind::Karplus, self.n_kp);
                    self.n_order += 1;
                    self.n_kp += 1;
                }
                Layer::Grain { level, density, dur, rev, spread, rate, wow, flutter } => {
                    if self.has_grain {
                        continue;
                    }
                    let lvl = level.sample(rng);
                    // テープの wow (遅い揺れ) と flutter (速い揺れ) を各粒の detune に配る
                    let wow = LfoUnit::new(&[Shape::Sine], wow.rate, wow.depth, 1.0, 0.05, rng, sr);
                    let flut = LfoUnit::new(&[Shape::Sine, Shape::Sh], flutter.rate, flutter.depth, 1.0, 0.05, rng, sr);
                    let dens = density.sample(rng);
                    let g = self.grain.as_mut().unwrap();
                    g.lg = lvl;
                    g.wow = wow;
                    g.flut = flut;
                    g.next = now;
                    g.dens = dens;
                    g.dens_walk = Walk::new(log(dens), 0.25, 0.5);
                    g.rate = rate.sample(rng);
                    g.spread = spread.sample(rng);
                    g.dur = dur;
                    g.rev = rev;
                    g.grains.iter_mut().for_each(|x| x.active = false);
                    self.order[self.n_order] = (Kind::Grain, 0);
                    self.n_order += 1;
                    self.has_grain = true;
                }
            }
        }

        // アルペジエータ
        self.arp = None;
        if let Some(a) = &pt.arp {
            if rng.random() < a.p {
                let scale = rng.pick(a.scale).degrees();
                let range = floor(a.range.sample(rng));
                let rate = a.rate.sample(rng);
                self.arp = Some(ArpUnit {
                    scale,
                    range,
                    rate,
                    next: now,
                    rate_walk: Walk::new(log(rate), 0.2, 0.5),
                });
            }
        }

        // エンベロープ (全体共通スライダ × 発音ごとのゆらぎ)
        let a = np.attack * rng.rnd(0.75, 1.3);
        let dc = np.decay * rng.rnd(0.75, 1.3);
        let sus = pt.sus.sample(rng);
        self.env = Env { stage: Stage::Attack, v: 0.0, t: 0.0, a_len: (a * sr).max(1.0), sus, dcoef: tc(dc / 3.0, sr), rcoef: 0.0 };
    }

    pub fn info(&self) -> VoiceInfo {
        VoiceInfo {
            pad: self.pad,
            root: self.root,
            cut: self.cut,
            pitch: self.pitch,
            cents: self.cents,
            cv: self.pitch_cv.v,
            delay: if self.n_kp > 0 { self.kps[0].dtime.v } else { f64::NAN },
            released: self.released,
        }
    }

    pub fn note_off(&mut self, now: f64, release: f64, rng: &mut Rng, sr: f64) {
        self.released = true;
        let rel = release * rng.rnd(0.75, 1.3);
        self.env.stage = Stage::Release;
        self.env.rcoef = tc(rel / 6.0, sr);
        self.end_at = now + rel + 0.05;
    }

    fn update_coefs(&mut self, sr: f64) {
        let freq = self.freq_p.v * cents(self.filt_lfo_v);
        let q = self.q_p.v;
        for bq in self.filt.iter_mut().take(self.nf) {
            bq.set(self.ftype, freq, q, sr);
        }
    }

    /// 30ms ごとの変調: ドリフト、フィルタスイープ、ランダムウォーク、アルペジオ、粒。
    /// 戻り値 false は破棄してよい
    pub fn tick(&mut self, now: f64, dt: f64, transpose: f64, rng: &mut Rng, tables: &Tables) -> bool {
        let sr = tables.sr;
        if self.released && now >= self.end_at {
            self.active = false;
            return false;
        }
        let ahead = now + 0.1;

        // 緩やかな上昇 / 下降。上限で折り返す
        self.pitch += self.drift_dir * self.drift_rate * dt;
        if self.pitch.abs() > self.drift_max {
            self.pitch = clamp(self.pitch, -self.drift_max, self.drift_max);
            self.drift_dir = -self.drift_dir;
        }
        let c = self.pitch + self.pitch_walk.step(dt, rng) + transpose;
        self.cents = c;
        self.pitch_cv.set_target(c, 0.05, sr);

        // フィルタが開く / 閉じる。端で折り返す
        self.cut *= pow(2.0, self.cut_dir * self.cut_rate * dt + rng.gauss() * 0.08 * sqrt(dt));
        if self.cut > 12000.0 {
            self.cut = 12000.0;
            self.cut_dir = -1.0;
        }
        if self.cut < 40.0 {
            self.cut = 40.0;
            self.cut_dir = 1.0;
        }
        let q = self.q_walk.step(dt, rng).max(0.1);
        self.freq_p.set_target(self.cut, 0.05, sr);
        self.q_p.set_target(q, 0.1, sr);

        let a = self.amp_walk.step(dt, rng).max(0.0);
        self.amp.set_target(a, 0.08, sr);
        let p = clamp(self.pan_walk.step(dt, rng), -1.0, 1.0);
        self.pan.set_target(p, 0.1, sr);

        // 各部のゆらぎ
        if let Some(l) = self.filt_lfo.as_mut() {
            l.tick(dt, rng, sr);
        }
        for &(kind, i) in self.order.iter().take(self.n_order) {
            match kind {
                Kind::Osc => {
                    let o = &mut self.oscs[i];
                    if let Some(l) = o.lfo.as_mut() {
                        l.tick(dt, rng, sr);
                    }
                    let x = o.det_walk.step(dt, rng);
                    o.det.set_target(x, 0.08, sr);
                }
                Kind::Noise => {
                    let n = &mut self.noises[i];
                    if let Some(l) = n.lfo.as_mut() {
                        l.tick(dt, rng, sr);
                    }
                    let x = n.lg_walk.step(dt, rng).max(0.0);
                    n.lg.set_target(x, 0.08, sr);
                }
                Kind::Karplus => {
                    let kp = &mut self.kps[i];
                    if let Some(l) = kp.lfo.as_mut() {
                        l.tick(dt, rng, sr);
                    }
                    let x = kp.fb_walk.step(dt, rng);
                    kp.fb.set_target(clamp(x, 0.0, 0.997), 0.1, sr);
                    kp.ex_base = kp.ex_walk.step(dt, rng).max(0.0);
                }
                Kind::Grain => {
                    let g = self.grain.as_mut().unwrap();
                    g.wow.tick(dt, rng, sr);
                    g.flut.tick(dt, rng, sr);
                    g.dens = exp(g.dens_walk.step(dt, rng));
                }
            }
        }
        if let Some(arp) = self.arp.as_mut() {
            arp.rate = clamp(exp(arp.rate_walk.step(dt, rng)), 1.0, 30.0);
        }

        // アルペジオ: 100ms 先までの音程変化を予約する
        if let Some(arp) = self.arp.as_mut() {
            while arp.next < ahead {
                let at = arp.next.max(now);
                // 旧版の R() は数2つの配列を範囲とみなすので、fifths [0, 7] は 0〜7 の連続値になる
                let s = arp.scale;
                let d = if s.len() == 2 { rng.rnd(s[0], s[1]) } else { rng.pick(s) };
                let deg = d + 12.0 * floor(rng.random() * arp.range);
                if self.arp_n < MAX_ARP_EVENTS {
                    self.arp_events[self.arp_n] = (at * sr, deg * 100.0);
                    self.arp_n += 1;
                }
                self.arp_now = deg * 100.0;
                arp.next = at + rng.rnd(0.6, 1.4) / arp.rate;
            }
        }

        for kp in self.kps.iter_mut().take(self.n_kp) {
            let f = kp.f * cents(c + self.arp_now);
            kp.dtime.set_target(clamp(1.0 / f, 0.0005, 1.0), 0.02, sr);
            if kp.pluck > 0.0 && rng.random() < kp.pluck * dt {
                kp.eg.set_value(rng.rnd(0.3, 0.9));
                kp.eg.set_target_after((0.004 * sr) as u32, kp.ex_base, 0.01, sr);
            } else {
                kp.eg.set_target(kp.ex_base, 0.05, sr);
            }
        }

        if self.has_grain {
            let g = self.grain.as_mut().unwrap();
            if g.next < now {
                g.next = now;
            }
            while g.next < ahead {
                let at = g.next;
                spawn_grain(g, at, rng, tables);
                g.next += rng.rnd(0.3, 1.7) / clamp(g.dens, 0.5, 80.0);
            }
        }
        true
    }

    /// 1サンプル分。戻り値は (L, R)
    #[inline]
    pub fn render(&mut self, t: f64, tables: &Tables) -> (f64, f64) {
        let sr = tables.sr;
        let k = (t as u64).is_multiple_of(128);
        let pcv = self.pitch_cv.next();
        while self.arp_n > 0 && self.arp_events[0].0 <= t {
            self.arp_cv = self.arp_events[0].1;
            self.arp_events.copy_within(1..self.arp_n, 0);
            self.arp_n -= 1;
        }
        let cv = pcv + self.arp_cv;
        let mut x = 0.0;

        for o in self.oscs.iter_mut().take(self.n_osc) {
            let lfo = match o.lfo.as_mut() { Some(l) => l.next(tables, k), None => 0.0 };
            let det = o.det.next() + cv + lfo;
            x += o.osc.next(&tables.wt, o.wave, o.freq * cents(det), sr) * o.lg;
        }

        let nl = tables.noise.len();
        for n in self.noises.iter_mut().take(self.n_noise) {
            let lfo = match n.lfo.as_mut() { Some(l) => l.next(tables, k), None => 0.0 };
            let g = n.lg.next() + lfo;
            x += tables.noise[n.pos] * g;
            n.pos += 1;
            if n.pos >= nl {
                n.pos = 0;
            }
        }

        for kp in self.kps.iter_mut().take(self.n_kp) {
            let lfo = match kp.lfo.as_mut() { Some(l) => l.next(tables, k), None => 0.0 };
            let d = clamp(kp.dtime.next() + lfo, 0.0, 1.0) * sr;
            let y = kp.dl.read(d);
            // 傾き 1 の tanh でループの振幅に上限をつける
            let fb = kp.fb.next() * lookup(&tables.sat, kp.lp.process(kp.q.process(y)));
            let ex = tables.noise[kp.pos] * kp.eg.next();
            kp.pos += 1;
            if kp.pos >= nl {
                kp.pos = 0;
            }
            kp.dl.push(ex + fb);
            x += y * kp.lg;
        }

        if self.has_grain {
            let g = self.grain.as_mut().unwrap();
            let wow = g.wow.next(tables, k);
            let flut = g.flut.next(tables, k);
            if k {
                self.grain_rate = cents(cv + wow + flut);
            }
            let common = self.grain_rate;
            let mut s = 0.0;
            for gr in g.grains.iter_mut() {
                if !gr.active || t < gr.start {
                    continue;
                }
                // 窓: 0 → amp (dur の 40%) → 0
                let a = gr.dur * 0.4;
                let e = if gr.t < a { gr.amp * gr.t / a } else { gr.amp * (1.0 - (gr.t - a) / (gr.dur - a)) };
                let buf = if gr.rev { &tables.tape_rev } else { &tables.tape };
                let i = gr.pos as usize;
                let y = if i + 1 < buf.len() {
                    let f = gr.pos - i as f64;
                    buf[i] + (buf[i + 1] - buf[i]) * f
                } else {
                    0.0
                };
                s += y * e.max(0.0);
                let step = gr.ratio * common;
                gr.pos += step;
                gr.travelled += step;
                gr.t += 1.0;
                if gr.t >= gr.dur || gr.travelled >= gr.limit || gr.pos >= buf.len() as f64 {
                    gr.active = false;
                }
            }
            x += s * g.lg;
        }

        // VCF
        let lfo = match self.filt_lfo.as_mut() { Some(l) => l.next(tables, k), None => 0.0 };
        self.freq_p.next();
        self.q_p.next();
        // LFO が付いたフィルタ (sample & hold で急に動く) は Chromium と同じく毎サンプル係数を計算する。
        // それ以外は変化が緩やかなので 16 サンプルごと
        if self.coef_count == 0 || self.filt_lfo.is_some() {
            self.filt_lfo_v = lfo;
            self.update_coefs(sr);
        }
        self.coef_count = (self.coef_count + 1) % COEF_EVERY;
        let mut y = self.filt[0].process(x);
        if self.nf > 1 {
            y = self.filt[1].process(y);
        }

        y *= self.amp.next() * self.env.next();
        let (gl, gr) = pan_gains(self.pan.next());
        (y * gl, y * gr)
    }
}

/// ループするノイズの開始位置 (旧版の src.start(t, Math.random() * 2))。
/// Chromium は開始オフセットを最も近いサンプルに丸める (実測)
fn noise_start(rng: &mut Rng, tables: &Tables) -> usize {
    let p = libm::round(rng.random() * 2.0 * tables.sr) as usize;
    p % tables.noise.len()
}

fn spawn_grain(g: &mut GrainUnit, at: f64, rng: &mut Rng, tables: &Tables) {
    let sr = tables.sr;
    let dur = g.dur.sample(rng) * rng.rnd(0.7, 1.3);
    let rev = rng.random() < g.rev;
    let det = rng.gauss() * g.spread;
    let amp = (1.2 / sqrt((g.dens * dur).max(1.0))).min(1.0);
    let buf_sec = tables.tape.len() as f64 / sr;
    let offset = rng.random() * (buf_sec - dur * 2.0 - 0.05);
    let Some(slot) = g.grains.iter_mut().find(|x| !x.active) else { return };
    *slot = Grain {
        active: true,
        start: at * sr,
        rev,
        pos: offset.max(0.0) * sr,
        ratio: g.rate * cents(det),
        t: 0.0,
        dur: (dur * sr).max(2.0),
        amp,
        travelled: 0.0,
        // start(at, offset, dur + .02): バッファ上で dur + 20ms 進んだら止まる
        limit: (dur + 0.02) * sr,
    };
}
