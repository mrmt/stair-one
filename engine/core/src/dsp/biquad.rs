//! BiquadFilterNode。係数は Web Audio 仕様 (Chromium Biquad.cpp) と同じ式
//! lowpass / highpass の Q は dB、bandpass / allpass の Q は線形

use super::PI;
use crate::patches::FType;
use libm::{cos, pow, sin};

#[derive(Clone, Copy, Default, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// freq は detune 適用後の Hz
    pub fn set(&mut self, t: FType, freq: f64, q: f64, sr: f64) {
        let nyq = sr * 0.5;
        let c = (freq / nyq).clamp(0.0, 1.0);
        let (b0, b1, b2, a0, a1, a2) = match t {
            FType::Lowpass => {
                if c >= 1.0 {
                    (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else if c <= 0.0 {
                    (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else {
                    let w0 = PI * c;
                    let alpha = 0.5 * sin(w0) * pow(10.0, -0.05 * q);
                    let k = cos(w0);
                    ((1.0 - k) * 0.5, 1.0 - k, (1.0 - k) * 0.5, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                }
            }
            FType::Highpass => {
                if c >= 1.0 {
                    (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else if c <= 0.0 {
                    (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else {
                    let w0 = PI * c;
                    let alpha = 0.5 * sin(w0) * pow(10.0, -0.05 * q);
                    let k = cos(w0);
                    ((1.0 + k) * 0.5, -(1.0 + k), (1.0 + k) * 0.5, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                }
            }
            FType::Bandpass => {
                if c > 0.0 && c < 1.0 {
                    if q > 0.0 {
                        let w0 = PI * c;
                        let alpha = sin(w0) / (2.0 * q);
                        let k = cos(w0);
                        (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                    } else {
                        (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                    }
                } else {
                    (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                }
            }
            FType::Allpass => {
                if c > 0.0 && c < 1.0 {
                    if q > 0.0 {
                        let w0 = PI * c;
                        let alpha = sin(w0) / (2.0 * q);
                        let k = cos(w0);
                        (1.0 - alpha, -2.0 * k, 1.0 + alpha, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                    } else {
                        (-1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                    }
                } else {
                    (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                }
            }
        };
        let n = 1.0 / a0;
        self.b0 = b0 * n;
        self.b1 = b1 * n;
        self.b2 = b2 * n;
        self.a1 = a1 * n;
        self.a2 = a2 * n;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        // 転置直接形 II
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}
