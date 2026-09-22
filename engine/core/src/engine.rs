//! エンジン本体。Web (wasm) と AU (staticlib) はこの API だけを使う

use crate::fx::Fx;
use crate::params::{Values, PARAMS, PITCH};
use crate::patches::PATCHES;
use crate::rng::Rng;
use crate::tables::Tables;
use crate::voice::{NoteParams, Voice, VoiceInfo};
use crate::dsp::clamp;

pub const MAX_VOICES: usize = 40;
pub const PADS: usize = PATCHES.len();

pub struct Engine {
    sr: f64,
    rng: Rng,
    tables: Tables,
    params: [f64; 10],
    voices: Vec<Voice>,
    born: u64,
    fx: Fx,
    /// 経過サンプル数
    now: u64,
    ticks: u64,
    next_tick: u64,
    last_tick: f64,
    /// tick で回す順 (発音した順)。旧版の voices 配列と同じ順に乱数を使うため
    order: Vec<usize>,
}

/// k 回目の変調ループの時刻 (サンプル)。30ms ごとを 128 サンプル境界に切り上げる
/// (旧版を OfflineAudioContext で鳴らす検証用の harness と同じ規則)
fn tick_frame(k: u64, sr: f64) -> u64 {
    let f = k as f64 * 0.03 * sr / 128.0;
    (libm::ceil(f - 1e-9) as u64) * 128
}

impl Engine {
    pub fn new(sr: f64, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let tables = Tables::new(sr, &mut rng);
        let voices = (0..MAX_VOICES).map(|_| Voice::new(sr)).collect();
        let mut e = Engine {
            sr,
            rng,
            tables,
            params: PARAMS.map(|d| d.default),
            voices,
            born: 0,
            fx: Fx::new(sr),
            now: 0,
            ticks: 1,
            next_tick: 0,
            last_tick: 0.0,
            order: Vec::with_capacity(MAX_VOICES),
        };
        e.next_tick = tick_frame(1, sr);
        e.fx.apply(&Values(&e.params));
        e
    }

    /// 検証用: 途中の段の出力を返す
    pub fn set_tap(&mut self, tap: crate::fx::Tap) {
        self.fx.tap = tap;
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    fn time(&self) -> f64 {
        self.now as f64 / self.sr
    }

    pub fn set_param(&mut self, idx: usize, v: f64) {
        let Some(d) = PARAMS.get(idx) else { return };
        self.params[idx] = clamp(v, d.min, d.max);
        self.fx.apply(&Values(&self.params));
    }

    pub fn param(&self, idx: usize) -> f64 {
        self.params[idx]
    }

    pub fn note_on(&mut self, pad: usize) {
        if pad >= PADS {
            return;
        }
        // 同時発音が溜まりすぎたら、リリース中の古いものから捨てる
        let slot = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => {
                let oldest = |released: bool| {
                    self.voices.iter().enumerate().filter(|(_, v)| !released || v.released).min_by_key(|(_, v)| v.born).map(|(i, _)| i)
                };
                oldest(true).or_else(|| oldest(false)).unwrap()
            }
        };
        let p = Values(&self.params);
        let np = NoteParams { attack: p.attack(), decay: p.decay() };
        self.born += 1;
        let now = self.time();
        self.voices[slot].start(pad, &PATCHES[pad], now, self.born, &np, &mut self.rng, &self.tables);
    }

    pub fn note_off(&mut self, pad: usize) {
        let now = self.time();
        let rel = Values(&self.params).release();
        for v in self.voices.iter_mut() {
            if v.active && v.pad == pad && !v.released {
                v.note_off(now, rel, &mut self.rng, self.sr);
            }
        }
    }

    /// 鳴っているボイスの中身を発音した順に
    pub fn voices(&self, out: &mut Vec<VoiceInfo>) {
        out.clear();
        let mut idx: Vec<usize> = (0..MAX_VOICES).filter(|&i| self.voices[i].active).collect();
        idx.sort_by_key(|&i| self.voices[i].born);
        out.extend(idx.into_iter().map(|i| self.voices[i].info()));
    }

    pub fn voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    fn tick(&mut self) {
        let now = self.time();
        let dt = clamp(now - self.last_tick, 0.001, 0.1);
        self.last_tick = now;
        // pitch つまみ。発音中の全ボイスに足す (cent)
        let transpose = self.params[PITCH] * 100.0;
        self.order.clear();
        self.order.extend((0..MAX_VOICES).filter(|&i| self.voices[i].active));
        let voices = &self.voices;
        self.order.sort_by_key(|&i| voices[i].born);
        for &i in &self.order {
            self.voices[i].tick(now, dt, transpose, &mut self.rng, &self.tables);
        }
        self.fx.tick(dt, &Values(&self.params), &mut self.rng);
    }

    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len().min(out_r.len());
        for i in 0..n {
            if self.now >= self.next_tick {
                self.tick();
                self.ticks += 1;
                self.next_tick = tick_frame(self.ticks, self.sr);
            }
            let t = self.now as f64;
            let (mut l, mut r) = (0.0, 0.0);
            let mut any = false;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let (a, b) = v.render(t, &self.tables);
                l += a;
                r += b;
                any = true;
            }
            let (yl, yr) = self.fx.process(l, r, any);
            out_l[i] = yl as f32;
            out_r[i] = yr as f32;
            self.now += 1;
        }
    }
}
