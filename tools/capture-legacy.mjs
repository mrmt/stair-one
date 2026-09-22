// 旧 Web Audio 版 (index.html) の各パッドを OfflineAudioContext で鳴らして WAV にする。Rust 版との比較用
//   node tools/capture-legacy.mjs [--html tools/legacy-index.html] [--out reference/legacy] [--takes 3] [--hold 2] [--tail 1.5]
//                                 [--pads 1-16] [--params dmix=0,drive=100] [--sr 44100]
// 旧版のコードはそのまま動かし、次の3つだけ差し替える:
//   - Math.random: シード付き mulberry32 (Rust 版の Rng と同じ)。シード k の旧版と render --seed k は同じ乱数列を使う
//   - AudioContext: OfflineAudioContext (実時間より速く、毎回同じ結果になる)
//   - setInterval(tick, 30): ctx.suspend() で 30ms ごと (128 サンプル境界に切り上げ) に呼ぶ。Rust 版の tick と同じ時刻
// 起動から 1.5 秒 (128 サンプル境界に切り上げ) 空回ししてから押す (render の preroll と同じ)
import { chromium } from '@playwright/test';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { resolve } from 'node:path';

const args = Object.fromEntries(process.argv.slice(2).reduce((a, x, i, xs) => (x.startsWith('--') ? [...a, [x.slice(2), xs[i + 1]]] : a), []));
const html = resolve(args.html ?? 'tools/legacy-index.html');
const out = resolve(args.out ?? 'reference/legacy');
const takes = +(args.takes ?? 3);
const hold = +(args.hold ?? 2);
const tail = +(args.tail ?? 1.5);
const sr = +(args.sr ?? 44100);
const [p0, p1] = (args.pads ?? '1-16').split('-').map(Number);
const parallel = +(args.parallel ?? 4);
const params = (args.params ?? '').split(',').filter(Boolean).map(x => x.split('='));
// --tap voice|dist|crush|phaser: 途中の段をそのまま録る (render --tap と比べる)
const tap = args.tap;
mkdirSync(out, { recursive: true });

function initScript({ seed, len, sr }) {
  let a = seed >>> 0;
  Math.random = () => {
    a = (a + 0x6D2B79F5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const OAC = window.OfflineAudioContext;
  window.AudioContext = function () {
    const c = new OAC(2, len, sr);
    // 旧版は音声の解錠で resume() を呼ぶ。描画の再開は harness だけが行う
    c.__resume = c.resume.bind(c);
    c.resume = () => Promise.resolve();
    window.__ctx = c;
    return c;
  };
  const addModule = AudioWorklet.prototype.addModule;
  AudioWorklet.prototype.addModule = function (...xs) { return (window.__mod = addModule.apply(this, xs)); };
  const si = window.setInterval;
  window.setInterval = (f, ms, ...rest) => (ms === 30 ? ((window.__tick = f), 0) : si(f, ms, ...rest));
}

function wav(l, r, sr) {
  const n = l.length, buf = Buffer.alloc(44 + n * 8);
  buf.write('RIFF', 0); buf.writeUInt32LE(36 + n * 8, 4); buf.write('WAVE', 8);
  buf.write('fmt ', 12); buf.writeUInt32LE(16, 16); buf.writeUInt16LE(3, 20); buf.writeUInt16LE(2, 22);
  buf.writeUInt32LE(sr, 24); buf.writeUInt32LE(sr * 8, 28); buf.writeUInt16LE(8, 32); buf.writeUInt16LE(32, 34);
  buf.write('data', 36); buf.writeUInt32LE(n * 8, 40);
  for (let i = 0; i < n; i++) { buf.writeFloatLE(l[i], 44 + i * 8); buf.writeFloatLE(r[i], 48 + i * 8); }
  return buf;
}

const q = t => Math.ceil(t * sr / 128 - 1e-9) * 128;
const pre = q(1.5);
const len = pre + q(hold + tail) + 128;

async function capture(browser, pad, seed) {
  const page = await browser.newPage();
  if (process.env.DEBUG) { page.on('console', m => console.log('[page]', m.text())); page.on('pageerror', e => console.log('[err]', e.message)); }
  await page.addInitScript(initScript, { seed, len, sr });
  await page.goto(`http://127.0.0.1:${port}/`);
  for (const [id, v] of params) await page.locator(`#s_${id}`).fill(v);
  const res = await page.evaluate(async ({ pad, pre, len, sr, holdF, endF }) => {
    // ページ上の最初の操作で音声が作られる (旧版の unlockAudio)
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    const ctx = window.__ctx;
    await window.__mod;
    await new Promise(ok => setTimeout(ok, 0));   // addModule().then() の中の接続を先に済ませる
    // フレーム → [押下/離鍵..., tick?]。同じ描画単位で suspend は1回しかできないのでまとめる
    const at = new Map();
    const add = (f, x) => { if (f < len) { if (!at.has(f)) at.set(f, []); at.get(f).push(x); } };
    add(pre, 'on'); add(pre + holdF, 'off');
    for (let k = 1; ; k++) { const f = Math.ceil(k * 0.03 * sr / 128 - 1e-9) * 128; if (f >= len) break; add(f, 'tick'); }
    for (const [f, xs] of [...at].sort((a, b) => a[0] - b[0])) {
      ctx.suspend(f / sr).then(() => {
        // 押下 / 離鍵を先に、tick を後に (Rust 版も同じ順)
        for (const x of xs) {
          if (x === 'on') window.stair.press(pad, 'cap');
          if (x === 'off') window.stair.release(pad, 'cap');
        }
        if (xs.includes('tick')) window.__tick();
        ctx.__resume();
      });
    }
    const buf = await ctx.startRendering();
    const enc = f => { const u = new Uint8Array(f.buffer); let s = ''; for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000)); return btoa(s); };
    return { l: enc(buf.getChannelData(0).slice(pre, pre + endF)), r: enc(buf.getChannelData(1).slice(pre, pre + endF)) };
  }, { pad, pre, len, sr, holdF: q(hold), endF: Math.round((hold + tail) * sr) });
  await page.close();
  const dec = s => { const b = Buffer.from(s, 'base64'); return new Float32Array(b.buffer, b.byteOffset, b.length / 4); };
  const name = `p${String(pad + 1).padStart(2, '0')}-s${seed}.wav`;
  writeFileSync(resolve(out, name), wav(dec(res.l), dec(res.r), sr));
  console.log(name);
}

let page_html = readFileSync(html, 'utf8');
if (tap) {
  const from = 'fx.sum.connect(comp).connect(fx.master).connect(clip).connect(ctx.destination);';
  if (!page_html.includes(from)) throw new Error('tap: 出力段の接続が見つからない');
  const node = { voice: 'voiceBus', dist: 'fx.distOut', crush: 'fx.crushOut', phaser: 'fx.phOut' }[tap];
  page_html = page_html.replace(from, `${node}.connect(ctx.destination);`);
}
const server = createServer((req, res) => { res.setHeader('Content-Type', 'text/html; charset=utf-8'); res.end(page_html); });
await new Promise(ok => server.listen(0, '127.0.0.1', ok));
const port = server.address().port;
const browser = await chromium.launch();
const jobs = [];
for (let pad = p0 - 1; pad < p1; pad++) for (let s = 1; s <= takes; s++) jobs.push([pad, s]);
await Promise.all(Array.from({ length: parallel }, async () => {
  while (jobs.length) { const [pad, s] = jobs.shift(); await capture(browser, pad, s); }
}));
await browser.close();
server.close();
