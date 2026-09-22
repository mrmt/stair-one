//! エンジンの振る舞い。旧 tests/audio.spec.js の音の検証を Rust 側に移したもの

use stair_core::params::index_of;
use stair_core::{Engine, PADS};

const SR: f64 = 48000.0;

fn param(e: &mut Engine, id: &str, v: f64) {
    e.set_param(index_of(id).unwrap(), v);
}

/// n 秒鳴らして (peak, rms, 非有限値の数) を返す
fn run(e: &mut Engine, secs: f64) -> (f32, f32, usize) {
    let n = (secs * SR) as usize;
    let (mut l, mut r) = (vec![0f32; 128], vec![0f32; 128]);
    let (mut peak, mut sum, mut bad) = (0f32, 0f64, 0);
    let mut done = 0;
    while done < n {
        e.process(&mut l, &mut r);
        for x in l.iter().chain(r.iter()) {
            if !x.is_finite() {
                bad += 1;
            }
            peak = peak.max(x.abs());
            sum += (*x as f64).powi(2);
        }
        done += 128;
    }
    (peak, (sum / (2 * done) as f64).sqrt() as f32, bad)
}

/// エフェクトの残響を切り、リリースを最短にして「止まったか」を測りやすくする
fn dry_short(e: &mut Engine) {
    param(e, "release", 0.0);
    param(e, "dmix", 0.0);
    param(e, "dfb", 0.0);
}

#[test]
fn 押す前は無音() {
    let mut e = Engine::new(SR, 1);
    let (peak, _, bad) = run(&mut e, 0.5);
    // 歪みの曲線は 0 を補間で読むのでわずかな直流が残る (旧版も同じ)
    assert!(peak < 1e-6, "peak {peak}");
    assert_eq!(bad, 0);
}

#[test]
fn 押している間鳴り離すと止まる() {
    let mut e = Engine::new(SR, 1);
    dry_short(&mut e);
    e.note_on(0);
    let (on, _, _) = run(&mut e, 1.5);
    assert!(on > 0.01, "peak {on}");
    e.note_off(0);
    run(&mut e, 0.5);
    assert_eq!(e.voice_count(), 0);
    let (off, _, _) = run(&mut e, 0.8);
    assert!(off < on / 10.0, "on {on} off {off}");
}

#[test]
fn 全パッドが発音する() {
    for pad in 0..PADS {
        let mut e = Engine::new(SR, 7);
        dry_short(&mut e);
        e.note_on(pad);
        let (peak, _, bad) = run(&mut e, 1.2);
        assert!(peak > 0.01, "pad {} peak {peak}", pad + 1);
        assert_eq!(bad, 0, "pad {}", pad + 1);
        e.note_off(pad);
        run(&mut e, 0.5);
        assert_eq!(e.voice_count(), 0, "pad {}", pad + 1);
    }
}

#[test]
fn 同じシードなら同じ音で違うシードなら違う音() {
    let render = |seed| {
        let mut e = Engine::new(SR, seed);
        e.note_on(15);
        let (mut l, mut r) = (vec![0f32; 48000], vec![0f32; 48000]);
        e.process(&mut l, &mut r);
        l
    };
    assert_eq!(render(3), render(3));
    assert_ne!(render(3), render(4));
}

#[test]
fn 発音ごとに音が変わる() {
    let mut e = Engine::new(SR, 1);
    dry_short(&mut e);
    let mut takes = vec![];
    for _ in 0..3 {
        e.note_on(0);
        takes.push(run(&mut e, 0.5).1);
        e.note_off(0);
        run(&mut e, 0.5);
    }
    assert!(takes[0] != takes[1] && takes[1] != takes[2], "{takes:?}");
}

#[test]
fn フィードバック最大でもループが発散しない() {
    let mut e = Engine::new(SR, 5);
    param(&mut e, "dfb", 92.0);
    param(&mut e, "dmix", 100.0);
    // くし形共鳴を持つパッドとカオス
    for pad in [8, 9, 14, 15] {
        e.note_on(pad);
    }
    let (peak, _, bad) = run(&mut e, 12.0);
    assert_eq!(bad, 0);
    assert!(peak < 1.0);
}

#[test]
fn 音量歪み最大で16パッド同時押しでもクリップしない() {
    let mut e = Engine::new(SR, 9);
    param(&mut e, "volume", 100.0);
    param(&mut e, "drive", 100.0);
    param(&mut e, "dfb", 92.0);
    for pad in 0..PADS {
        e.note_on(pad);
    }
    let (peak, _, bad) = run(&mut e, 4.0);
    assert_eq!(bad, 0);
    assert!(peak > 0.05 && peak < 0.96, "peak {peak}");
}

#[test]
fn 同時発音は40まで() {
    let mut e = Engine::new(SR, 2);
    for i in 0..60 {
        e.note_on(i % PADS);
        run(&mut e, 0.01);
    }
    assert_eq!(e.voice_count(), 40);
}

#[test]
fn サンプルレートが違っても鳴る() {
    for sr in [44100.0, 96000.0] {
        let mut e = Engine::new(sr, 1);
        e.note_on(10);
        let n = (sr * 0.8) as usize;
        let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
        e.process(&mut l, &mut r);
        let peak = l.iter().fold(0f32, |a, x| a.max(x.abs()));
        assert!(peak > 0.01, "sr {sr} peak {peak}");
    }
}
