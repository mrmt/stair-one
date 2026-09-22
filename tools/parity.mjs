// 部品単位で Chromium (OfflineAudioContext) と Rust の出力を比べる
//   node tools/parity.mjs
import { chromium } from '@playwright/test';
import { execFileSync } from 'node:child_process';

const SR = 44100, N = SR;
// 決まった乱数のノイズ (mulberry32)
function noise(seed, n, amp = 0.5) {
  let a = seed >>> 0; const out = new Array(n);
  for (let i = 0; i < n; i++) {
    a = (a + 0x6D2B79F5) >>> 0; let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    out[i] = (((t ^ (t >>> 14)) >>> 0) / 4294967296 * 2 - 1) * amp;
  }
  return out;
}
// コンプ用: 音量が段階的に変わるノイズ
const env = i => [0.05, 0.9, 0.2, 1.5, 0.02][Math.floor(i / (N / 5))];
const compL = noise(3, N).map((v, i) => v * env(i));
const compR = noise(4, N).map((v, i) => v * env(i) * 0.7);

const cases = {
  saw110: {}, square440: {}, saw3000: {}, sawsweep: {},
  lp: { input: noise(1, N) }, lpq: { input: noise(1, N) }, hp: { input: noise(1, N) }, bp: { input: noise(1, N) }, ap: { input: noise(1, N) },
  comp: { input: compL, input2: compR },
  delay: { input: noise(2, N) }, delay2: { input: Array.from({ length: N }, (_, i) => Math.sin(2 * Math.PI * 440 * i / SR)) }, delayc: { input: noise(2, N) },
  karplus: { input: noise(8, N).map((v, i) => (i < 441 ? v : 0)) }, karplus2: { input: noise(8, N).map((v, i) => (i < 441 ? v : 0)) }, karplus3: { input: noise(8, N).map((v, i) => (i < 441 ? v : 0)) },
  shaper: { input: noise(5, N, 0.8) },
  // 高域の少ない入力 (220Hz の正弦 + 少しの倍音) で、エフェクト由来の高域を見る
  fxchain_lf: { input: Array.from({ length: N }, (_, i) => 0.4 * Math.sin(2 * Math.PI * 220 * i / SR) + 0.05 * Math.sin(2 * Math.PI * 660 * i / SR)), input2: Array.from({ length: N }, (_, i) => 0.3 * Math.sin(2 * Math.PI * 220 * i / SR)) },
  fxdelay: { input: Array.from({ length: N }, (_, i) => (i < N / 2 ? 0.4 * Math.sin(2 * Math.PI * 220 * i / SR) : 0)), input2: Array.from({ length: N }, (_, i) => (i < N / 2 ? 0.3 * Math.sin(2 * Math.PI * 330 * i / SR) : 0)) },
  fxchain: { input: noise(6, N, 0.6).map((v, i) => v * env(i)), input2: noise(7, N, 0.6).map((v, i) => v * env(i)) },
};

import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
const server = createServer((q, r) => { r.setHeader('Content-Type', 'text/html; charset=utf-8'); r.end(readFileSync(process.env.HTML ?? 'tools/legacy-index.html')); });
await new Promise(ok => server.listen(0, '127.0.0.1', ok));
const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${server.address().port}/`);
let fail = 0;
for (const [name, c] of Object.entries(cases)) {
  const chrome = await page.evaluate(async ({ name, c, SR, N }) => {
    const ctx = new OfflineAudioContext(2, N, SR);
    const src = arr => { const b = ctx.createBuffer(1, N, SR); b.getChannelData(0).set(arr); const s = ctx.createBufferSource(); s.buffer = b; s.start(); return s; };
    const out = ctx.destination;
    const merger = ctx.createChannelMerger(2); merger.connect(out);
    if (name.startsWith('saw') || name.startsWith('square')) {
      const o = ctx.createOscillator();
      o.type = name.startsWith('saw') ? 'sawtooth' : 'square';
      if (name === 'sawsweep') { o.frequency.setValueAtTime(50, 0); o.frequency.exponentialRampToValueAtTime(5000, N / SR); }
      else o.frequency.value = +name.replace(/\D/g, '');
      o.connect(merger, 0, 0); o.start();
    } else if (['lp', 'lpq', 'hp', 'bp', 'ap'].includes(name)) {
      const f = ctx.createBiquadFilter();
      const [type, fr, q] = { lp: ['lowpass', 800, 6], lpq: ['lowpass', 3000, -6], hp: ['highpass', 150, 2], bp: ['bandpass', 1000, 5], ap: ['allpass', 900, .7] }[name];
      f.type = type; f.frequency.value = fr; f.Q.value = q;
      src(c.input).connect(f).connect(merger, 0, 0);
    } else if (name === 'comp') {
      const m = ctx.createChannelMerger(2);
      src(c.input).connect(m, 0, 0); src(c.input2).connect(m, 0, 1);
      const k = ctx.createDynamicsCompressor();
      k.threshold.value = -18; k.knee.value = 12; k.ratio.value = 6; k.attack.value = .005; k.release.value = .2;
      m.connect(k).connect(out);
    } else if (name.startsWith('karplus')) {
      const f = name === 'karplus2' ? 450 : 200;
      const dl = ctx.createDelay(1); dl.delayTime.value = 1 / f;
      if (name === 'karplus3') dl.delayTime.setTargetAtTime(1 / 205, Math.floor(.05 * SR) / SR, .02);
      const lp = ctx.createBiquadFilter(); lp.frequency.value = 5000; lp.Q.value = -6;
      const sat = ctx.createWaveShaper(); sat.curve = Float32Array.from({ length: 1025 }, (_, i) => Math.tanh(i / 512 - 1));
      const fb = ctx.createGain(); fb.gain.value = .98;
      src(c.input).connect(dl); dl.connect(lp).connect(sat).connect(fb).connect(dl);
      dl.connect(merger, 0, 0);
    } else if (name === 'delay2') {
      const d = ctx.createDelay(2.5); d.delayTime.value = .18; d.delayTime.setTargetAtTime(.2, 0, .2);
      src(c.input).connect(d).connect(merger, 0, 0);
    } else if (name === 'delayc') {
      const d = ctx.createDelay(1); d.delayTime.value = .0101;
      src(c.input).connect(d).connect(merger, 0, 0);
    } else if (name === 'delay') {
      const d = ctx.createDelay(1); d.delayTime.value = .005; d.delayTime.setTargetAtTime(.02, 0, .05);
      src(c.input).connect(d).connect(merger, 0, 0);
    } else if (name.startsWith('fxchain') || name === 'fxdelay') {
      // 旧 index.html の ensureAudio() / applyFx() と同じグラフ (ディレイは切る)
      const html = await (await fetch(location.href)).text();
      const crusherSrc = html.match(/const CRUSHER_SRC = `([\s\S]*?)`;/)[1];
      await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([crusherSrc], { type: 'application/javascript' })));
      const g = v => { const n = ctx.createGain(); n.gain.value = v; return n; };
      const m = ctx.createChannelMerger(2);
      src(c.input).connect(m, 0, 0); src(c.input2).connect(m, 0, 1);
      const voiceBus = g(.6); m.connect(voiceBus);
      const t = 0, k = .05;
      const d = .15, dm = Math.min(1, d * 3);
      const n2 = 2048, cv = new Float32Array(n2), kk = 1 + d * d * 60, b = .15 * d, off = Math.tanh(kk * b), norm = Math.tanh(kk);
      for (let i = 0; i < n2; i++) { const x = i / (n2 - 1) * 2 - 1; cv[i] = (Math.tanh(kk * (x + b)) - off) / norm; }
      const distOut = g(1), distDry = g(1), distWet = g(0), shaper = ctx.createWaveShaper(); shaper.oversample = '4x'; shaper.curve = cv;
      voiceBus.connect(distDry).connect(distOut); voiceBus.connect(shaper).connect(distWet).connect(distOut);
      distDry.gain.setTargetAtTime(1 - dm, t, k); distWet.gain.setTargetAtTime(dm * .5, t, k);
      const crushOut = g(1), crushDry = g(1), crushWet = g(0);
      distOut.connect(crushDry).connect(crushOut); crushWet.connect(crushOut);
      const crusher = new AudioWorkletNode(ctx, 'crusher'); distOut.connect(crusher).connect(crushWet);
      const C = .2; crusher.parameters.get('bits').setTargetAtTime(16 - C * 14, t, k); crusher.parameters.get('down').setTargetAtTime(1 + C * C * 40, t, k);
      crushDry.gain.setTargetAtTime(1 - Math.min(1, C * 3), t, k); crushWet.gain.setTargetAtTime(Math.min(1, C * 3), t, k);
      const phOut = g(1), phDry = g(1), phWet = g(0), phIn = g(1);
      crushOut.connect(phDry).connect(phOut); crushOut.connect(phIn);
      const phLfo = ctx.createOscillator(); phLfo.frequency.value = .3; const phDepth = g(1800); phLfo.connect(phDepth); phLfo.start();
      let prev = phIn;
      for (const f of [250, 520, 900, 1500, 2400, 3600]) { const ap = ctx.createBiquadFilter(); ap.type = 'allpass'; ap.frequency.value = f; ap.Q.value = .7; phDepth.connect(ap.detune); prev.connect(ap); prev = ap; }
      const phFb = g(0), phFbDelay = ctx.createDelay(.01); phFbDelay.delayTime.value = .001;
      prev.connect(phFb).connect(phFbDelay).connect(phIn); prev.connect(phWet).connect(phOut);
      const ph = .2;
      phLfo.frequency.setTargetAtTime(.08 + ph * 1.5, t, k); phDepth.gain.setTargetAtTime(1200 + ph * 2400, t, k);
      phFb.gain.setTargetAtTime(ph * .7, t, k); phWet.gain.setTargetAtTime(ph, t, k); phDry.gain.setTargetAtTime(1 - ph * .5, t, k);
      const comp = ctx.createDynamicsCompressor(); comp.threshold.value = -18; comp.knee.value = 12; comp.ratio.value = 6; comp.attack.value = .005; comp.release.value = .2;
      const master = g(0); const vol = .8; master.gain.setTargetAtTime(vol * vol * 1.6, t, k);
      const clip = ctx.createWaveShaper(); const cc = new Float32Array(1024);
      for (let i = 0; i < cc.length; i++) { const x = i / (cc.length - 1) * 2 - 1; cc[i] = .95 * Math.tanh(2 * x) / Math.tanh(2); }
      clip.curve = cc;
      const sum = g(1); phOut.connect(sum);
      if (name === 'fxdelay') {
        // 旧版のステレオディレイ。ディレイタイムは揺らさない
        const dtm = .02 * Math.pow(1.2 / .02, 70 / 100);
        const dL = ctx.createDelay(2.5), dR = ctx.createDelay(2.5);
        const lpL = ctx.createBiquadFilter(), lpR = ctx.createBiquadFilter();
        lpL.frequency.value = lpR.frequency.value = 4500; lpL.Q.value = lpR.Q.value = -6;
        const fbL = g(0), fbR = g(0);
        phOut.connect(dL); phOut.connect(g(.7)).connect(dR);
        dL.connect(lpL).connect(fbL).connect(dR); dR.connect(lpR).connect(fbR).connect(dL);
        const mg = ctx.createChannelMerger(2); dL.connect(mg, 0, 0); dR.connect(mg, 0, 1);
        const dWet = g(0); mg.connect(dWet).connect(sum);
        fbL.gain.setTargetAtTime(.4, t, k); fbR.gain.setTargetAtTime(.4, t, k); dWet.gain.setTargetAtTime(.25, t, k);
        dL.delayTime.value = dtm * .9; dR.delayTime.value = dtm * .62 * .9; dL.delayTime.setTargetAtTime(dtm, t, .2); dR.delayTime.setTargetAtTime(dtm * .62, t, .2);
      }
      sum.connect(comp).connect(master).connect(clip).connect(out);
    } else if (name === 'shaper') {
      const n = 2048, cv = new Float32Array(n), a = .15, k = 1 + a * a * 60, b = .15 * a, off = Math.tanh(k * b), norm = Math.tanh(k);
      for (let i = 0; i < n; i++) { const x = i / (n - 1) * 2 - 1; cv[i] = (Math.tanh(k * (x + b)) - off) / norm; }
      const ws = ctx.createWaveShaper(); ws.curve = cv; ws.oversample = '4x';
      const s = src(c.input), dry = ctx.createGain(), wet = ctx.createGain(); dry.gain.value = .55; wet.gain.value = .225;
      s.connect(dry).connect(merger, 0, 0); s.connect(ws).connect(wet).connect(merger, 0, 0);
    }
    const r = await ctx.startRendering();
    return [Array.from(r.getChannelData(0)), Array.from(r.getChannelData(1))];
  }, { name, c, SR, N });
  const rust = JSON.parse(execFileSync('engine/target/release/examples/parity', { input: JSON.stringify({ case: name, sr: SR, n: N, ...c }), maxBuffer: 1 << 28 }).toString());
  const cmp = (a, b) => {
    let e = 0, s = 0, m = 0;
    for (let i = 0; i < b.length; i++) { const d = a[i] - b[i]; e += d * d; s += a[i] * a[i]; m = Math.max(m, Math.abs(d)); }
    return { snr: 10 * Math.log10(s / (e || 1e-30)), max: m };
  };
  if (process.env.DUMP === name) (await import('node:fs')).writeFileSync(`${process.env.DUMP_DIR ?? '.'}/${name}.json`, JSON.stringify({ chrome, rust: [rust.out, rust.out2] }));
  const r0 = cmp(chrome[0], rust.out);
  const r1 = rust.out2.length ? cmp(chrome[1], rust.out2) : null;
  // Chromium の DelayNode は読み位置を float32 で持つので、小数部の丸め分だけずれる
  const need = name.includes('delay') ? 25 : 60;
  const ok = r0.snr > need && (!r1 || r1.snr > need);
  if (!ok) fail++;
  console.log(`${ok ? 'ok  ' : 'NG  '}${name.padEnd(10)} SNR ${r0.snr.toFixed(1)}dB max ${r0.max.toExponential(2)}${r1 ? `  R: SNR ${r1.snr.toFixed(1)}dB` : ''}`);
}
await browser.close();
server.close();
process.exit(fail ? 1 : 0);
