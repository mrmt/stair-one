import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/index.html');
});

test('パッドが4x4に並ぶ', async ({ page }) => {
  const pads = page.locator('.pad');
  await expect(pads).toHaveCount(16);
  const boxes = await pads.evaluateAll(els => els.map(e => e.getBoundingClientRect()).map(r => ({ x: Math.round(r.x), y: Math.round(r.y) })));
  expect(new Set(boxes.map(b => b.x)).size).toBe(4);
  expect(new Set(boxes.map(b => b.y)).size).toBe(4);
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

test('スライダの値が表示に反映される', async ({ page }) => {
  const out = page.locator('output[for="s_attack"]');
  await page.locator('#s_attack').fill('100');
  await expect(out).toHaveText('4.00s');
  await page.locator('#s_volume').fill('50');
  await expect(page.locator('output[for="s_volume"]')).toHaveText('50');
});

test('狭幅ではパッドの下にスライダが来る', async ({ page, viewport }) => {
  test.skip(viewport.width > 760, '狭幅のみ');
  const grid = await page.locator('#grid').boundingBox();
  const ctl = await page.locator('.controls').boundingBox();
  expect(ctl.y).toBeGreaterThanOrEqual(grid.y + grid.height);
});
