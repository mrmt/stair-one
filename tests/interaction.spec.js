import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page, browserName }) => {
  // CI (ubuntu) の WebKit は AudioContext を動かすとページごと固まることがある
  // (debug ブランチで mobile-webkit を16回ずつ回し、発音を伴うテストが数回タイムアウト)。
  // このファイルは画面と操作だけを見るので WebKit では Web Audio を外す。音は audio-chromium で見る
  if (browserName === 'webkit') {
    await page.addInitScript(() => { window.AudioContext = undefined; window.webkitAudioContext = undefined; });
  }
  await page.goto('/index.html');
});

test('パッドが4x4に並ぶ', async ({ page }) => {
  const pads = page.locator('.pad');
  await expect(pads).toHaveCount(16);
  const boxes = await pads.evaluateAll(els => els.map(e => e.getBoundingClientRect()).map(r => ({ x: Math.round(r.x), y: Math.round(r.y) })));
  expect(new Set(boxes.map(b => b.x)).size).toBe(4);
  expect(new Set(boxes.map(b => b.y)).size).toBe(4);
});

test('パッド名とつまみの範囲・初期値がエンジン (engine/) の定義と一致する', async ({ page }) => {
  const meta = await page.evaluate(() => window.stair.meta());
  await expect(page.locator('.pad .nm')).toHaveText(meta.pads);
  for (const d of meta.params) {
    const input = page.locator(`#s_${d.id}`);
    const attr = await input.evaluate(el => ({ min: +el.min, max: +el.max, step: el.step ? +el.step : 1, value: +el.defaultValue }));
    expect(attr, d.id).toEqual({ min: d.min, max: d.max, step: d.step, value: d.default });
  }
  expect(await page.locator('.knob input').count()).toBe(meta.params.length);
});

test('タイトルの左に親ディレクトリへ戻るアイコンがある', async ({ page }) => {
  const home = page.locator('header a.home');
  await expect(home).toBeVisible();
  await expect(home).toHaveAttribute('href', '../');
  await expect(home.locator('svg')).toHaveCount(1);
  const icon = await home.boundingBox();
  const title = await page.locator('header .mark').boundingBox();
  expect(icon.width).toBeGreaterThan(8);
  expect(icon.x + icon.width).toBeLessThanOrEqual(title.x);
});

test('横スクロールが出ない', async ({ page }) => {
  const over = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(over).toBeLessThanOrEqual(0);
});

test('ポインタを押している間だけ点灯する', async ({ page }) => {
  const pad = page.locator('.pad').nth(5);
  await pad.dispatchEvent('pointerdown', { pointerId: 7, bubbles: true });
  await expect(pad).toHaveAttribute('aria-pressed', 'true');
  await pad.dispatchEvent('pointerup', { pointerId: 7, bubbles: true });
  await expect(pad).toHaveAttribute('aria-pressed', 'false');
});

test('複数ポインタの同時押しは片方を離しても他方が鳴り続ける', async ({ page }) => {
  const a = page.locator('.pad').nth(0);
  const b = page.locator('.pad').nth(1);
  await a.dispatchEvent('pointerdown', { pointerId: 1, bubbles: true });
  await b.dispatchEvent('pointerdown', { pointerId: 2, bubbles: true });
  await a.dispatchEvent('pointerup', { pointerId: 1, bubbles: true });
  await expect(a).toHaveAttribute('aria-pressed', 'false');
  await expect(b).toHaveAttribute('aria-pressed', 'true');
  await b.dispatchEvent('pointerup', { pointerId: 2, bubbles: true });
  await expect(b).toHaveAttribute('aria-pressed', 'false');
});

test('つまみの値が表示に反映される', async ({ page }) => {
  const out = page.locator('output[for="s_attack"]');
  await page.locator('#s_attack').fill('100');
  await expect(out).toHaveText('4.00s');
  await page.locator('#s_volume').fill('50');
  await expect(page.locator('output[for="s_volume"]')).toHaveText('50');
  await page.locator('#s_pitch').fill('-3.5');
  await expect(page.locator('output[for="s_pitch"]')).toHaveText('-3.5 st');
});

test('つまみ8個が2列4行で指定の順に並び、distortion と volume は別に置かれる', async ({ page }) => {
  const main = page.locator('.knobs:not(.small) .ctl');
  await expect(main).toHaveCount(8);
  const items = await main.evaluateAll(els => els.map(e => {
    const r = e.getBoundingClientRect();
    return { id: e.querySelector('input').id, x: Math.round(r.x), y: Math.round(r.y) };
  }));
  const xs = [...new Set(items.map(i => i.x))].sort((a, b) => a - b);
  expect(xs).toHaveLength(2);
  expect(new Set(items.map(i => i.y)).size).toBe(4);
  const column = x => items.filter(i => i.x === x).sort((a, b) => a.y - b.y).map(i => i.id);
  expect(column(xs[0])).toEqual(['s_attack', 's_decay', 's_release', 's_pitch']);
  expect(column(xs[1])).toEqual(['s_dtime', 's_dfb', 's_dmix', 's_phaser']);

  const small = await page.locator('.knobs.small .ctl input').evaluateAll(els => els.map(e => e.id));
  expect(small).toEqual(['s_drive', 's_volume']);
  await expect(page.locator('#s_crush')).toHaveCount(0);
});

test('デスクトップではつまみがパッドの左にある', async ({ page, viewport }) => {
  test.skip(viewport.width <= 760, 'デスクトップ幅のみ');
  const ctl = await page.locator('.controls').boundingBox();
  const grid = await page.locator('#grid').boundingBox();
  expect(ctl.x + ctl.width).toBeLessThanOrEqual(grid.x);
});

test('つまみを上へドラッグすると値が増え、ダブルクリックで既定値に戻る', async ({ page, hasTouch }) => {
  test.skip(hasTouch, 'マウスのあるプロファイルのみ');
  const input = page.locator('#s_dmix');
  const knob = page.locator('.ctl', { has: input }).locator('.knob');
  const box = await knob.boundingBox();
  const cx = box.x + box.width / 2, cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx, cy - 60, { steps: 6 });
  await page.mouse.up();
  const v = Number(await input.inputValue());
  expect(v).toBeGreaterThan(50);
  await expect(page.locator('output[for="s_dmix"]')).toHaveText(String(v));

  // 下へドラッグすると減る
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx, cy + 30, { steps: 4 });
  await page.mouse.up();
  expect(Number(await input.inputValue())).toBeLessThan(v);

  await knob.dblclick();
  await expect(input).toHaveValue('25');
});

test('狭幅ではパッドの下につまみが来る', async ({ page, viewport }) => {
  test.skip(viewport.width > 760, '狭幅のみ');
  const grid = await page.locator('#grid').boundingBox();
  const ctl = await page.locator('.controls').boundingBox();
  expect(ctl.y).toBeGreaterThanOrEqual(grid.y + grid.height);
});

test.describe('プラグイン (JUCE の WebView) の中', () => {
  test.beforeEach(async ({ page }) => {
    // JUCE の native integration (window.__JUCE__.backend) の偽物
    await page.addInitScript(() => {
      const listeners = [];
      window.__sent = [];
      window.__JUCE__ = { backend: {
        emitEvent: (id, m) => window.__sent.push({ id, ...m }),
        addEventListener: (id, fn) => listeners.push(fn),
      } };
      window.__fromHost = m => listeners.forEach(fn => fn(m));
    });
    await page.goto('/index.html');
  });

  test('準備ができたら知らせ、押下とつまみをホストへ送る', async ({ page }) => {
    await expect.poll(() => page.evaluate(() => window.__sent.map(m => m.t))).toContain('ready');
    await expect(page.locator('#midiLearn')).toBeHidden();
    await expect(page.locator('header a.home')).toBeHidden();

    const pad = page.locator('.pad').nth(3);
    await pad.dispatchEvent('pointerdown', { pointerId: 3, bubbles: true });
    await pad.dispatchEvent('pointerup', { pointerId: 3, bubbles: true });
    await page.locator('#s_drive').fill('60');
    const sent = await page.evaluate(() => window.__sent.filter(m => m.t !== 'ready'));
    expect(sent).toEqual([
      { id: 'stair', t: 'on', pad: 3 },
      { id: 'stair', t: 'off', pad: 3 },
      { id: 'stair', t: 'param', i: 8, v: 60 },
    ]);
    // Web Audio は使わない
    expect(await page.evaluate(() => window.stair.voiceCount())).toBe(0);
  });

  test('ホストのつまみとパッドの点灯を画面に映し、送り返さない', async ({ page }) => {
    await page.evaluate(() => window.__fromHost({ t: 'param', i: 9, v: 33 }));
    await expect(page.locator('#s_volume')).toHaveValue('33');
    await expect(page.locator('output[for="s_volume"]')).toHaveText('33');
    await page.evaluate(() => window.__fromHost({ t: 'held', mask: 0b101 }));
    await expect(page.locator('.pad').nth(0)).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.pad').nth(1)).toHaveAttribute('aria-pressed', 'false');
    await expect(page.locator('.pad').nth(2)).toHaveAttribute('aria-pressed', 'true');
    expect(await page.evaluate(() => window.__sent.filter(m => m.t === 'param'))).toEqual([]);
  });

  test('キーボードでは弾かない (ホストのショートカットに任せる)', async ({ page }) => {
    await page.keyboard.down('q');
    await expect(page.locator('.pad').nth(4)).toHaveAttribute('aria-pressed', 'false');
    expect(await page.evaluate(() => window.__sent.filter(m => m.t === 'on'))).toEqual([]);
  });
});
