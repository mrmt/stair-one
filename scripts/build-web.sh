#!/bin/sh
# engine/ (Rust) を wasm にして index.html に埋め込む。パッド名とつまみ定義も engine/ から書き出す
#   scripts/build-web.sh          ビルドして index.html を書き換える
#   scripts/build-web.sh --check  ビルドして、index.html が最新か確かめる (CI 用。差分があれば失敗)
set -eu
cd "$(dirname "$0")/.."

# Homebrew の cargo は rust-toolchain.toml を読まないので、rustup の cargo を先に置く
if command -v brew >/dev/null 2>&1 && [ -d "$(brew --prefix rustup 2>/dev/null)/bin" ]; then
  PATH="$(brew --prefix rustup)/bin:$PATH"
fi

# wasm にビルドした場所のパスを埋め込まない。ホスト (macOS / Linux) が違うとシンボルのハッシュでバイト列は変わるので、
# --check はバイト列が違っても同じ音を出せば通す (scripts/embed.mjs)
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="--remap-path-prefix=$PWD/engine=/engine --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
(cd engine && cargo build -q -p stair-ffi --release --target wasm32-unknown-unknown && cargo build -q -p render --release)

WASM=engine/target/wasm32-unknown-unknown/release/stair_ffi.wasm
META="$(engine/target/release/render meta)"

if [ "${1:-}" = "--check" ]; then
  node scripts/embed.mjs "$WASM" "$META" --check
else
  node scripts/embed.mjs "$WASM" "$META"
fi
