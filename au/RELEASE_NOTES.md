Stair One のプラグイン版。音は同じタグの Web 版 (`index.html`) と同じエンジンで鳴る。

## インストール (macOS 11 以降、Apple Silicon / Intel)

Apple の公証を受けていない (アドホック署名) ため、ダウンロードした zip には隔離属性が付く。外してから入れる。

```sh
cd ~/Downloads
xattr -dr com.apple.quarantine StairOne-*-AU.zip StairOne-*-VST3.zip
ditto -x -k StairOne-*-AU.zip ~/Library/Audio/Plug-Ins/Components/
ditto -x -k StairOne-*-VST3.zip ~/Library/Audio/Plug-Ins/VST3/
killall -9 AudioComponentRegistrar   # AU の一覧を読み直させる
auval -v aumu Str1 Mrmt              # 任意: 検証
```

Logic Pro では「ソフトウェア音源 → AU 音源 → mrmt → Stair One」。
MIDI ノート 36–51 (Logic 表記 C1–D#2) がパッド 1–16。
