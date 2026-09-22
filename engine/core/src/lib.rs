//! Stair One の音声エンジン。旧 index.html の Web Audio グラフを Rust に移したもの

pub mod dsp;
pub mod engine;
pub mod fx;
pub mod params;
pub mod patches;
pub mod rng;
pub mod tables;
pub mod voice;

pub use engine::{Engine, MAX_VOICES, PADS};
