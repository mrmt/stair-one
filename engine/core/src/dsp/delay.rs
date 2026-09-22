//! DelayNode。線形補間で小数サンプルの遅延を読む

#[derive(Clone, Debug, Default)]
pub struct Delay {
    buf: Vec<f64>,
    mask: usize,
    w: usize,
}

impl Delay {
    /// len サンプル以上の遅延を持てるリングバッファ (作成時だけメモリ確保)
    pub fn new(len: usize) -> Self {
        let n = len.max(2).next_power_of_two();
        Delay { buf: vec![0.0; n], mask: n - 1, w: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
        self.w = 0;
    }

    /// 次に書く位置から d サンプル前の値。d >= 1
    #[inline]
    pub fn read(&self, d: f64) -> f64 {
        let n = self.buf.len();
        let d = d.clamp(1.0, (n - 2) as f64);
        let pos = (self.w + n) as f64 - d;
        let i = pos as usize;
        let f = pos - i as f64;
        let a = self.buf[i & self.mask];
        let b = self.buf[(i + 1) & self.mask];
        a + (b - a) * f
    }

    #[inline]
    pub fn push(&mut self, x: f64) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) & self.mask;
    }
}

/// 1 描画単位 (128 サンプル) の遅れ。Chromium は閉路を「処理中のノードの前回の出力」で断ち切るので、
/// 閉路の中の 1 本の辺だけ 128 サンプル遅れる (DelayNode の遅延時間とは別に足される)
#[derive(Clone, Copy)]
pub struct Quantum {
    buf: [f64; 128],
    i: usize,
}

impl Default for Quantum {
    fn default() -> Self {
        Quantum { buf: [0.0; 128], i: 0 }
    }
}

impl Quantum {
    /// x を入れ、128 サンプル前の値を返す
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = core::mem::replace(&mut self.buf[self.i], x);
        self.i = (self.i + 1) & 127;
        y
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
