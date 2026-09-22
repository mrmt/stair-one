//! AudioParam の setValueAtTime / setTargetAtTime 相当

use libm::exp;

/// setTargetAtTime の1サンプルあたりの係数
pub fn tc(tau: f64, sr: f64) -> f64 {
    if tau <= 0.0 { 1.0 } else { 1.0 - exp(-1.0 / (tau * sr)) }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Param {
    pub v: f64,
    target: f64,
    coef: f64,
    // 指定サンプル後に始まる setTargetAtTime (setValueAtTime → 少し後から減衰、の形に使う)
    pending: u32,
    p_target: f64,
    p_coef: f64,
}

impl Param {
    pub fn new(v: f64) -> Self {
        Param { v, target: v, ..Default::default() }
    }

    /// 予約済みの後の時刻の setTargetAtTime (set_target_after) は消さない (AudioParam のタイムラインと同じ)
    pub fn set_value(&mut self, v: f64) {
        self.v = v;
        self.target = v;
        self.coef = 0.0;
    }

    pub fn set_target(&mut self, target: f64, tau: f64, sr: f64) {
        self.target = target;
        self.coef = tc(tau, sr);
    }

    pub fn set_target_after(&mut self, delay: u32, target: f64, tau: f64, sr: f64) {
        if delay == 0 {
            return self.set_target(target, tau, sr);
        }
        // delay 回目の next() から減衰を始める
        self.pending = delay + 1;
        self.p_target = target;
        self.p_coef = tc(tau, sr);
    }

    /// このサンプルの値を返し、1サンプル進める (setTargetAtTime の開始時刻では開始値そのもの)
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        if self.pending > 0 {
            self.pending -= 1;
            if self.pending == 0 {
                self.target = self.p_target;
                self.coef = self.p_coef;
            }
        }
        let v = self.v;
        self.v += (self.target - self.v) * self.coef;
        v
    }
}
