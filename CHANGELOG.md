# Changelog

## 未リリース

- **MIDI learn を追加した** — ヘッダーの `MIDI learn` で learn モードに入り、パッド / スライダを選んでコントローラを動かすと割り当てる。
  パッドは note / CC (ボタン型)、スライダは CC。割当は `localStorage` (`stair-one.midi.v1`) に保存し、次回も有効。
  learn 中は `Delete` で解除、`clear all` で全解除、`Esc` で抜ける。Web MIDI 非対応ブラウザではボタンを無効にする
- **タイトルの左にアイコンを置き、親ディレクトリ (`../`) へのリンクにした** — elevator-one / elevator-two と同じ SVG を `index.html` に直書き (`header a.home`)

## v0.1

- 最初の版。4x4 の16パッド、押している間だけ鳴る
- 音源: 32′/16′/8′/4′ の矩形・ノコギリ波の複数本デチューン、ホワイトノイズ、カープラス・ストロング (くし形共鳴)、グラニュラー (テープ wow / flutter、逆再生)
- オシレータごとに別設定の LFO (sine / square / sawtooth / sample & hold、1〜100Hz)
- 音程ドリフト (上昇 / 下降)、ランダムアルペジエータ、フィルタの開閉スイープと高レゾナンス
- 全パラメータが発音ごとにランダムに決まり、発音中も揺れ続ける
- 共通スライダ: attack / decay / release、distortion、bitcrush、phaser、stereo delay (time / feedback / mix)、volume
- 入力: ポインタ (マルチタッチ)、キーボード。外部入力用に `window.stair.press / release` を公開
