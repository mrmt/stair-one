// index.html の `const META = ...;` と `const ENGINE_WASM = '...';` の2行を書き換える
//   node scripts/embed.mjs <wasm> <meta json> [--check]
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
if (flag === '--check') {
  if (out !== html) {
    console.error('index.html の埋め込みが engine/ と一致しない。scripts/build-web.sh を実行してコミットする');
    process.exit(1);
  }
  console.log('index.html は最新');
} else {
  writeFileSync('index.html', out);
  console.log(`index.html: wasm ${(b64.length / 1024).toFixed(0)} KB (base64)`);
}
