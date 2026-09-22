//! Chromium との部品単位の一致確認 (tools/parity.mjs から呼ぶ)
//!   stdin: {"case": "...", "sr": 44100, "n": 44100, "input": [...], "input2": [...]}
//!   stdout: {"out": [...], "out2": [...]}
use stair_core::dsp::biquad::Biquad;
use stair_core::dsp::compressor::Compressor;
use stair_core::dsp::delay::Delay;
use stair_core::dsp::osc::Osc;
use stair_core::dsp::param::Param;
use stair_core::dsp::shaper::{drive_curve, Oversampled4x, DRIVE_LEN};
use stair_core::dsp::wavetable::Wavetables;
use stair_core::fx::Fx;
use stair_core::params::{index_of, Values, PARAMS};
use stair_core::patches::{FType, Wave};
use std::io::Read;

fn nums(v: &serde_json::Value, k: &str) -> Vec<f64> {
    v[k].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap()).collect()).unwrap_or_default()
}

fn main() {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    let case = v["case"].as_str().unwrap();
    let sr = v["sr"].as_f64().unwrap();
    let n = v["n"].as_u64().unwrap() as usize;
    let x = nums(&v, "input");
    let x2 = nums(&v, "input2");
    let mut out2 = vec![];
    let out: Vec<f64> = match case {
        "saw110" | "square440" | "saw3000" | "sawsweep" => {
            let wt = Wavetables::new(sr);
            let mut o = Osc::default();
            let (wave, f0) = match case { "saw110" => (Wave::Saw, 110.0), "square440" => (Wave::Square, 440.0), "saw3000" => (Wave::Saw, 3000.0), _ => (Wave::Saw, 50.0) };
            (0..n).map(|i| {
                // sawsweep: 50Hz → 5000Hz を指数で
                let f = if case == "sawsweep" { f0 * 100f64.powf(i as f64 / n as f64) } else { f0 };
                o.next(&wt, wave, f, sr)
            }).collect()
        }
        "lp" | "hp" | "bp" | "ap" | "lpq" => {
            let (t, f, q) = match case {
                "lp" => (FType::Lowpass, 800.0, 6.0),
                "lpq" => (FType::Lowpass, 3000.0, -6.0),
                "hp" => (FType::Highpass, 150.0, 2.0),
                "bp" => (FType::Bandpass, 1000.0, 5.0),
                _ => (FType::Allpass, 900.0, 0.7),
            };
            let mut b = Biquad::default();
            b.set(t, f, q, sr);
            x.iter().map(|v| b.process(*v)).collect()
        }
        "comp" => {
            let mut c = Compressor::new(sr, -18.0, 12.0, 6.0, 0.005, 0.2);
            let mut o = vec![];
            for (a, b) in x.iter().zip(&x2) {
                let (l, r) = c.process(*a, *b);
                o.push(l);
                out2.push(r);
            }
            o
        }
        "delay" => {
            // delayTime: 0.005 から setTargetAtTime(0.02, 0, 0.05)
            let mut d = Delay::new(4096);
            let mut p = Param::new(0.005);
            p.set_target(0.02, 0.05, sr);
            x.iter().map(|v| {
                let t = p.v * sr;
                p.next();
                // Chromium は書いてから読む (遅延 0 も可)。1 サンプル以上ならこの順と同じ
                let y = d.read(t);
                d.push(*v);
                y
            }).collect()
        }
        "fxchain" | "fxchain_lf" | "fxdelay" => {
            // 既定のつまみ。fxchain はディレイを切る。fxdelay はディレイタイムを揺らさず固定
            let mut p = PARAMS.map(|d| d.default);
            if case != "fxdelay" {
                p[index_of("dmix").unwrap()] = 0.0;
                p[index_of("dfb").unwrap()] = 0.0;
            }
            let mut fx = Fx::new(sr);
            fx.apply(&Values(&p));
            let dtm = Values(&p).dtime();
            fx.jump_delay_times(dtm * 0.9, dtm * 0.62 * 0.9);
            fx.set_delay_times(dtm, dtm * 0.62);
            let mut o = vec![];
            for (a, b) in x.iter().zip(&x2) {
                let (l, r) = fx.process(*a, *b, true);
                o.push(l);
                out2.push(r);
            }
            o
        }
        "karplus" | "karplus2" | "karplus3" => {
            // ノイズ → Delay(1/f) → LPF(damp, Q -6dB) → tanh 飽和 → fb → Delay
            // karplus3 は遅延時間を 50ms 後から 1/205 秒へ setTargetAtTime で動かす
            let f = if case == "karplus2" { 450.0 } else { 200.0 };
            let mut dt = Param::new(1.0 / f);
            let start = (0.05 * sr) as usize;
            let mut dl = Delay::new(65536);
            let mut q = stair_core::dsp::delay::Quantum::default();
            let mut lp = Biquad::default();
            lp.set(FType::Lowpass, 5000.0, -6.0, sr);
            let sat: Vec<f64> = (0..1025).map(|i| ((i as f64 / 512.0 - 1.0).tanh()) as f32 as f64).collect();
            x.iter().enumerate().map(|(i, v)| {
                if case == "karplus3" && i == start {
                    dt.set_target(1.0 / 205.0, 0.02, sr);
                }
                let y = dl.read(dt.next() * sr);
                let fb = 0.98 * stair_core::dsp::shaper::lookup(&sat, lp.process(q.process(y)));
                dl.push(v + fb);
                y
            }).collect()
        }
        "delay2" => {
            let mut d = Delay::new((2.5 * sr) as usize + 4);
            let mut p = Param::new(0.18);
            p.set_target(0.2, 0.2, sr);
            x.iter().map(|v| { let t = p.next() * sr; let y = d.read(t); d.push(*v); y }).collect()
        }
        "delayc" => {
            let mut d = Delay::new(4096);
            x.iter().map(|v| { let y = d.read(0.0101 * sr); d.push(*v); y }).collect()
        }
        "shaper" => {
            let mut c = [0.0; DRIVE_LEN];
            drive_curve(0.15, &mut c);
            let mut os = Oversampled4x::default();
            x.iter().map(|v| 0.55 * v + 0.225 * os.process(*v, &c)).collect()
        }
        _ => panic!("unknown case {case}"),
    };
    println!("{}", serde_json::json!({ "out": out, "out2": out2 }));
}
