//! 試聴・評価用 CLI
//!
//!   render play --pad 3 --hold 2 --tail 1.5 --seed 42 --param drive=60 --out out/
//!   render play --all --takes 3 --out out/          16 パッド × 3 シード
//!   render score song.json --out out/song           複数音の演奏列
//!   render analyze foo.wav --hold 2 --out out/foo   既存 WAV (旧版の録音など) を同じ指標で評価
//!   render compare reference/legacy out/            パッドごとに指標を並べる
//!
//! 出力は <name>.wav / <name>.png (波形 + スペクトログラム) / <name>.json (指標)

mod analyze;

use serde::Deserialize;
use stair_core::params::{index_of, PARAMS};
use stair_core::patches::PATCHES;
use stair_core::fx::Tap;
use stair_core::Engine;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "usage:
  render play (--pad N | --all) [--hold S] [--tail S] [--seed N] [--takes K] [--sr HZ] [--param id=v]... [--out DIR]
  render score FILE.json [--seed N] [--sr HZ] [--out PREFIX]
  render analyze FILE.wav [--hold S] [--out PREFIX]
  render compare DIR_A DIR_B
  render meta                     パッド名とつまみ定義を JSON で出す

pads (1-16):"
    );
    for (i, p) in PATCHES.iter().enumerate() {
        eprintln!("  {:2} {}", i + 1, p.name);
    }
    eprintln!("params:");
    for d in PARAMS {
        eprintln!("  {:8} {}..{} (default {})", d.id, d.min, d.max, d.default);
    }
    std::process::exit(2)
}

struct Args {
    pos: Vec<String>,
    opts: BTreeMap<String, Vec<String>>,
}

impl Args {
    fn parse() -> Args {
        let mut pos = vec![];
        let mut opts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut it = std::env::args().skip(1).peekable();
        while let Some(a) = it.next() {
            if let Some(k) = a.strip_prefix("--") {
                let flag = matches!(k, "all");
                let v = if flag { String::new() } else { it.next().unwrap_or_else(|| usage()) };
                opts.entry(k.to_string()).or_default().push(v);
            } else {
                pos.push(a);
            }
        }
        Args { pos, opts }
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.opts.get(k).and_then(|v| v.last()).map(|s| s.as_str())
    }
    fn num(&self, k: &str, d: f64) -> f64 {
        self.get(k).map(|s| s.parse().unwrap_or_else(|_| usage())).unwrap_or(d)
    }
    fn has(&self, k: &str) -> bool {
        self.opts.contains_key(k)
    }
}

struct Take {
    l: Vec<f32>,
    r: Vec<f32>,
    sr: u32,
    /// (区間名, 開始, 終了)
    sections: Vec<(String, f64, f64)>,
    /// 押下 / 離鍵の時刻
    marks: Vec<f64>,
}

fn apply_params(e: &mut Engine, params: &[(String, f64)]) {
    for (id, v) in params {
        match index_of(id) {
            Some(i) => e.set_param(i, *v),
            None => {
                eprintln!("unknown param: {id}");
                usage()
            }
        }
    }
}

fn parse_params(a: &Args) -> Vec<(String, f64)> {
    a.opts
        .get("param")
        .map(|v| {
            v.iter()
                .map(|s| {
                    let (k, x) = s.split_once('=').unwrap_or_else(|| usage());
                    (k.to_string(), x.parse().unwrap_or_else(|_| usage()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// --tap voice|dist|crush|phaser: 途中の段を録る (旧版の capture-legacy --tap と比べる検証用)
static TAP: std::sync::Mutex<Tap> = std::sync::Mutex::new(Tap::Out);

/// 起動直後はディレイタイムが 0 から伸びていく途中なので、この秒数だけ空回ししてから録る
const PREROLL: f64 = 1.5;

/// 時刻順のイベント列を鳴らす
fn run(sr: u32, seed: u64, params: &[(String, f64)], events: &[(f64, usize, bool)], length: f64) -> (Vec<f32>, Vec<f32>) {
    let mut e = Engine::new(sr as f64, seed);
    e.set_tap(*TAP.lock().unwrap());
    apply_params(&mut e, params);
    // 128 サンプル境界に切り上げる (旧版の harness と同じ)
    let pre = (PREROLL * sr as f64 / 128.0).ceil() as usize;
    let (mut pl, mut pr) = (vec![0f32; 128], vec![0f32; 128]);
    for _ in 0..pre {
        e.process(&mut pl, &mut pr);
    }
    let total = (length * sr as f64) as usize;
    let (mut l, mut r) = (vec![0f32; total], vec![0f32; total]);
    let mut ev: Vec<_> = events.to_vec();
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut pos = 0;
    // Web 版と同じく 128 サンプル単位でイベントを受け付ける
    let block = 128;
    let mut k = 0;
    while pos < total {
        let t = pos as f64 / sr as f64;
        while k < ev.len() && ev[k].0 <= t {
            let (_, pad, on) = ev[k];
            if on { e.note_on(pad) } else { e.note_off(pad) }
            k += 1;
        }
        let n = block.min(total - pos);
        e.process(&mut l[pos..pos + n], &mut r[pos..pos + n]);
        pos += n;
    }
    (l, r)
}

fn play_one(pad: usize, seed: u64, a: &Args) -> Take {
    let sr = a.num("sr", 48000.0) as u32;
    let hold = a.num("hold", 2.0);
    let tail = a.num("tail", 1.5);
    let params = parse_params(a);
    let (l, r) = run(sr, seed, &params, &[(0.0, pad, true), (hold, pad, false)], hold + tail);
    Take {
        l,
        r,
        sr,
        sections: vec![("attack".into(), 0.0, 0.3f64.min(hold)), ("hold".into(), 0.3f64.min(hold), hold), ("release".into(), hold, hold + tail)],
        marks: vec![0.0, hold],
    }
}

fn write_take(t: &Take, prefix: &Path) {
    if let Some(dir) = prefix.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let spec = hound::WavSpec { channels: 2, sample_rate: t.sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(prefix.with_extension("wav"), spec).expect("wav");
    for (a, b) in t.l.iter().zip(&t.r) {
        w.write_sample(*a).unwrap();
        w.write_sample(*b).unwrap();
    }
    w.finalize().unwrap();
    analyze::write_png(&prefix.with_extension("png"), &t.l, &t.r, t.sr, &t.marks).expect("png");
    let m = analyze::analyze(&t.l, &t.r, t.sr, &t.sections);
    std::fs::write(prefix.with_extension("json"), serde_json::to_string_pretty(&m).unwrap()).unwrap();
    let hold = m.sections.iter().find(|s| s.name == "hold").or(m.sections.first());
    match hold {
        Some(h) => println!(
            "{}  peak {:.3}  rms {:.1}dB  centroid {:.0}Hz  f0 {:.1}Hz ({:.2})  clip {}  nan {}",
            prefix.display(),
            m.peak,
            20.0 * h.rms.max(1e-9).log10(),
            h.centroid_mean,
            h.f0,
            h.periodicity,
            m.clipped,
            m.nan_or_inf
        ),
        None => println!("{}  peak {:.3}", prefix.display(), m.peak),
    }
}

fn read_wav(path: &Path) -> (Vec<f32>, Vec<f32>, u32) {
    let mut rd = hound::WavReader::open(path).expect("wav");
    let spec = rd.spec();
    let xs: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => rd.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let k = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            rd.samples::<i32>().map(|s| s.unwrap() as f32 * k).collect()
        }
    };
    let ch = spec.channels as usize;
    let l = xs.iter().step_by(ch).copied().collect();
    let r = if ch > 1 { xs.iter().skip(1).step_by(ch).copied().collect() } else { xs.to_vec() };
    (l, r, spec.sample_rate)
}

#[derive(Deserialize)]
struct Score {
    length: f64,
    #[serde(default)]
    params: BTreeMap<String, f64>,
    notes: Vec<Note>,
}

#[derive(Deserialize)]
struct Note {
    /// 1-16
    pad: usize,
    at: f64,
    dur: f64,
}

fn out_dir(a: &Args) -> PathBuf {
    PathBuf::from(a.get("out").unwrap_or("out"))
}

fn main() {
    let a = Args::parse();
    *TAP.lock().unwrap() = match a.get("tap") {
        None => Tap::Out,
        Some("voice") => Tap::Voice,
        Some("dist") => Tap::Dist,
        Some("crush") => Tap::Crush,
        Some("phaser") => Tap::Phaser,
        Some(_) => usage(),
    };
    let Some(cmd) = a.pos.first() else { usage() };
    match cmd.as_str() {
        "play" => {
            let seed = a.num("seed", 1.0) as u64;
            let takes = a.num("takes", 1.0) as u64;
            let pads: Vec<usize> = if a.has("all") {
                (0..PATCHES.len()).collect()
            } else {
                let p = a.num("pad", 0.0) as usize;
                if p < 1 || p > PATCHES.len() {
                    usage()
                }
                vec![p - 1]
            };
            for &pad in &pads {
                for k in 0..takes {
                    let s = seed + k;
                    let t = play_one(pad, s, &a);
                    write_take(&t, &out_dir(&a).join(format!("p{:02}-s{}", pad + 1, s)));
                }
            }
        }
        "score" => {
            let Some(file) = a.pos.get(1) else { usage() };
            let sc: Score = serde_json::from_str(&std::fs::read_to_string(file).expect("score")).expect("score json");
            let sr = a.num("sr", 48000.0) as u32;
            let seed = a.num("seed", 1.0) as u64;
            let mut ev = vec![];
            let mut marks = vec![];
            for n in &sc.notes {
                if n.pad < 1 || n.pad > PATCHES.len() {
                    usage()
                }
                ev.push((n.at, n.pad - 1, true));
                ev.push((n.at + n.dur, n.pad - 1, false));
                marks.push(n.at);
                marks.push(n.at + n.dur);
            }
            let params: Vec<_> = sc.params.into_iter().collect();
            let (l, r) = run(sr, seed, &params, &ev, sc.length);
            let t = Take { l, r, sr, sections: vec![("all".into(), 0.0, sc.length)], marks };
            let prefix = PathBuf::from(a.get("out").unwrap_or("out/score"));
            write_take(&t, &prefix);
        }
        "analyze" => {
            let Some(file) = a.pos.get(1) else { usage() };
            let (l, r, sr) = read_wav(Path::new(file));
            let len = l.len() as f64 / sr as f64;
            let hold = a.num("hold", 2.0).min(len);
            let prefix = a.get("out").map(PathBuf::from).unwrap_or_else(|| Path::new(file).with_extension(""));
            let t = Take {
                l,
                r,
                sr,
                sections: vec![("attack".into(), 0.0, 0.3f64.min(hold)), ("hold".into(), 0.3f64.min(hold), hold), ("release".into(), hold, len)],
                marks: vec![0.0, hold],
            };
            // WAV はそのまま、PNG と JSON だけ書く
            if let Some(dir) = prefix.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            analyze::write_png(&prefix.with_extension("png"), &t.l, &t.r, t.sr, &t.marks).expect("png");
            let m = analyze::analyze(&t.l, &t.r, t.sr, &t.sections);
            std::fs::write(prefix.with_extension("json"), serde_json::to_string_pretty(&m).unwrap()).unwrap();
            println!("{}  peak {:.3}  rms {:.1}dB", prefix.display(), m.peak, 20.0 * m.rms.max(1e-9).log10());
        }
        "meta" => {
            // Web 版の index.html に埋め込むパッド名とつまみ定義
            let pads: Vec<_> = PATCHES.iter().map(|p| p.name).collect();
            let params: Vec<_> = PARAMS
                .iter()
                .map(|d| serde_json::json!({ "id": d.id, "label": d.label, "min": d.min, "max": d.max, "step": d.step, "default": d.default }))
                .collect();
            println!("{}", serde_json::json!({ "pads": pads, "params": params }));
        }
        "compare" => {
            let (Some(da), Some(db)) = (a.pos.get(1), a.pos.get(2)) else { usage() };
            compare(Path::new(da), Path::new(db));
        }
        _ => usage(),
    }
}

/// パッドごとに hold 区間の指標を平均して並べる (シードが違うので分布で比べる)
fn compare(da: &Path, db: &Path) {
    let load = |d: &Path| -> BTreeMap<usize, Vec<serde_json::Value>> {
        let mut m: BTreeMap<usize, Vec<serde_json::Value>> = BTreeMap::new();
        for e in std::fs::read_dir(d).expect("dir").flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || !name.starts_with('p') {
                continue;
            }
            let Ok(pad) = name[1..3].parse::<usize>() else { continue };
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            m.entry(pad).or_default().push(v);
        }
        m
    };
    let (ma, mb) = (load(da), load(db));
    let stat = |vs: &[serde_json::Value], sec: &str, key: &str| -> f64 {
        let xs: Vec<f64> = vs
            .iter()
            .filter_map(|v| v["sections"].as_array()?.iter().find(|s| s["name"] == sec)?[key].as_f64())
            .collect();
        if xs.is_empty() { f64::NAN } else { xs.iter().sum::<f64>() / xs.len() as f64 }
    };
    let db_ = |x: f64| 20.0 * x.max(1e-9).log10();
    let bands = |vs: &[serde_json::Value]| -> Vec<f64> {
        let mut acc = vec![0.0; analyze::BANDS.len()];
        let mut n = 0.0;
        for v in vs {
            let Some(s) = v["sections"].as_array().and_then(|a| a.iter().find(|s| s["name"] == "hold")) else { continue };
            let Some(b) = s["bands_db"].as_array() else { continue };
            for (i, x) in b.iter().enumerate() {
                acc[i] += 10f64.powf(x.as_f64().unwrap_or(-200.0) / 10.0);
            }
            n += 1.0;
        }
        acc.iter().map(|e| 10.0 * (e / n + 1e-20).log10()).collect()
    };
    println!("{:<18} {:>13} {:>15} {:>13} {:>11}  (A → B, hold 区間の平均)", "pad", "rms dB", "centroid Hz", "periodicity", "rms_cv");
    for pad in 1..=PATCHES.len() {
        let (Some(a), Some(b)) = (ma.get(&pad), mb.get(&pad)) else { continue };
        println!(
            "{:2} {:<15} {:>6.1}→{:<6.1} {:>7.0}→{:<7.0} {:>6.2}→{:<6.2} {:>5.2}→{:<5.2}",
            pad,
            PATCHES[pad - 1].name,
            db_(stat(a, "hold", "rms")),
            db_(stat(b, "hold", "rms")),
            stat(a, "hold", "centroid_mean"),
            stat(b, "hold", "centroid_mean"),
            stat(a, "hold", "periodicity"),
            stat(b, "hold", "periodicity"),
            stat(a, "hold", "rms_cv"),
            stat(b, "hold", "rms_cv"),
        );
        let (ba, bb) = (bands(a), bands(b));
        let diff: Vec<String> = ba.iter().zip(&bb).map(|(x, y)| format!("{:+5.1}", y - x)).collect();
        println!("   帯域差 B-A dB ({}Hz): {}", analyze::BANDS.map(|f| f.to_string()).join("/"), diff.join(" "));
    }
}
