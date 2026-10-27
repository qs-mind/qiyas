// usage: node inter.mjs <baseUrl> <label> <query> [runs]
import { createRequire } from 'module'; import fs from 'fs';
// Playwright is not a dependency of the app: PLAYWRIGHT_FROM is the package.json of any folder that has it installed.
const require = createRequire(process.env.PLAYWRIGHT_FROM ?? new URL('../../package.json', import.meta.url).pathname); const { chromium } = require('playwright');
const here = new URL('.', import.meta.url).pathname;
const stub = fs.readFileSync(here + 'stub.js', 'utf8') + fs.readFileSync(here + 'bigdata.js', 'utf8');
const [base, label, query, runs = '3'] = process.argv.slice(2);
const install = `
window.__inp = []; window.__lt = [];
new PerformanceObserver((l) => l.getEntries().forEach((e) => __lt.push(Math.round(e.duration)))).observe({ type: 'longtask', buffered: true });
for (const type of ['click', 'keydown', 'input']) document.addEventListener(type, () => { const t0 = performance.now(); requestAnimationFrame(() => requestAnimationFrame(() => __inp.push([type, performance.now() - t0]))); }, true);`;
const med = (a) => a.length ? [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)] : NaN;
const out = {};
const browser = await chromium.launch();
async function step(page, name, fn) {
  await page.evaluate(() => { window.__inp.length = 0; window.__lt.length = 0; });
  const t = Date.now(); await fn(); await page.waitForTimeout(250);
  const r = await page.evaluate(() => ({ inp: window.__inp.map((x) => x[1]), lt: window.__lt }));
  (out[name] ??= []).push({ worst: Math.max(...r.inp, 0), sum: r.inp.length, longMax: Math.max(0, ...r.lt), nLong: r.lt.length });
}
try {
  for (let i = 0; i < +runs; i++) {
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' });
    await ctx.addInitScript(stub + install);
    const page = await ctx.newPage();
    const t0 = Date.now();
    await page.goto(`${base}/?${query}`); await page.waitForSelector('[role=tab]'); await page.waitForTimeout(1500);
    (out.treeitems ??= []).push({ worst: await page.locator('[role=treeitem]').count() });
    (out.domNodes ??= []).push({ worst: await page.evaluate(() => document.getElementsByTagName('*').length) });
    await step(page, 'ribbon tab Draw', () => page.getByRole('tab', { name: 'Draw' }).click());
    await step(page, 'ribbon tab Annotate', () => page.getByRole('tab', { name: 'Annotate' }).click());
    await step(page, 'ribbon tab Measure', () => page.getByRole('tab', { name: 'Measure' }).click());
    await step(page, 'layer visible toggle', () => page.getByRole('button', { name: 'Visible: A-03-LAYER-3', exact: true }).click({ force: true }));
    await step(page, 'layer isolate', () => page.getByRole('button', { name: 'Isolate: A-05-LAYER-5', exact: true }).click({ force: true }));
    await step(page, 'Outputs menu open', () => page.getByRole('button', { name: /^Outputs/ }).click());
    await page.keyboard.press('Escape'); await page.waitForTimeout(300);
    await step(page, 'open Take-off output', async () => { await page.getByRole('button', { name: /^Outputs/ }).click(); await page.getByRole('menuitem', { name: /^Take-off \d/ }).click(); await page.waitForTimeout(600); });
    const hdr = page.locator('.grid-sort').first();
    if (await hdr.count()) {
      await step(page, 'sort take-off column', () => hdr.click());
      await step(page, 'sort take-off column again', () => hdr.click());
      await step(page, 'wheel-scroll take-off table', async () => { const g = page.locator('.grid').first(); const bb = await g.boundingBox(); if (bb) { await page.mouse.move(bb.x + bb.width / 2, bb.y + bb.height / 2); for (let k = 0; k < 10; k++) { await page.mouse.wheel(0, 600); await page.waitForTimeout(30); } } });
    }
    await step(page, 'open Bill of quantities', async () => { await page.getByRole('button', { name: /^Outputs/ }).click(); await page.getByRole('menuitem', { name: /^Bill of quantities \d/ }).click(); await page.waitForTimeout(600); });
    const box = page.locator('textarea').first();
    if (await box.count()) {
      await box.click();
      await step(page, 'type 20 chars in Mizan composer', async () => { await page.keyboard.type('Measure the ground floor walls now', { delay: 15 }); });
      await step(page, 'wheel-scroll Mizan thread', async () => { await page.mouse.move(1200, 400); for (let k = 0; k < 10; k++) { await page.mouse.wheel(0, -400); await page.waitForTimeout(30); } });
    }
    await step(page, 'theme toggle (Dark)', () => page.getByRole('button', { name: 'Dark' }).click());
    await ctx.close();
  }
} finally { await browser.close(); }
console.log(`== ${label} ${query}  (median over ${runs} runs of the worst next-paint delay in the step, ms; long task max ms)`);
for (const [k, v] of Object.entries(out)) console.log(k.padEnd(34), String(Math.round(med(v.map((x) => x.worst)))).padStart(7), v[0].longMax !== undefined ? `  long max ${Math.round(med(v.map((x) => x.longMax)))}` : '');
fs.writeFileSync(`${process.env.PERF_OUT ?? '/tmp'}/inter-${label}.json`, JSON.stringify(out));
