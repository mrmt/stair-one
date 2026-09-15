import { test, expect } from '@playwright/test';
import { installAudioProbe, readLevel, maxPeakOver } from './helpers/audio.js';

async function mouseDownOn(page, locator) {
  const box = await locator.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
}

// エフェクトの残響を切り、リリースを最短にして「止まったか」を測りやすくする
async function dryShort(page) {
  await page.locator('#s_release').fill('0');
  await page.locator('#s_dmix').fill('0');
  await page.locator('#s_dfb').fill('0');
}

test.beforeEach(async ({ page }) => {
  await installAudioProbe(page);
  await page.goto('/index.html');
});

test('押す前は発音していない', async ({ page }) => {
  expect(await readLevel(page)).toBeNull();
});

test('押している間鳴り、離すと止まる', async ({ page }) => {
  await dryShort(page);
  const pad = page.locator('.pad').nth(0);
  await mouseDownOn(page, pad);
  await expect(pad).toHaveAttribute('aria-pressed', 'true');
  const on = await maxPeakOver(page, 1500);
  expect(on.last.state).toBe('running');
  expect(on.peak).toBeGreaterThan(0.01);

  await page.mouse.up();
  await expect(pad).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => page.evaluate(() => window.stair.voiceCount())).toBe(0);
  await page.waitForTimeout(500);
  const off = await maxPeakOver(page, 800);
  expect(off.peak).toBeLessThan(on.peak / 10);
});

test('16パッドすべてが発音する', async ({ page }) => {
  test.setTimeout(90000);
  await dryShort(page);
  const pads = page.locator('.pad');
  await expect(pads).toHaveCount(16);
  for (let i = 0; i < 16; i++) {
    await mouseDownOn(page, pads.nth(i));
    const { peak } = await maxPeakOver(page, 1200, 100);
    await page.mouse.up();
    expect(peak, `pad ${i + 1}`).toBeGreaterThan(0.01);
    await expect.poll(() => page.evaluate(() => window.stair.voiceCount())).toBe(0);
  }
});

test('キーボードで発音し、フォーカスを失うと全部止まる', async ({ page }) => {
  await page.keyboard.down('q');
  await page.keyboard.down('v');
  await expect(page.locator('.pad').nth(4)).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.pad').nth(15)).toHaveAttribute('aria-pressed', 'true');
  const { peak } = await maxPeakOver(page, 1000);
  expect(peak).toBeGreaterThan(0.01);

  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await expect(page.locator('.pad[aria-pressed="true"]')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.stair.voiceCount()), { timeout: 15000 }).toBe(0);
});

test('同じパッドでも発音ごとにパラメータが変わる', async ({ page }) => {
  await dryShort(page);
  const snaps = [];
  for (let n = 0; n < 4; n++) {
    await page.evaluate(() => window.stair.press(0, 'test'));
    snaps.push(await page.evaluate(() => window.stair.debug().find(v => !v.released)));
    await page.evaluate(() => window.stair.release(0, 'test'));
  }
  const roots = new Set(snaps.map(s => s.root.toFixed(4)));
  const cuts = new Set(snaps.map(s => s.cut.toFixed(2)));
  expect(roots.size).toBeGreaterThan(1);
  expect(cuts.size).toBeGreaterThan(1);
});

test('発音中もパラメータが揺れる', async ({ page }) => {
  await page.evaluate(() => window.stair.press(0, 'test'));
  const a = await page.evaluate(() => window.stair.debug()[0].cut);
  await page.waitForTimeout(600);
  const b = await page.evaluate(() => window.stair.debug()[0].cut);
  expect(a).not.toBe(b);
});

test('pitch つまみで発音中の全ボイスの音程が上下する', async ({ page }) => {
  // ドリフトしないパッドで見る。Buzz 80Hz (osc: pitchCV → detune) と Comb Metal (karplus: 遅延時間を JS で計算)
  await page.evaluate(() => { window.stair.press(4, 'test'); window.stair.press(8, 'test'); });
  await page.waitForTimeout(400);
  const snap = () => page.evaluate(() => Object.fromEntries(window.stair.debug().map(v => [v.pad, { d: v.cents - v.pitch, cv: v.cv, delay: v.delay }])));

  const base = await snap();
  expect(Math.abs(base[4].d)).toBeLessThan(100);
  expect(Math.abs(base[8].d)).toBeLessThan(100);

  await page.locator('#s_pitch').fill('12');
  await expect(page.locator('output[for="s_pitch"]')).toHaveText('+12.0 st');
  await page.waitForTimeout(500);
  const up = await snap();
  for (const pad of [4, 8]) {
    expect(up[pad].d).toBeGreaterThan(1100);
    expect(up[pad].d).toBeLessThan(1300);
  }
  expect(up[4].cv).toBeGreaterThan(1000);                      // オシレータの detune に届いている
  expect(up[8].delay / base[8].delay).toBeGreaterThan(.4);     // 1オクターブ上 = 遅延時間が約半分
  expect(up[8].delay / base[8].delay).toBeLessThan(.6);

  await page.locator('#s_pitch').fill('-12');
  await page.waitForTimeout(500);
  const down = await snap();
  for (const pad of [4, 8]) expect(down[pad].d).toBeLessThan(-1100);
  expect(down[4].cv).toBeLessThan(-1000);
  expect(down[8].delay / base[8].delay).toBeGreaterThan(1.6);  // 1オクターブ下 = 約2倍
});

test('フィードバックを最大にしてもループが発散しない', async ({ page }) => {
  test.setTimeout(40000);
  // Chrome は BiquadFilter の状態が非有限になると警告を出す。発散の検出に使う
  let bad = 0;
  page.on('console', m => { if (/state is bad/.test(m.text())) bad++; });
  await page.locator('#s_dfb').fill('92');
  await page.locator('#s_dmix').fill('100');
  // くし形共鳴を持つパッドとカオス
  await page.evaluate(() => [8, 9, 14, 15].forEach(i => window.stair.press(i, 'test')));
  await page.waitForTimeout(12000);
  expect(bad).toBe(0);
});

test('音量・歪み最大で16パッド同時押しでもクリップしない', async ({ page }) => {
  await page.locator('#s_volume').fill('100');
  await page.locator('#s_drive').fill('100');
  await page.locator('#s_dfb').fill('92');
  await page.evaluate(() => { for (let i = 0; i < 16; i++) window.stair.press(i, 'test'); });
  const { peak } = await maxPeakOver(page, 4000, 100);
  expect(peak).toBeGreaterThan(0.05);
  expect(peak).toBeLessThan(1);
});
