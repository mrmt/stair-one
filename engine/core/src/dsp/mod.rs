//! Web Audio のノードに相当する部品。旧 JS 版の音を再現するため、挙動は Web Audio 仕様 / Chromium 実装に合わせる

pub mod biquad;
pub mod compressor;
pub mod delay;
pub mod osc;
pub mod param;
pub mod shaper;
pub mod wavetable;

pub const PI: f64 = core::f64::consts::PI;

#[inline]
pub fn clamp(x: f64, a: f64, b: f64) -> f64 {
    b.min(a.max(x))
}

#[inline]
pub fn frac(x: f64) -> f64 {
    x - libm::floor(x)
}

/// cent → 周波数比
#[inline]
pub fn cents(c: f64) -> f64 {
    libm::exp2(c / 1200.0)
}

/// StereoPanner (モノラル入力) の左右ゲイン
#[inline]
pub fn pan_gains(p: f64) -> (f64, f64) {
    let x = (clamp(p, -1.0, 1.0) + 1.0) * 0.5;
    (libm::cos(x * PI * 0.5), libm::sin(x * PI * 0.5))
}
