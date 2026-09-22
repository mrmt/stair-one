//! 16 パッチ (パッチ定義の正本)
//!   oct: 1=32′ (C0≒16.35Hz 基準) / 2=16′ / 4=8′ / 8=4′
//!   lfo.rate は Hz、lfo.depth は cent (noise は音量比、karplus は cent 換算)
//! パッチは「値」ではなく「値の分布」を持つ。発音ごとに `Num::sample` で確定させる

use crate::rng::Rng;

/// 数値の分布。F は固定値、R は範囲の一様乱数
#[derive(Clone, Copy, Debug)]
pub enum Num {
    F(f64),
    R(f64, f64),
}

impl Num {
    pub fn sample(&self, rng: &mut Rng) -> f64 {
        match *self {
            Num::F(x) => x,
            Num::R(a, b) => rng.rnd(a, b),
        }
    }
}

const fn f(x: f64) -> Num {
    Num::F(x)
}
const fn r(a: f64, b: f64) -> Num {
    Num::R(a, b)
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Wave {
    #[default]
    Sine,
    Square,
    Saw,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Shape {
    #[default]
    Sine,
    Square,
    Saw,
    /// sample & hold
    Sh,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FType {
    #[default]
    Lowpass,
    Bandpass,
    Highpass,
    Allpass,
}

#[derive(Clone, Copy, Debug)]
pub enum Scale {
    Minor,
    Whole,
    Chrom,
    Fifths,
    Dim,
    Odd,
}

impl Scale {
    pub fn degrees(self) -> &'static [f64] {
        match self {
            Scale::Minor => &[0.0, 3.0, 5.0, 7.0, 10.0],
            Scale::Whole => &[0.0, 2.0, 4.0, 6.0, 8.0, 10.0],
            Scale::Chrom => &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0],
            Scale::Fifths => &[0.0, 7.0],
            Scale::Dim => &[0.0, 3.0, 6.0, 9.0],
            Scale::Odd => &[0.0, 1.0, 6.0, 11.0],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    pub shape: &'static [Shape],
    pub rate: Num,
    pub depth: Num,
}

#[derive(Clone, Copy, Debug)]
pub struct Tape {
    pub rate: Num,
    pub depth: Num,
}

#[derive(Clone, Copy, Debug)]
pub enum Layer {
    Osc { wave: &'static [Wave], oct: Num, count: Num, spread: Num, level: Num, lfo: Option<Lfo> },
    Noise { level: Num, lfo: Option<Lfo> },
    Karplus { oct: Num, level: Num, excite: Num, pluck: Num, fb: Num, damp: Num, lfo: Option<Lfo> },
    Grain { level: Num, density: Num, dur: Num, rev: f64, spread: Num, rate: Num, wow: Tape, flutter: Tape },
}

#[derive(Clone, Copy, Debug)]
pub struct Drift {
    pub up: f64,
    pub down: f64,
    pub rate: Num,
    pub max: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Filter {
    pub ftype: &'static [FType],
    pub cut: Num,
    pub q: Num,
    /// 開く方向に進む確率
    pub open: f64,
    /// oct/秒
    pub sweep: Num,
    /// 2段直列になる確率
    pub series: f64,
    pub lfo: Option<Lfo>,
}

#[derive(Clone, Copy, Debug)]
pub struct Arp {
    pub p: f64,
    pub rate: Num,
    pub scale: &'static [Scale],
    pub range: Num,
}

#[derive(Clone, Copy, Debug)]
pub struct Patch {
    pub name: &'static str,
    pub root: Num,
    pub gain: f64,
    pub layers: &'static [Layer],
    pub drift: Drift,
    pub filter: Filter,
    pub arp: Option<Arp>,
    pub sus: Num,
}

use FType::*;
use Shape::{Sh, Sine as SSine, Saw as SSaw, Square as SSquare};
use Wave::{Saw, Square};

const ANY_LFO: &[Shape] = &[SSine, SSquare, SSaw, Sh];
// 旧版は drift / sweep が無いと R([0, 0]) を呼んで乱数を1回使う。乱数列を揃えるため r(0, 0) にしておく
const NO_DRIFT: Drift = Drift { up: 0.0, down: 0.0, rate: r(0.0, 0.0), max: 1200.0 };

const fn lfo(shape: &'static [Shape], rate: Num, depth: Num) -> Option<Lfo> {
    Some(Lfo { shape, rate, depth })
}
const fn drift(up: f64, down: f64, rate: Num) -> Drift {
    Drift { up, down, rate, max: 1200.0 }
}
const fn filt(ftype: &'static [FType], cut: Num, q: Num) -> Filter {
    Filter { ftype, cut, q, open: 0.5, sweep: r(0.0, 0.0), series: 0.0, lfo: None }
}
const fn osc(wave: &'static [Wave], oct: Num, count: Num, spread: Num, level: Num, lfo: Option<Lfo>) -> Layer {
    Layer::Osc { wave, oct, count, spread, level, lfo }
}

pub const PATCHES: [Patch; 16] = [
    Patch {
        name: "Sub Saw 32′", root: r(0.0, 7.0), gain: 0.8,
        layers: &[
            osc(&[Saw], f(1.0), f(3.0), r(4.0, 14.0), f(0.9), lfo(&[SSine, SSaw], r(1.0, 4.0), r(3.0, 15.0))),
            osc(&[Saw], f(2.0), f(2.0), r(6.0, 20.0), f(0.5), lfo(&[SSine, Sh], r(2.0, 7.0), r(4.0, 20.0))),
        ],
        drift: drift(0.3, 0.3, r(5.0, 30.0)),
        filter: Filter { sweep: r(0.05, 0.3), ..filt(&[Lowpass], r(90.0, 400.0), r(2.0, 6.0)) },
        arp: None, sus: r(0.7, 0.9),
    },
    Patch {
        name: "S&H Square 16′", root: r(0.0, 12.0), gain: 0.6,
        layers: &[
            osc(&[Square], f(2.0), f(2.0), r(2.0, 10.0), f(0.8), lfo(&[Sh], r(4.0, 14.0), r(200.0, 1200.0))),
        ],
        drift: drift(0.25, 0.25, r(10.0, 60.0)),
        filter: Filter { sweep: r(0.1, 0.5), ..filt(&[Lowpass], r(400.0, 1500.0), r(4.0, 10.0)) },
        arp: None, sus: r(0.6, 0.9),
    },
    // 高Qバンドパスで大半が削れるので、ゲインは他より大きく取る
    Patch {
        name: "Noise Rise", root: r(0.0, 12.0), gain: 12.0,
        layers: &[
            Layer::Noise { level: f(1.0), lfo: lfo(&[Sh, SSquare], r(1.0, 8.0), r(0.0, 0.5)) },
        ],
        drift: NO_DRIFT,
        filter: Filter { open: 0.85, sweep: r(0.3, 0.9), series: 0.5, ..filt(&[Bandpass], r(150.0, 500.0), r(8.0, 18.0)) },
        arp: None, sus: r(0.8, 1.0),
    },
    Patch {
        name: "Moiré 7", root: r(0.0, 12.0), gain: 0.5,
        layers: &[
            osc(&[Saw], f(4.0), f(7.0), r(8.0, 35.0), f(1.0), lfo(&[SSine, SSaw], r(1.0, 3.0), r(2.0, 10.0))),
        ],
        drift: drift(0.2, 0.2, r(3.0, 15.0)),
        filter: Filter { open: 0.2, sweep: r(0.05, 0.3), ..filt(&[Lowpass], r(800.0, 3000.0), r(1.0, 4.0)) },
        arp: None, sus: r(0.7, 0.9),
    },
    Patch {
        name: "Buzz 80Hz", root: r(0.0, 12.0), gain: 0.55,
        layers: &[
            osc(&[Square], f(2.0), f(2.0), r(3.0, 10.0), f(0.7), lfo(&[SSquare], r(60.0, 100.0), r(300.0, 1200.0))),
            osc(&[Saw], f(1.0), f(1.0), f(0.0), f(0.6), lfo(&[SSaw], r(30.0, 90.0), r(100.0, 600.0))),
        ],
        drift: NO_DRIFT,
        filter: Filter { sweep: r(0.05, 0.4), ..filt(&[Lowpass], r(600.0, 2500.0), r(3.0, 9.0)) },
        arp: None, sus: r(0.6, 0.9),
    },
    Patch {
        name: "Random Arp", root: r(0.0, 12.0), gain: 0.55,
        layers: &[
            osc(&[Square], f(8.0), f(2.0), r(3.0, 9.0), f(0.8), lfo(&[SSine], r(4.0, 8.0), r(5.0, 30.0))),
        ],
        drift: NO_DRIFT,
        filter: Filter { sweep: r(0.1, 0.6), ..filt(&[Lowpass], r(700.0, 2500.0), r(6.0, 14.0)) },
        arp: Some(Arp { p: 1.0, rate: r(6.0, 16.0), scale: &[Scale::Minor, Scale::Whole, Scale::Chrom, Scale::Fifths, Scale::Odd], range: r(1.0, 3.99) }),
        sus: r(0.7, 1.0),
    },
    Patch {
        name: "Falling Res", root: r(0.0, 12.0), gain: 0.4,
        layers: &[
            osc(&[Saw], f(4.0), f(3.0), r(5.0, 15.0), f(0.8), lfo(&[SSaw], r(1.0, 5.0), r(10.0, 60.0))),
        ],
        drift: Drift { up: 0.0, down: 1.0, rate: r(60.0, 240.0), max: 3600.0 },
        filter: Filter { open: 0.1, sweep: r(0.2, 0.7), series: 0.6, ..filt(&[Lowpass], r(1500.0, 5000.0), r(18.0, 28.0)) },
        arp: None, sus: r(0.6, 0.9),
    },
    Patch {
        name: "Siren", root: r(0.0, 12.0), gain: 2.5,
        layers: &[
            osc(&[Square], f(8.0), f(2.0), r(10.0, 30.0), f(0.7), lfo(&[SSine], r(1.0, 6.0), r(100.0, 700.0))),
            Layer::Noise { level: f(0.3), lfo: None },
        ],
        drift: Drift { up: 1.0, down: 0.0, rate: r(80.0, 300.0), max: 3600.0 },
        filter: Filter { sweep: r(0.05, 0.3), ..filt(&[Bandpass], r(800.0, 2000.0), r(3.0, 8.0)) },
        arp: None, sus: r(0.7, 1.0),
    },
    Patch {
        name: "Comb Metal", root: r(0.0, 12.0), gain: 2.5,
        layers: &[
            Layer::Karplus { oct: f(8.0), level: f(0.25), excite: r(0.01, 0.04), pluck: r(0.5, 3.0), fb: r(0.97, 0.995), damp: r(3000.0, 9000.0), lfo: lfo(&[SSine], r(1.0, 6.0), r(3.0, 20.0)) },
            Layer::Karplus { oct: r(11.3, 12.7), level: f(0.2), excite: r(0.01, 0.03), pluck: r(0.3, 2.0), fb: r(0.96, 0.99), damp: r(4000.0, 10000.0), lfo: None },
        ],
        drift: NO_DRIFT,
        filter: filt(&[Highpass], r(80.0, 200.0), r(1.0, 3.0)),
        arp: None, sus: r(0.8, 1.0),
    },
    Patch {
        name: "Bowed Comb", root: r(0.0, 12.0), gain: 14.0,
        layers: &[
            Layer::Karplus { oct: f(4.0), level: f(0.4), excite: r(0.08, 0.2), pluck: f(0.0), fb: r(0.9, 0.98), damp: r(1500.0, 5000.0), lfo: lfo(&[Sh], r(1.0, 4.0), r(5.0, 40.0)) },
        ],
        drift: drift(0.5, 0.5, r(10.0, 60.0)),
        filter: Filter { sweep: r(0.05, 0.4), ..filt(&[Bandpass], r(300.0, 1200.0), r(2.0, 6.0)) },
        arp: None, sus: r(0.8, 1.0),
    },
    Patch {
        name: "Tape Cloud", root: r(0.0, 12.0), gain: 1.2,
        layers: &[
            Layer::Grain { level: f(0.9), density: r(15.0, 40.0), dur: r(0.04, 0.18), rev: 0.2, spread: r(0.0, 700.0), rate: r(0.7, 1.2),
                wow: Tape { rate: r(0.3, 1.5), depth: r(20.0, 80.0) }, flutter: Tape { rate: r(6.0, 14.0), depth: r(5.0, 25.0) } },
        ],
        drift: NO_DRIFT,
        filter: Filter { sweep: r(0.05, 0.3), ..filt(&[Lowpass], r(1500.0, 6000.0), r(1.0, 3.0)) },
        arp: None, sus: r(0.8, 1.0),
    },
    Patch {
        name: "Reverse Flutter", root: r(0.0, 12.0), gain: 2.5,
        layers: &[
            Layer::Grain { level: f(0.9), density: r(6.0, 18.0), dur: r(0.1, 0.4), rev: 0.9, spread: r(0.0, 1200.0), rate: r(0.5, 1.0),
                wow: Tape { rate: r(1.0, 3.0), depth: r(50.0, 200.0) }, flutter: Tape { rate: r(10.0, 20.0), depth: r(20.0, 60.0) } },
        ],
        drift: NO_DRIFT,
        filter: Filter { sweep: r(0.05, 0.3), lfo: lfo(&[Sh], r(2.0, 8.0), r(300.0, 1200.0)), ..filt(&[Bandpass], r(500.0, 2000.0), r(2.0, 6.0)) },
        arp: None, sus: r(0.8, 1.0),
    },
    // 出力段のコンプで潰れるため、ゲインの比では音量が下がらない。出力 RMS の実測で旧 1.1 の約70%になる値
    Patch {
        name: "S&H Filter Noise", root: r(0.0, 12.0), gain: 0.47,
        layers: &[
            Layer::Noise { level: f(1.0), lfo: None },
        ],
        drift: NO_DRIFT,
        filter: Filter { series: 0.7, sweep: r(0.0, 0.2), lfo: lfo(&[Sh], r(5.0, 20.0), r(800.0, 2400.0)), ..filt(&[Lowpass], r(400.0, 2000.0), r(12.0, 22.0)) },
        arp: None, sus: r(0.7, 1.0),
    },
    Patch {
        name: "Growl 32′", root: r(0.0, 7.0), gain: 0.6,
        layers: &[
            osc(&[Square], f(1.0), f(2.0), r(2.0, 8.0), f(0.8), lfo(&[SSaw], r(25.0, 55.0), r(200.0, 900.0))),
            osc(&[Saw], f(2.0), f(2.0), r(4.0, 12.0), f(0.6), lfo(&[Sh], r(1.0, 5.0), r(20.0, 100.0))),
        ],
        drift: drift(0.3, 0.3, r(5.0, 40.0)),
        filter: Filter { sweep: r(0.1, 0.6), series: 0.5, ..filt(&[Lowpass], r(120.0, 600.0), r(6.0, 14.0)) },
        arp: None, sus: r(0.7, 0.9),
    },
    Patch {
        name: "Arp Comb", root: r(0.0, 12.0), gain: 1.4,
        layers: &[
            Layer::Karplus { oct: f(8.0), level: f(0.22), excite: r(0.01, 0.03), pluck: r(4.0, 10.0), fb: r(0.95, 0.99), damp: r(2500.0, 7000.0), lfo: None },
            osc(&[Square], f(4.0), f(1.0), f(0.0), f(0.25), lfo(&[Sh], r(3.0, 12.0), r(5.0, 40.0))),
        ],
        drift: NO_DRIFT,
        filter: Filter { sweep: r(0.05, 0.4), ..filt(&[Lowpass], r(1000.0, 4000.0), r(2.0, 8.0)) },
        arp: Some(Arp { p: 1.0, rate: r(4.0, 12.0), scale: &[Scale::Minor, Scale::Fifths, Scale::Dim, Scale::Odd], range: r(1.0, 2.99) }),
        sus: r(0.7, 1.0),
    },
    Patch {
        name: "Chaos", root: r(0.0, 12.0), gain: 1.2,
        layers: &[
            osc(&[Saw, Square], r(1.0, 3.0), r(1.0, 4.99), r(0.0, 60.0), f(0.6), lfo(ANY_LFO, r(1.0, 100.0), r(0.0, 1500.0))),
            osc(&[Saw, Square], r(2.0, 8.0), r(1.0, 4.99), r(0.0, 60.0), f(0.5), lfo(ANY_LFO, r(1.0, 100.0), r(0.0, 1500.0))),
            Layer::Noise { level: r(0.0, 0.5), lfo: lfo(ANY_LFO, r(1.0, 30.0), r(0.0, 1.0)) },
            Layer::Karplus { oct: r(3.0, 12.0), level: f(0.15), excite: r(0.01, 0.05), pluck: r(0.0, 6.0), fb: r(0.9, 0.99), damp: r(1000.0, 8000.0), lfo: None },
            Layer::Grain { level: f(0.5), density: r(5.0, 30.0), dur: r(0.03, 0.3), rev: 0.5, spread: r(0.0, 1500.0), rate: r(0.5, 1.5),
                wow: Tape { rate: r(0.3, 3.0), depth: r(0.0, 150.0) }, flutter: Tape { rate: r(6.0, 20.0), depth: r(0.0, 60.0) } },
        ],
        drift: Drift { up: 0.35, down: 0.35, rate: r(10.0, 300.0), max: 3600.0 },
        filter: Filter { series: 0.5, sweep: r(0.0, 1.0), ..filt(&[Lowpass, Bandpass, Highpass], r(200.0, 4000.0), r(1.0, 20.0)) },
        arp: Some(Arp { p: 0.5, rate: r(3.0, 18.0), scale: &[Scale::Chrom, Scale::Odd, Scale::Whole], range: r(1.0, 3.99) }),
        sus: r(0.4, 1.0),
    },
];
