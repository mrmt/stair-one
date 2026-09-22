//! DynamicsCompressorNode。Chromium の DynamicsCompressorKernel を移植 (ステレオ連動、先読み 6ms)
//! 旧 JS 版はパッチの音量をこのコンプの潰れ方込みで決めているので、挙動を揃える

use libm::{asin, exp, log10, pow, sin, sqrt};

const PI_2: f64 = core::f64::consts::FRAC_PI_2;
const DIVISION: usize = 32;
const MAX_PRE_DELAY: usize = 1024;
const MASK: usize = MAX_PRE_DELAY - 1;
const SPACING_DB: f64 = 5.0;

fn db2lin(db: f64) -> f64 {
    pow(10.0, 0.05 * db)
}
fn lin2db(x: f64) -> f64 {
    20.0 * log10(x)
}

pub struct Compressor {
    sr: f64,
    // 静的カーブ
    linear_threshold: f64,
    db_threshold: f64,
    db_knee: f64,
    knee_threshold: f64,
    knee_threshold_db: f64,
    ykt_db: f64,
    slope: f64,
    k: f64,
    master_gain: f64,
    // エンベロープ
    attack_frames: f64,
    sat_release_frames: f64,
    kr: [f64; 5],
    detector_average: f64,
    compressor_gain: f64,
    max_attack_diff_db: f64,
    // 先読み
    pre: [[f64; MAX_PRE_DELAY]; 2],
    r: usize,
    w: usize,
    // 32 サンプル単位で決まる値
    phase: usize,
    envelope_rate: f64,
    scaled_desired: f64,
}

impl Compressor {
    pub fn new(sr: f64, threshold: f64, knee: f64, ratio: f64, attack: f64, release: f64) -> Box<Self> {
        let mut c = Box::new(Compressor {
            sr,
            linear_threshold: 0.0,
            db_threshold: threshold,
            db_knee: knee,
            knee_threshold: 0.0,
            knee_threshold_db: 0.0,
            ykt_db: 0.0,
            slope: 1.0 / ratio,
            k: 0.0,
            master_gain: 1.0,
            attack_frames: attack.max(0.001) * sr,
            sat_release_frames: 0.0025 * sr,
            kr: [0.0; 5],
            detector_average: 0.0,
            compressor_gain: 1.0,
            max_attack_diff_db: -1.0,
            pre: [[0.0; MAX_PRE_DELAY]; 2],
            r: 0,
            w: 0,
            phase: 0,
            envelope_rate: 1.0,
            scaled_desired: 1.0,
        });
        c.linear_threshold = db2lin(threshold);
        c.k = c.k_at_slope(1.0 / ratio);
        c.knee_threshold_db = threshold + knee;
        c.knee_threshold = db2lin(c.knee_threshold_db);
        c.ykt_db = lin2db(c.knee_curve(c.knee_threshold, c.k));
        let full_range_gain = c.saturate(1.0, c.k);
        c.master_gain = pow(1.0 / full_range_gain, 0.6);

        // 4点を通る適応リリース曲線
        let rf = sr * release;
        let (y1, y2, y3, y4) = (rf * 0.09, rf * 0.16, rf * 0.42, rf * 0.98);
        c.kr = [
            0.9999999999999998 * y1 + 1.8432219684323923e-16 * y2 - 1.9373394351676423e-16 * y3 + 8.824516011816245e-18 * y4,
            -1.5788320352845888 * y1 + 2.3305837032074286 * y2 - 0.9141194204840429 * y3 + 0.1623677525612032 * y4,
            0.5334142869106424 * y1 - 1.272736789213631 * y2 + 0.9258856042207512 * y3 - 0.18656310191776226 * y4,
            0.08783463138207234 * y1 - 0.1694162967925622 * y2 + 0.08588057951595272 * y3 - 0.00429891410546283 * y4,
            -0.042416883008123074 * y1 + 0.1115693827987602 * y2 - 0.09764676325265872 * y3 + 0.028494263462021576 * y4,
        ];
        // 先読み 6ms
        let pre_frames = ((0.006 * sr) as usize).min(MAX_PRE_DELAY - 1);
        c.r = (MAX_PRE_DELAY - pre_frames) & MASK;
        c
    }

    fn knee_curve(&self, x: f64, k: f64) -> f64 {
        if x < self.linear_threshold {
            return x;
        }
        self.linear_threshold + (1.0 - exp(-k * (x - self.linear_threshold))) / k
    }

    fn slope_at(&self, x: f64, k: f64) -> f64 {
        if x < self.linear_threshold {
            return 1.0;
        }
        let x2 = x * 1.001;
        let (xd, x2d) = (lin2db(x), lin2db(x2));
        let (yd, y2d) = (lin2db(self.knee_curve(x, k)), lin2db(self.knee_curve(x2, k)));
        (y2d - yd) / (x2d - xd)
    }

    fn k_at_slope(&self, desired: f64) -> f64 {
        let x = db2lin(self.db_threshold + self.db_knee);
        let (mut min_k, mut max_k, mut k) = (0.1, 10000.0, 5.0);
        for _ in 0..15 {
            if self.slope_at(x, k) < desired {
                max_k = k;
            } else {
                min_k = k;
            }
            k = sqrt(min_k * max_k);
        }
        k
    }

    fn saturate(&self, x: f64, k: f64) -> f64 {
        if x < self.knee_threshold {
            self.knee_curve(x, k)
        } else {
            let xd = lin2db(x);
            db2lin(self.ykt_db + self.slope * (xd - self.knee_threshold_db))
        }
    }

    /// 32 サンプルごとに目標ゲインと追従速度を決める
    fn update_envelope(&mut self) {
        if !self.detector_average.is_finite() {
            self.detector_average = 1.0;
        }
        let desired = self.detector_average;
        let scaled = asin(desired) / PI_2;
        let releasing = scaled > self.compressor_gain;
        let mut diff_db = lin2db(self.compressor_gain / scaled);
        if releasing {
            self.max_attack_diff_db = -1.0;
            if !diff_db.is_finite() {
                diff_db = -1.0;
            }
            let x = 0.25 * (diff_db.clamp(-12.0, 0.0) + 12.0);
            let (x2, x3, x4) = (x * x, x * x * x, x * x * x * x);
            let [a, b, c, d, e] = self.kr;
            let release_frames = a + b * x + c * x2 + d * x3 + e * x4;
            self.envelope_rate = db2lin(SPACING_DB / release_frames);
        } else {
            if !diff_db.is_finite() {
                diff_db = 1.0;
            }
            if self.max_attack_diff_db == -1.0 || self.max_attack_diff_db < diff_db {
                self.max_attack_diff_db = diff_db;
            }
            let eff = self.max_attack_diff_db.max(0.5);
            self.envelope_rate = 1.0 - pow(0.25 / eff, 1.0 / self.attack_frames);
        }
        self.scaled_desired = scaled;
    }

    #[inline]
    pub fn process(&mut self, l: f64, r: f64) -> (f64, f64) {
        if self.phase == 0 {
            self.update_envelope();
        }
        self.phase = (self.phase + 1) % DIVISION;

        self.pre[0][self.w] = l;
        self.pre[1][self.w] = r;
        let abs_in = l.abs().max(r.abs());

        let shaped = self.saturate(abs_in, self.k);
        let attenuation = if abs_in <= 0.0001 { 1.0 } else { shaped / abs_in };
        let att_db = (-lin2db(attenuation)).max(2.0);
        let sat_release_rate = db2lin(att_db / self.sat_release_frames) - 1.0;
        let rate = if attenuation > self.detector_average { sat_release_rate } else { 1.0 };
        self.detector_average += (attenuation - self.detector_average) * rate;
        self.detector_average = self.detector_average.min(1.0);
        if !self.detector_average.is_finite() {
            self.detector_average = 1.0;
        }

        if self.envelope_rate < 1.0 {
            self.compressor_gain += (self.scaled_desired - self.compressor_gain) * self.envelope_rate;
        } else {
            self.compressor_gain = (self.compressor_gain * self.envelope_rate).min(1.0);
        }
        let g = self.master_gain * sin(PI_2 * self.compressor_gain);

        let out = (self.pre[0][self.r] * g, self.pre[1][self.r] * g);
        self.r = (self.r + 1) & MASK;
        self.w = (self.w + 1) & MASK;
        out
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }
}
