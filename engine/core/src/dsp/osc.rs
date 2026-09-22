//! オシレータ。波形は Chromium と同じ帯域制限テーブル (wavetable.rs) から読む
//! 位相と形は Web Audio の OscillatorNode に合わせる (sawtooth は 0 から上昇、square は +1 から)

use super::frac;
use super::wavetable::Wavetables;
use crate::patches::Wave;

#[derive(Clone, Copy, Default, Debug)]
pub struct Osc {
    pub phase: f64,
}

impl Osc {
    /// freq は Hz (detune 適用後)
    #[inline]
    pub fn next(&mut self, wt: &Wavetables, wave: Wave, freq: f64, sr: f64) -> f64 {
        let y = wt.sample(wave, self.phase, freq.abs());
        self.phase = frac(self.phase + freq / sr);
        y
    }
}
