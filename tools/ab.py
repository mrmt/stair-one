# /// script
# dependencies = ["numpy", "soundfile"]
# ///
"""旧版と Rust 版を交互に並べた試聴用 WAV を作る
    uv run tools/ab.py reference/legacy out/rust out/ab [--takes 2]
パッドごとに out/ab/pNN.wav (旧 → 新 を takes 回)、全パッド通しの out/ab/all.wav を書く。
各音の前に 0.4 秒の無音、旧 → 新の切替前に短いクリック (旧) / 2回クリック (新) を入れる
"""
import argparse
import glob
import os

import numpy as np
import soundfile as sf

ap = argparse.ArgumentParser()
ap.add_argument("a")
ap.add_argument("b")
ap.add_argument("out")
ap.add_argument("--takes", type=int, default=2)
args = ap.parse_args()
os.makedirs(args.out, exist_ok=True)


def click(sr, n):
    t = np.arange(int(sr * 0.01)) / sr
    c = 0.3 * np.sin(2 * np.pi * 2000 * t) * np.exp(-t * 400)
    gap = np.zeros(int(sr * 0.08))
    one = np.concatenate([c, gap])
    return np.tile(one, n)[:, None].repeat(2, axis=1)


reel = []
sr0 = None
for pad in range(1, 17):
    fa = sorted(glob.glob(f"{args.a}/p{pad:02}-s*.wav"))[: args.takes]
    fb = sorted(glob.glob(f"{args.b}/p{pad:02}-s*.wav"))[: args.takes]
    parts = []
    for x, y in zip(fa, fb):
        for f, n in ((x, 1), (y, 2)):
            d, sr = sf.read(f, always_2d=True)
            sr0 = sr0 or sr
            parts += [click(sr, n), np.zeros((int(sr * 0.4), 2)), d]
    if parts:
        seq = np.concatenate(parts)
        sf.write(f"{args.out}/p{pad:02}.wav", seq, sr0)
        reel.append(seq)
if reel:
    sf.write(f"{args.out}/all.wav", np.concatenate(reel), sr0)
print(f"{args.out}/pNN.wav, {args.out}/all.wav")
