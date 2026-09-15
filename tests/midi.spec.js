import { test, expect } from '@playwright/test';

// 実機がないので navigator.requestMIDIAccess を偽物にし、window.__midiSend で入力を注入する
async function fakeMidi(page) {
  await page.addInitScript(() => {
    const input = { id: 'fake', name: 'Fake Controller', type: 'input', state: 'connected', onmidimessage: null };
    const access = { inputs: new Map([['fake', input]]), outputs: new Map(), onstatechange: null };
    Object.defineProperty(Navigator.prototype, 'requestMIDIAccess', {
      configurable: true,
      value: () => Promise.resolve(access),
    });
    window.__midiSend = data => input.onmidimessage && input.onmidimessage({ data: Uint8Array.from(data) });
  });
}

const send = (page, data) => page.evaluate(d => window.__midiSend(d), data);
const voiceCount = page => page.evaluate(() => window.stair.voiceCount());
const midiMap = page => page.evaluate(() => window.stair.midiMap());
const sliderRow = (page, id) => page.locator('.ctl', { has: page.locator('#' + id) });

test.describe('Web MIDI あり', () => {
  test.beforeEach(async ({ page }) => {
    await fakeMidi(page);
    await page.goto('/index.html');
  });

  test('learn モードの切り替え。learn 中にパッドを押しても鳴らない', async ({ page }) => {
    const btn = page.locator('#midiLearn');
    await btn.click();
    await expect(btn).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('body')).toHaveClass(/learning/);
    await expect(page.locator('#midiClear')).toBeVisible();
    await expect(page.locator('#midiStatus')).toHaveText('MIDI 1 in');

    const pad = page.locator('.pad').nth(0);
    await pad.click();
    await expect(pad).toHaveClass(/learn-sel/);
    await expect(pad).toHaveAttribute('aria-pressed', 'false');
    expect(await voiceCount(page)).toBe(0);

    await page.keyboard.press('Escape');
    await expect(btn).toHaveAttribute('aria-pressed', 'false');
    await expect(page.locator('.learn-sel')).toHaveCount(0);
    await expect(page.locator('#midiClear')).toBeHidden();
  });

  test('パッドを note に割り当てて弾く', async ({ page }) => {
    await page.locator('#midiLearn').click();
    const pad = page.locator('.pad').nth(2);
    await pad.click();
    await send(page, [0x90, 36, 100]);
    await expect(pad.locator('.midi-badge')).toHaveText('N36');
    await expect(pad).not.toHaveClass(/learn-sel/);
    await page.locator('#midiLearn').click();

    await send(page, [0x90, 36, 100]);
    await expect(pad).toHaveAttribute('aria-pressed', 'true');
    expect(await voiceCount(page)).toBe(1);
    await send(page, [0x80, 36, 0]);
    await expect(pad).toHaveAttribute('aria-pressed', 'false');

    // velocity 0 の note on も note off として扱う
    await send(page, [0x90, 36, 90]);
    await expect(pad).toHaveAttribute('aria-pressed', 'true');
    await send(page, [0x90, 36, 0]);
    await expect(pad).toHaveAttribute('aria-pressed', 'false');
  });

  test('スライダを CC に割り当てて動かす', async ({ page }) => {
    await page.locator('#midiLearn').click();
    const row = sliderRow(page, 's_volume');
    await row.click();
    await expect(row).toHaveClass(/learn-sel/);
    // 行を押しただけではつまみは動かない
    await expect(page.locator('#s_volume')).toHaveValue('80');
    // スライダに note は割り当てない
    await send(page, [0x90, 40, 100]);
    await expect(row.locator('.midi-badge')).toHaveText('');
    await send(page, [0xb0, 7, 10]);
    await expect(row.locator('.midi-badge')).toHaveText('CC7');

    // ch 2 は表示に出る
    const drive = sliderRow(page, 's_drive');
    await drive.click();
    await send(page, [0xb1, 21, 0]);
    await expect(drive.locator('.midi-badge')).toHaveText('CC21 ch2');
    await page.locator('#midiLearn').click();

    await send(page, [0xb0, 7, 127]);
    await expect(page.locator('#s_volume')).toHaveValue('100');
    await expect(page.locator('output[for="s_volume"]')).toHaveText('100');
    await send(page, [0xb0, 7, 0]);
    await expect(page.locator('#s_volume')).toHaveValue('0');
    await send(page, [0xb1, 21, 127]);
    await expect(page.locator('#s_drive')).toHaveValue('100');
  });

  test('パッドを CC (ボタン型) で押す・離す', async ({ page }) => {
    await page.locator('#midiLearn').click();
    const pad = page.locator('.pad').nth(5);
    await pad.click();
    await send(page, [0xb0, 64, 127]);
    await expect(pad.locator('.midi-badge')).toHaveText('CC64');
    await page.locator('#midiLearn').click();

    await send(page, [0xb0, 64, 127]);
    await expect(pad).toHaveAttribute('aria-pressed', 'true');
    await send(page, [0xb0, 64, 0]);
    await expect(pad).toHaveAttribute('aria-pressed', 'false');
  });

  test('リロード後も割当が効く', async ({ page }) => {
    await page.locator('#midiLearn').click();
    await page.locator('.pad').nth(0).click();
    await send(page, [0x90, 40, 100]);
    await page.locator('#midiLearn').click();

    await page.reload();
    await expect(page.locator('.pad').nth(0).locator('.midi-badge')).toHaveText('N40');
    // learn を押さなくても保存済みの割当があれば MIDI を開く
    await expect(page.locator('#midiStatus')).toHaveText('MIDI 1 in');
    await send(page, [0x90, 40, 100]);
    await expect(page.locator('.pad').nth(0)).toHaveAttribute('aria-pressed', 'true');
  });

  test('再割当で旧割当が外れる / Delete / clear all', async ({ page }) => {
    await page.locator('#midiLearn').click();
    const [p0, p1] = [page.locator('.pad').nth(0), page.locator('.pad').nth(1)];

    await p0.click(); await send(page, [0x90, 36, 100]);
    await p1.click(); await send(page, [0x90, 36, 100]);
    expect(await midiMap(page)).toEqual({ 'note:1:36': 'pad:1' });
    await expect(p0.locator('.midi-badge')).toHaveText('');
    await expect(p1.locator('.midi-badge')).toHaveText('N36');

    // 同じ target に別の key → 置き換え
    await p1.click(); await send(page, [0x90, 37, 100]);
    expect(await midiMap(page)).toEqual({ 'note:1:37': 'pad:1' });

    await p1.click();
    await page.keyboard.press('Delete');
    expect(await midiMap(page)).toEqual({});

    await p0.click(); await send(page, [0x90, 50, 100]);
    await p1.click(); await send(page, [0xb0, 20, 100]);
    await page.locator('#midiClear').click();
    expect(await midiMap(page)).toEqual({});
    await expect(page.locator('.midi-badge:not(:empty)')).toHaveCount(0);
  });

  test('割当のない MIDI は無視する', async ({ page }) => {
    await send(page, [0x90, 60, 100]);
    await send(page, [0xb0, 1, 100]);
    await expect(page.locator('.pad[aria-pressed="true"]')).toHaveCount(0);
  });
});

test('Web MIDI 非対応ブラウザでは learn ボタンが無効', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, 'requestMIDIAccess', { configurable: true, value: undefined });
  });
  await page.goto('/index.html');
  await expect(page.locator('#midiLearn')).toBeDisabled();
});
