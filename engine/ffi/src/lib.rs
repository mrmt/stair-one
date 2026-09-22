//! C ABI。Web 版 (wasm, AudioWorklet から呼ぶ) と AU 版 (staticlib, JUCE から呼ぶ) で共通
//! wasm-bindgen は使わない。ポインタと数値だけをやり取りする

use stair_core::params::PARAMS;
use stair_core::voice::VoiceInfo;
use stair_core::{Engine, PADS};

/// 1回の stair_process で書ける最大サンプル数
pub const MAX_BLOCK: usize = 4096;
/// stair_debug の 1 ボイスあたりの数
const INFO_FIELDS: usize = 8;

pub struct Handle {
    engine: Engine,
    l: Vec<f32>,
    r: Vec<f32>,
    voices: Vec<VoiceInfo>,
    info: Vec<f64>,
}

/// エンジンを作る。seed は乱数の種 (Web 版は起動ごとに crypto から取る)
#[no_mangle]
pub extern "C" fn stair_new(sample_rate: f64, seed: u32) -> *mut Handle {
    let h = Handle {
        engine: Engine::new(sample_rate, seed as u64),
        l: vec![0.0; MAX_BLOCK],
        r: vec![0.0; MAX_BLOCK],
        voices: Vec::with_capacity(stair_core::MAX_VOICES),
        info: vec![0.0; 1 + stair_core::MAX_VOICES * INFO_FIELDS],
    };
    Box::into_raw(Box::new(h))
}

/// # Safety
/// h は stair_new が返したもので、まだ解放していないこと
#[no_mangle]
pub unsafe extern "C" fn stair_free(h: *mut Handle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

fn get<'a>(h: *mut Handle) -> &'a mut Handle {
    // SAFETY: 呼び出し側 (worklet / AU) は stair_new の戻り値だけを渡す
    unsafe { &mut *h }
}

#[no_mangle]
pub extern "C" fn stair_note_on(h: *mut Handle, pad: u32) {
    get(h).engine.note_on(pad as usize);
}

#[no_mangle]
pub extern "C" fn stair_note_off(h: *mut Handle, pad: u32) {
    get(h).engine.note_off(pad as usize);
}

/// つまみの値 (UI の input と同じ単位)。idx は params.rs の PARAMS の順
#[no_mangle]
pub extern "C" fn stair_set_param(h: *mut Handle, idx: u32, value: f64) {
    get(h).engine.set_param(idx as usize, value);
}

/// n サンプル (MAX_BLOCK 以下) を内部の L/R バッファに書く。読むのは stair_out_l / stair_out_r
#[no_mangle]
pub extern "C" fn stair_process(h: *mut Handle, n: u32) {
    let h = get(h);
    let n = (n as usize).min(MAX_BLOCK);
    h.engine.process(&mut h.l[..n], &mut h.r[..n]);
}

#[no_mangle]
pub extern "C" fn stair_out_l(h: *mut Handle) -> *const f32 {
    get(h).l.as_ptr()
}

#[no_mangle]
pub extern "C" fn stair_out_r(h: *mut Handle) -> *const f32 {
    get(h).r.as_ptr()
}

/// 呼び出し側のバッファに直接書く (AU 用)
///
/// # Safety
/// l, r は n 個の f32 を書ける領域であること
#[no_mangle]
pub unsafe extern "C" fn stair_render(h: *mut Handle, l: *mut f32, r: *mut f32, n: u32) {
    let (l, r) = (std::slice::from_raw_parts_mut(l, n as usize), std::slice::from_raw_parts_mut(r, n as usize));
    get(h).engine.process(l, r);
}

#[no_mangle]
pub extern "C" fn stair_voice_count(h: *mut Handle) -> u32 {
    get(h).engine.voice_count() as u32
}

/// 鳴っているボイスの中身。[数, (pad, root, cut, pitch, cents, cv, delay, released) × 数]
#[no_mangle]
pub extern "C" fn stair_debug(h: *mut Handle) -> *const f64 {
    let h = get(h);
    h.engine.voices(&mut h.voices);
    h.info[0] = h.voices.len() as f64;
    for (i, v) in h.voices.iter().enumerate() {
        let o = &mut h.info[1 + i * INFO_FIELDS..1 + (i + 1) * INFO_FIELDS];
        o.copy_from_slice(&[v.pad as f64, v.root, v.cut, v.pitch, v.cents, v.cv, v.delay, v.released as u8 as f64]);
    }
    h.info.as_ptr()
}

#[no_mangle]
pub extern "C" fn stair_pad_count() -> u32 {
    PADS as u32
}

#[no_mangle]
pub extern "C" fn stair_param_count() -> u32 {
    PARAMS.len() as u32
}

#[no_mangle]
pub extern "C" fn stair_max_block() -> u32 {
    MAX_BLOCK as u32
}
