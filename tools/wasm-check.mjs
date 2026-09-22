// wasm (Web 版) とネイティブ (render / AU) が同じ音を出すかを 1 サンプル単位で確かめる
//   node tools/wasm-check.mjs            (先に scripts/build-web.sh でビルドしておく)
// render play と同じ手順 (1.5 秒の空回し、128 サンプルごとのイベント) を wasm でなぞり、render の WAV と比べる
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const SR = 48000, HOLD = 1, TAIL = 0.5, SEED = 7;
const wasm = readFileSync('engine/target/wasm32-unknown-unknown/release/stair_ffi.wasm');
const e = new WebAssembly.Instance(new WebAssembly.Module(wasm), {}).exports;
const dir = mkdtempSync(join(tmpdir(), 'stair-'));

function renderWasm(pad) {
  const h = e.stair_new(SR, SEED);
  const pre = Math.ceil(1.5 * SR / 128);
  for (let i = 0; i < pre; i++) e.stair_process(h, 128);
  const total = Math.floor((HOLD + TAIL) * SR);
  const l = new Float32Array(total), r = new Float32Array(total);
  let on = false, off = false;
  for (let pos = 0; pos < total; pos += 128) {
    const t = pos / SR;
    if (!on) { e.stair_note_on(h, pad); on = true; }
    if (!off && HOLD <= t) { e.stair_note_off(h, pad); off = true; }
    const n = Math.min(128, total - pos);
    e.stair_process(h, n);
    l.set(new Float32Array(e.memory.buffer, e.stair_out_l(h), n), pos);
    r.set(new Float32Array(e.memory.buffer, e.stair_out_r(h), n), pos);
  }
  e.stair_free(h);
  return [l, r];
}

// render の WAV (32bit float, 2ch)
function readWav(path) {
  const b = readFileSync(path);
  const data = b.indexOf('data') + 8;
  const f = new Float32Array(b.buffer.slice(b.byteOffset + data, b.byteOffset + b.length));
  const n = f.length / 2, l = new Float32Array(n), r = new Float32Array(n);
  for (let i = 0; i < n; i++) { l[i] = f[2 * i]; r[i] = f[2 * i + 1]; }
  return [l, r];
}

execFileSync('engine/target/release/render', ['play', '--all', '--seed', String(SEED), '--sr', String(SR), '--hold', String(HOLD), '--tail', String(TAIL), '--out', dir]);
let fail = 0;
for (let pad = 0; pad < e.stair_pad_count(); pad++) {
  const [wl, wr] = renderWasm(pad);
  const [nl, nr] = readWav(join(dir, `p${String(pad + 1).padStart(2, '0')}-s${SEED}.wav`));
  let diff = 0, first = -1;
  for (let i = 0; i < wl.length; i++) {
    if (wl[i] !== nl[i] || wr[i] !== nr[i]) { diff++; if (first < 0) first = i; }
  }
  if (wl.length !== nl.length || diff) fail++;
  console.log(`${diff ? 'NG' : 'ok'}  pad ${String(pad + 1).padStart(2)}  ${diff ? `${diff} サンプル違う (最初 ${first})` : `${wl.length} サンプル一致`}`);
}
process.exit(fail ? 1 : 0);
