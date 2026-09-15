# Changelog

## v0.1

- **スライダを丸いつまみにし、画面の左に置いた** — 2列×4行の8個 (左列 attack / decay / release / pitch、右列 delay time / delay feedback / delay mix / phaser)。
  distortion と volume は下に小さく残した。縦ドラッグ / ホイール / ダブルクリックで既定値。実体の range input を透明で重ねてあるので、キーボード操作と MIDI 割当はそのまま使える
- **pitch つまみを追加した** — 鳴っている全ての音の音程を ±12 半音上げ下げする
- **bitcrush のつまみを廃止した** — 効果は固定量 (0.2) で常にかける
- **S&H Filter Noise の音量を 70% にした** — パッチのゲイン 1.1 → 0.47。出力段のコンプで潰れるため
  ゲインを 70% (0.77) にしても出力 RMS はほぼ変わらなかった。各30回の出力 RMS 平均で 0.5 → 74%、0.42 → 63% だったので間を取った

- **MIDI learn を追加した** — ヘッダーの `MIDI learn` で learn モードに入り、パッド / スライダを選んでコントローラを動かすと割り当てる。
  パッドは note / CC (ボタン型)、スライダは CC。割当は `localStorage` (`stair-one.midi.v1`) に保存し、次回も有効。
  learn 中は `Delete` で解除、`clear all` で全解除、`Esc` で抜ける。Web MIDI 非対応ブラウザではボタンを無効にする
- **タイトルの左にアイコンを置き、親ディレクトリ (`../`) へのリンクにした** — elevator-one / elevator-two と同じ SVG を `index.html` に直書き (`header a.home`)

- 最初の版。4x4 の16パッド、押している間だけ鳴る
- 音源: 32′/16′/8′/4′ の矩形・ノコギリ波の複数本デチューン、ホワイトノイズ、カープラス・ストロング (くし形共鳴)、グラニュラー (テープ wow / flutter、逆再生)
- オシレータごとに別設定の LFO (sine / square / sawtooth / sample & hold、1〜100Hz)
- 音程ドリフト (上昇 / 下降)、ランダムアルペジエータ、フィルタの開閉スイープと高レゾナンス
- 全パラメータが発音ごとにランダムに決まり、発音中も揺れ続ける
- 共通スライダ: attack / decay / release、distortion、bitcrush、phaser、stereo delay (time / feedback / mix)、volume
- 入力: ポインタ (マルチタッチ)、キーボード。外部入力用に `window.stair.press / release` を公開
