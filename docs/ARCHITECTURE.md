# Architecture

`index.html` 1枚に UI と入力を持ち、音は Rust のエンジン (`engine/`) を wasm にして埋め込んだものが作る。

```
index.html (UI, 入力層, MIDI learn)
  └ AudioWorklet 'stair-engine' (data: URL) ── 埋め込み wasm (engine/ffi) ── engine/core
       ↑ port: {t:'on'|'off', pad} / {t:'param', i, v} / {t:'debug'}
       ↓ port: {t:'count', n} (約 50ms ごと) / {t:'debug', voices}
```

- `engine/core`: DSP 本体。パッチ定義 (`patches.rs`) とつまみ定義 (`params.rs`) の正本
- `engine/ffi`: C ABI (`stair_new` / `stair_note_on` / `stair_process` ...)。wasm と AU (staticlib) で共通
- `engine/render`: 試聴・評価用 CLI (`engine/README.md`)
- `scripts/build-web.sh`: wasm をビルドし、base64 で `index.html` の `const ENGINE_WASM` に、
  パッド名とつまみ定義を `const META` に書き込む。`--check` は CI 用
- `tools/legacy-index.html`: 移植前の Web Audio 版 (v0.1)。`tools/capture-legacy.mjs` と `tools/parity.mjs` が比較に使う

つまみの `id` (`params.rs`) は AU のパラメータ ID にもなるので、変更・削除しない (追加のみ)。
HTML の `<input>` の min / max / step / value は `META.params` と一致している必要がある (`tests/interaction.spec.js` が確認)。

エンジンは移植前の Web Audio グラフを、Chromium の挙動まで含めて再現している (再現した癖は `engine/README.md`)。
以下の「ノード」は `engine/core` の中の部品 (`dsp/`) を指す。

## オーディオグラフ

```
Voice × n ─→ voiceBus
  → Distortion (WaveShaper, dry/wet)
  → BitCrusher (AudioWorklet を Blob URL で読み込み, dry/wet。つまみは無く固定量 CRUSH = 0.2)
  → Phaser (allpass 6段, LFO→detune, DelayNode 経由のフィードバック)
  ├→ sum (dry)
  └→ StereoDelay (L/R 別時間, 交差フィードバック, LPF) → sum
  → DynamicsCompressor → master(volume) → tanh ソフトクリップ(±0.95) → destination
```

### フィードバックループの注意

BiquadFilter の lowpass は Q が **dB 指定**で、既定値 1 だとカットオフ付近に約 1.12 倍の山がある。
ループ内に既定 Q の LPF を置くと、フィードバック 0.9 前後でもループ利得が 1 を超えて発散し、
下流の BiquadFilter が `state is bad` 警告を出して壊れる。karplus とステレオディレイの LPF は `Q = -6` で山を消し、
karplus はさらにループ内に傾き 1 の tanh を置いて振幅の上限を保証している。
Rust 版もこの対策をそのまま持つ。`engine/core/tests/engine.rs` の「フィードバック最大でもループが発散しない」が確認する。

`AudioContext` は最初のパッド押下 (ユーザー操作) で `ensureAudio()` が作る。演奏開始ボタンはない。
Worklet の読み込みと wasm の組み立ては非同期なので、その間の押下・つまみ操作は溜めておき、できた時点で順に送る。
乱数の種は起動ごとに `crypto.getRandomValues` で決める。

## Voice

1回の押下 = 1 Voice。`Voice::start` (`engine/core/src/voice.rs`) がパッチ定義 (`PATCHES`) から今回の値を決める。
メモリはエンジン生成時に 40 ボイス分確保し、発音中は確保しない。

```
layers → VCF (Biquad 1段 or 同一設定2段直列) → amp (ゆらぎ) → vca (ADSR) → StereoPanner → voiceBus
```

### パッチ定義の値

`Num::sample` / `Rng::pick` が発音ごとに値を確定させる。`r(a, b)` は範囲の一様乱数、`f(x)` は固定値、配列は1つ選択。
つまりパッチは「値」ではなく「値の分布」を持つ。

### レイヤー種別

| kind | 実装 |
| --- | --- |
| `osc` | `count` 本の Oscillator を `spread` cent ずつずらす。1本ごとに別設定の LFO が detune にかかる |
| `noise` | ループするホワイトノイズ。LFO は音量にかかる (チョップ / トレモロ) |
| `karplus` | ノイズ励起 → Delay(1/f) → LPF → tanh → feedback。`pluck` 回/秒で励起を叩く。f は 300Hz で頭打ち。Chromium では閉路の1辺が 128 サンプル遅れるので、実際の周期は 1/f + 128 サンプル (これも再現している) |
| `grain` | 生成済みの汚いテープ素材 (正/逆) から短い断片を窓付きで散布。wow / flutter の LFO を各粒の detune に配る |

### 音程

全 osc / grain の detune に `pitchCV` (ドリフト + ランダムウォーク) と `arpCV` (アルペジオ) の ConstantSource を足している。
karplus は遅延時間に cent を足せないので、`tick()` で遅延時間を計算し直す。
`tick()` は 30ms ごと (128 サンプル境界に切り上げ) にエンジンの中で呼ぶ。

ドリフトは発音ごとに上昇 / 下降 / なしを確率で選び、`drift.max` cent で折り返す。

pitch つまみ (±12 半音) は `tick()` で全ボイスの `cents` に足すので、発音中の音にも 30ms 以内に掛かる
(osc / grain は `pitchCV`、karplus は遅延時間の計算に入る)。

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

## つまみ

`.knob` の中に SVG (弧・本体・指示線) と、実体の `<input type="range" id="s_*">` を透明で重ねている。
値は常に input が持ち、描画 (`drawKnob()`)・表示 (`updateReadouts()`)・音 (`applyFx()`) は input の `input` イベントで更新する。
こうしておくと、キーボード操作と読み上げ、MIDI の `slider:<id>` 割当、Playwright の `fill` がそのまま使える。

- 縦ドラッグ: pointer capture で 150px = 全域 (Shift で 600px)。ホイール: 1/100 (Shift で 1/400)。ダブルクリック: HTML の初期値に戻す
- 弧は -135°〜+135°、`pathLength="100"` の path に `stroke-dasharray` で値の区間だけ描く。min が負の input (pitch) は中央から伸ばす
- learn 中はつまみのドラッグとホイールを止め、`.ctl` の pointerdown が割当先の選択になる

## 入力層

`press(pad, source)` / `release(pad, source)` に集約し、パッドごとに押下元の Set を持つ。
Set が空→非空で noteOn、非空→空で noteOff。押下元が違えば同じパッドを重ねて押せる。

| source | 由来 |
| --- | --- |
| `ptr:<pointerId>` | pointerdown / pointerup / pointercancel / lostpointercapture |
| `key:<code>` | keydown (repeat 無視) / keyup |
| `midi:<key>` | MIDI learn で割り当てた note / CC |

`blur` と `visibilitychange(hidden)` で `releaseAll()`。

### MIDI learn

割当表 `midiMap` は `key → target` の1対1。

| | 形式 |
| --- | --- |
| key | `note:<ch>:<num>` / `cc:<ch>:<num>` (入力デバイスは区別しない。繋ぎ直しても効く) |
| target | `pad:<0-15>` / `slider:<input id>` |

- **learn**: `setLearning(true)` で body に `.learning`。パッドの pointerdown と `.ctl` 行の pointerdown が `selectTarget()` になる
  (つまみはドラッグを止めて値を動かさない)。選択中に来た最初の note on / CC で `assign()`。
  同じ target の旧 key と、同じ key の旧 target は外れる。スライダに note は割り当てない
- **演奏**: パッドは note on/off、または CC の 64 以上/未満で `press / release(pad, 'midi:<key>')`。
  スライダは CC 0–127 を min–max に写して `input` イベントを発火し、既存の readout 更新と worklet への送信に乗せる
- **保存**: `localStorage['stair-one.midi.v1'] = { map }`。読み込み時に存在しない target は捨てる
- **Web MIDI 取得**: 初回の learn 押下時。保存済み割当があれば読み込み時にも取る。`statechange` で入力を付け直し、
  切断時は `midi:` 由来の押下を全部離す (note off が届かないため)
- **音声の解錠**: MIDI 入力はユーザー操作扱いにならず AudioContext を動かせない。learn 押下とページ上の最初の
  pointerdown / keydown で `ensureAudio()`。動いていないときに割当済み MIDI が来たら状態表示に `click to enable sound` を出す

テストは `tests/midi.spec.js` が `navigator.requestMIDIAccess` を偽物に差し替え、`window.__midiSend([status, d1, d2])` で入力を注入する。

## テスト

| 場所 | 内容 |
| --- | --- |
| `engine/core/tests/engine.rs` (`cargo test`) | 全パッド発音、離すと止まる、同シード同出力、フィードバック最大で発散しない、最大設定でクリップしない、40 ボイス上限、サンプルレート違い |
| `tools/wasm-check.mjs` | wasm とネイティブの出力が 1 サンプル単位で一致する |
| `tools/parity.mjs` | 部品単位で Chromium (OfflineAudioContext) と一致する |
| `scripts/build-web.sh --check` | `index.html` の埋め込みが `engine/` と一致する |

`tests/helpers/audio.js` (elevator-one 由来) が `AudioNode.prototype.connect` を包み、destination 手前に AnalyserNode を挟む。

| project | 内容 |
| --- | --- |
| `audio-chromium` (`window.stair.debug()` は worklet に問い合わせるので Promise を返す) | file:// で開いても鳴る、押下で鳴る / 離すと止まる、16パッド全発音、キーボード、blur、発音ごと・発音中のゆらぎ、pitch つまみで発音中の音程が動く、フィードバック最大で発散しない、最大設定でクリップしない |
| `midi-chromium` | MIDI learn: モード切替、note / CC の割当と演奏、保存、再割当・解除、非対応ブラウザ |
| `desktop-chromium` / `mobile-webkit` (WebKit は Web Audio を外して実行。CI の WebKit で AudioContext を動かすとページが固まるため) | パッド名・つまみ定義がエンジンと一致、4x4 配置、ホームアイコン、横スクロールなし、ポインタ押下、マルチタッチ、つまみの値表示・2列×4行の並び・デスクトップで左配置、縦ドラッグとダブルクリック、狭幅の積み順 |
