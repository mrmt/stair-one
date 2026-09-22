# engine

Stair One の音声エンジン (Rust)。旧 `index.html` の Web Audio グラフを移植したもの。
同じソースから Web 版 (wasm) と AU 版 (staticlib) を作る予定。

| crate | 内容 |
| --- | --- |
| `core` | DSP 本体。パッチ定義 (`patches.rs`)、つまみ定義 (`params.rs`)、Web Audio 相当の部品 (`dsp/`) |
| `render` | 試聴・評価用 CLI |

ツールチェーンは `rust-toolchain.toml` で固定している。rustup の cargo を使う
(Homebrew の `cargo` は `rust-toolchain.toml` を読まない):

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH"
```

## render

```sh
cd engine
cargo build --release
./target/release/render play --pad 3 --hold 2 --tail 1.5 --seed 42 --param drive=60 --out out/
./target/release/render play --all --takes 3 --out out/      # 16 パッド × 3 シード
./target/release/render score song.json --out out/song       # {"length": 6, "params": {"drive": 40}, "notes": [{"pad": 3, "at": 0, "dur": 2}]}
./target/release/render analyze foo.wav --hold 2             # 既存 WAV を同じ指標で評価
./target/release/render compare DIR_A DIR_B                  # パッドごとの指標の平均を並べる
```

1音ごとに `.wav` / `.png` (上: 波形、下: 30Hz–16kHz の対数スペクトログラム、緑の点線が押下と離鍵) /
`.json` (ピーク、RMS、クリップ数、非有限値、区間ごとのスペクトル重心・基音・周期性・音量の揺れ・オクターブ帯域) を書く。
録音前に 1.5 秒空回しする (起動直後はディレイタイムが 0 から伸びていく途中のため)。

## 旧版との比較

```sh
node tools/capture-legacy.mjs --takes 10          # 旧 index.html を OfflineAudioContext で鳴らして reference/legacy/ に書く
./engine/target/release/render play --all --takes 10 --sr 44100 --out out/rust
./engine/target/release/render compare reference/legacy out/rust
node tools/parity.mjs                             # 部品単位で Chromium と一致するか
uv run tools/ab.py reference/legacy out/rust out/ab   # 旧 → 新 を交互に並べた試聴用 WAV (クリック1回 = 旧、2回 = 新)
```

`capture-legacy.mjs` は旧版のコードをそのまま動かし、`Math.random` を Rust 版と同じ mulberry32 に、
`setInterval(tick, 30)` を 128 サンプル境界の `ctx.suspend()` に差し替える。
そのため **旧版のシード k と `render --seed k` は同じ乱数列・同じ tick 時刻で鳴り、波形単位で比べられる**。
`--tap voice|dist|crush|phaser` で途中の段だけを録れる (`render --tap` も同じ)。

移植で合わせた Chromium の挙動 (`tools/parity.mjs` と実測で確認):

| 挙動 | 実装 |
| --- | --- |
| 閉路は「処理中のノードの前回の出力」で断ち切られ、閉路の 1 辺だけ 128 サンプル遅れる | `dsp::delay::Quantum` (karplus: dl→lp、フェーザー: 最後の allpass→phFb、ディレイ: dL→lpL) |
| ステレオ入力の DelayNode は delayTime の自動変化を 2 倍速で読み進める | `fx.rs` のディレイタイム |
| 何も繋がっていない GainNode は 1ch。AudioWorklet (BitCrusher) は 1ch の間、右の数え上げが止まる | `Fx::process` の `stereo` |
| AudioBufferSourceNode の開始オフセットは最も近いサンプルに丸められる | `voice::noise_start` |
| WaveShaper 4x は 128 / 256 タップの sinc で 192 サンプル遅れ、dry と混ぜると櫛形になる | `dsp::shaper` |
| setTargetAtTime は開始時刻では開始値のまま。後の時刻の予約は前の時刻の予約で消えない | `dsp::param` |
