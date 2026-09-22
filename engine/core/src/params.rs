//! つまみ定義。値は UI の input と同じ単位 (0–100 等) で持ち、音への写像はエンジン側で行う
//! id は AU のパラメータ ID にもなるので、以後変更・削除しない (追加のみ)

use libm::pow;

#[derive(Clone, Copy, Debug)]
pub struct ParamDef {
    pub id: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
}

const fn p(id: &'static str, label: &'static str, min: f64, max: f64, step: f64, default: f64) -> ParamDef {
    ParamDef { id, label, min, max, step, default }
}

pub const ATTACK: usize = 0;
pub const DECAY: usize = 1;
pub const RELEASE: usize = 2;
pub const PITCH: usize = 3;
pub const DTIME: usize = 4;
pub const DFB: usize = 5;
pub const DMIX: usize = 6;
pub const PHASER: usize = 7;
pub const DRIVE: usize = 8;
pub const VOLUME: usize = 9;

pub const PARAMS: [ParamDef; 10] = [
    p("attack", "attack", 0.0, 100.0, 1.0, 21.0),
    p("decay", "decay", 0.0, 100.0, 1.0, 60.0),
    p("release", "release", 0.0, 100.0, 1.0, 53.0),
    p("pitch", "pitch", -12.0, 12.0, 0.1, 0.0),
    p("dtime", "delay time", 0.0, 100.0, 1.0, 70.0),
    p("dfb", "delay feedback", 0.0, 92.0, 1.0, 40.0),
    p("dmix", "delay mix", 0.0, 100.0, 1.0, 25.0),
    p("phaser", "phaser", 0.0, 100.0, 1.0, 20.0),
    p("drive", "distortion", 0.0, 100.0, 1.0, 15.0),
    p("volume", "volume", 0.0, 100.0, 1.0, 80.0),
];

pub fn index_of(id: &str) -> Option<usize> {
    PARAMS.iter().position(|d| d.id == id)
}

/// 時間系は対数カーブ
fn log_map(x: f64, a: f64, b: f64) -> f64 {
    a * pow(b / a, x / 100.0)
}

/// つまみの生の値 → 音で使う値 (旧 JS 版の P と同じ写像)
pub struct Values<'a>(pub &'a [f64; 10]);

impl Values<'_> {
    pub fn attack(&self) -> f64 { log_map(self.0[ATTACK], 0.002, 4.0) }
    pub fn decay(&self) -> f64 { log_map(self.0[DECAY], 0.02, 6.0) }
    pub fn release(&self) -> f64 { log_map(self.0[RELEASE], 0.01, 8.0) }
    /// 半音
    pub fn pitch(&self) -> f64 { self.0[PITCH] }
    pub fn dtime(&self) -> f64 { log_map(self.0[DTIME], 0.02, 1.2) }
    pub fn dfb(&self) -> f64 { self.0[DFB] / 100.0 }
    pub fn dmix(&self) -> f64 { self.0[DMIX] / 100.0 }
    pub fn phaser(&self) -> f64 { self.0[PHASER] / 100.0 }
    pub fn drive(&self) -> f64 { self.0[DRIVE] / 100.0 }
    pub fn volume(&self) -> f64 { self.0[VOLUME] / 100.0 }
}
