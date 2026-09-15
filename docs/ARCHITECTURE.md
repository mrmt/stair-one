# Architecture

`index.html` 1枚に UI・合成・エフェクト・入力をすべて持つ。Web Audio API のみ。

## オーディオグラフ

```
Voice × n ─→ voiceBus
  → Distortion (WaveShaper, dry/wet)
  → BitCrusher (AudioWorklet を Blob URL で読み込み, dry/wet)
  → Phaser (allpass 6段, LFO→detune, DelayNode 経由のフィードバック)
  ├→ sum (dry)
  └→ StereoDelay (L/R 別時間, 交差フィードバック, LPF) → sum
  → DynamicsCompressor → master(volume) → tanh ソフトクリップ(±0.95) → destination
```

`AudioContext` は最初のパッド押下 (ユーザー操作) で `ensureAudio()` が作る。演奏開始ボタンはない。
BitCrusher の Worklet 読み込みは非同期なので、読み込み完了までは dry だけが通る。

## Voice

1回の押下 = 1 Voice。`noteOn(pad)` がパッチ定義 (`PATCHES`) からノードを組む。

```
layers → VCF (Biquad 1段 or 同一設定2段直列) → amp (ゆらぎ) → vca (ADSR) → StereoPanner → voiceBus
```

### パッチ定義の値

`R(x)` が発音ごとに値を確定させる。`[数, 数]` は範囲の一様乱数、文字列配列は1つ選択、それ以外は固定値。
つまりパッチは「値」ではなく「値の分布」を持つ。

### レイヤー種別

| kind | 実装 |
| --- | --- |
| `osc` | `count` 本の Oscillator を `spread` cent ずつずらす。1本ごとに別設定の LFO が detune にかかる |
| `noise` | ループするホワイトノイズ。LFO は音量にかかる (チョップ / トレモロ) |
| `karplus` | ノイズ励起 → DelayNode(1/f) → LPF → feedback。`pluck` 回/秒で励起を叩く。ループ内の遅延は最低 128 サンプルになるので f は 300Hz で頭打ち |
| `grain` | 生成済みの汚いテープ素材 (正/逆) から短い断片を窓付きで散布。wow / flutter の LFO を各粒の detune に配る |

### 音程

全 osc / grain の detune に `pitchCV` (ドリフト + ランダムウォーク) と `arpCV` (アルペジオ) の ConstantSource を足している。
karplus は AudioParam に cent を足せないので、`tick()` で遅延時間を計算し直す。

ドリフトは発音ごとに上昇 / 下降 / なしを確率で選び、`drift.max` cent で折り返す。

### LFO

`makeLFO()`: sine / square / sawtooth は OscillatorNode。sample & hold は 1秒 64段のランダム階段バッファを
ループ再生し、`playbackRate = rate / 64` で周期を決める。rate は 1〜100Hz にクランプ (テープの wow だけ 0.05Hz から)。

### 発音中のゆらぎ

`W(base, spread, revert)` は平均回帰つきランダムウォーク。`tick()` (30ms) が全 Voice の
ピッチ、フィルタのカットオフと Q、音量、パン、各 LFO の速さと深さ、オシレータのデチューン、
karplus のフィードバック、粒の密度、アルペジオ速度を少しずつ動かし、`setTargetAtTime` で渡す。

フィルタは発音ごとに開く / 閉じる方向と速さ (oct/秒) を決め、40Hz / 12kHz で折り返す。

### VCA

attack / decay / release は全パッド共通スライダの値 × 発音ごとの 0.75〜1.3 倍。sustain はパッチ側。
離すと `cancelAndHoldAtTime` から `setTargetAtTime(0, rel/6)` で落とし、`rel` 秒後に `dispose()` で全ノードを切り離す。
Voice が 40 を超えたらリリース中の古いものから捨てる。

## 入力層

`press(pad, source)` / `release(pad, source)` に集約し、パッドごとに押下元の Set を持つ。
Set が空→非空で noteOn、非空→空で noteOff。押下元が違えば同じパッドを重ねて押せる。

| source | 由来 |
| --- | --- |
| `ptr:<pointerId>` | pointerdown / pointerup / pointercancel / lostpointercapture |
| `key:<code>` | keydown (repeat 無視) / keyup |

`blur` と `visibilitychange(hidden)` で `releaseAll()`。

### MIDI 対応 (未実装)

`window.stair.press / release` がそのまま外部入力の口になる。

```js
const access = await navigator.requestMIDIAccess();
for (const input of access.inputs.values()) {
  input.onmidimessage = ({ data: [st, note, vel] }) => {
    const pad = NOTE_MAP[note]; if (pad == null) return;
    const src = `midi:${input.id}:${note}`;
    if ((st & 0xf0) === 0x90 && vel > 0) stair.press(pad, src);
    else if ((st & 0xf0) === 0x80 || (st & 0xf0) === 0x90) stair.release(pad, src);
  };
}
```

AudioContext 生成にはユーザー操作が要るので、MIDI だけで弾き始める場合は最初に一度クリックさせる必要がある。

## テスト

`tests/helpers/audio.js` (elevator-one 由来) が `AudioNode.prototype.connect` を包み、destination 手前に AnalyserNode を挟む。

| project | 内容 |
| --- | --- |
| `audio-chromium` | 押下で鳴る / 離すと止まる、16パッド全発音、キーボード、blur、発音ごと・発音中のゆらぎ、最大設定でクリップしない |
| `desktop-chromium` / `mobile-webkit` | 4x4 配置、横スクロールなし、ポインタ押下、マルチタッチ、スライダ表示、狭幅の積み順 |
