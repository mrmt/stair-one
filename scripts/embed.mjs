// index.html の `const META = ...;` と `const ENGINE_WASM = '...';` の2行を書き換える
//   node scripts/embed.mjs <wasm> <meta json> [--check]
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';

const [wasmPath, meta, flag] = process.argv.slice(2);
const html = readFileSync('index.html', 'utf8');
const b64 = readFileSync(wasmPath).toString('base64');
const lines = html.split('\n');
const set = (prefix, value) => {
  const i = lines.findIndex(l => l.startsWith(prefix));
  if (i < 0) throw new Error(`index.html に "${prefix}" の行が無い`);
  lines[i] = prefix + value + ';';
};
set('const META = ', JSON.stringify(JSON.parse(meta)));
set('const ENGINE_WASM = ', `'${b64}'`);
const out = lines.join('\n');
// 2つの wasm が同じ音を出すか (16 パッドを 1.5 秒ずつ)。
// ビルドするホスト (macOS / Linux) でシンボルのハッシュが変わり wasm のバイト列は一致しないので、音で比べる
function sameSound(a, b) {
  const inst = bytes => new WebAssembly.Instance(new WebAssembly.Module(bytes), {}).exports;
  const [ea, eb] = [inst(a), inst(b)];
  const run = (e, pad) => {
    const h = e.stair_new(48000, 7), out = [];
    e.stair_note_on(h, pad);
    for (let k = 0; k < 48000 * 1.5 / 128; k++) {
      if (k === 375) e.stair_note_off(h, pad);
      e.stair_process(h, 128);
      out.push(...new Float32Array(e.memory.buffer, e.stair_out_l(h), 128), ...new Float32Array(e.memory.buffer, e.stair_out_r(h), 128));
    }
    e.stair_free(h);
    return out;
  };
  for (let pad = 0; pad < ea.stair_pad_count(); pad++) {
    const [x, y] = [run(ea, pad), run(eb, pad)];
    if (x.length !== y.length || x.some((v, i) => !Object.is(v, y[i]))) return false;
  }
  return true;
}

if (flag === '--check') {
  const cur = html.split('\n');
  const embedded = Buffer.from((cur.find(l => l.startsWith('const ENGINE_WASM = ')) ?? '').slice(21, -2), 'base64');
  const built = readFileSync(wasmPath);
  const metaOk = cur.includes(lines.find(l => l.startsWith('const META = ')));
  if (metaOk && !embedded.equals(built) && sameSound(embedded, built)) {
    console.log('index.html は最新 (wasm のバイト列は違うが、同じ音を出す)');
    process.exit(0);
  }
  if (out !== html) {
    const sha = b => createHash('sha256').update(b).digest('hex').slice(0, 16);
    console.error(`META ${metaOk ? '一致' : '不一致'}`);
    console.error(`wasm 埋め込み ${embedded.length} B ${sha(embedded)} / ビルド ${built.length} B ${sha(built)}`);
    console.error('index.html の埋め込みが engine/ と一致しない。scripts/build-web.sh を実行してコミットする');
    process.exit(1);
  }
  console.log('index.html は最新');
} else {
  writeFileSync('index.html', out);
  console.log(`index.html: wasm ${(b64.length / 1024).toFixed(0)} KB (base64)`);
}
