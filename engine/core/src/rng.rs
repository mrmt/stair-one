//! シード付き乱数。旧 JS 版の Math.random / rnd / gauss / R() / W() に対応する

use libm::{cos, log, sqrt};

/// mulberry32。旧 JS 版を再現検証するとき Math.random を同じ生成器に差し替えるので、それに合わせる
pub struct Rng {
    a: u32,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { a: seed as u32 }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.a = self.a.wrapping_add(0x6D2B_79F5);
        let mut t = self.a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    /// [0, 1) の一様乱数 (Math.random 相当)
    pub fn random(&mut self) -> f64 {
        self.next_u32() as f64 / 4294967296.0
    }

    /// [a, b) の一様乱数
    pub fn rnd(&mut self, a: f64, b: f64) -> f64 {
        a + self.random() * (b - a)
    }

    /// 標準正規分布 (Box-Muller)
    pub fn gauss(&mut self) -> f64 {
        let mut u = 0.0;
        while u == 0.0 {
            u = self.random();
        }
        sqrt(-2.0 * log(u)) * cos(2.0 * core::f64::consts::PI * self.random())
    }

    /// 配列から1つ選ぶ。旧版の R() と同じく、候補が1つなら乱数を使わない (JS では配列でなく文字列だった値)
    pub fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        if xs.len() == 1 {
            return xs[0];
        }
        let i = (self.random() * xs.len() as f64) as usize;
        xs[i.min(xs.len() - 1)]
    }
}

/// 平均回帰つきランダムウォーク。発音中の「少しの揺れ」は全部これで作る
#[derive(Clone, Copy, Default)]
pub struct Walk {
    pub base: f64,
    pub v: f64,
    spread: f64,
    revert: f64,
}

impl Walk {
    pub fn new(base: f64, spread: f64, revert: f64) -> Self {
        Walk { base, v: base, spread, revert }
    }

    pub fn step(&mut self, dt: f64, rng: &mut Rng) -> f64 {
        let k = (self.revert * dt).min(1.0);
        self.v += (self.base - self.v) * k + rng.gauss() * self.spread * sqrt(dt);
        self.v
    }
}
