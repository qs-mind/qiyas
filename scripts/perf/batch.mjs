// usage: node batch.mjs <baseUrl> <label> <batchDir> [runs]  -- a real engine batch through the faked IPC
import { createRequire } from 'module'; import fs from 'fs';
// Playwright is not a dependency of the app: PLAYWRIGHT_FROM is the package.json of any folder that has it installed.
const require = createRequire(process.env.PLAYWRIGHT_FROM ?? new URL('../../package.json', import.meta.url).pathname); const { chromium } = require('playwright');
const here = new URL('.', import.meta.url).pathname;
const stub = fs.readFileSync(here + 'stub.js', 'utf8');
const [base, label, dir, runs = '3'] = process.argv.slice(2);
const patch = `
(() => { const inv = window.__TAURI_INTERNALS__.invoke; window.__marks = { stacks: [] };
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    if (cmd === 'render_batch') { __marks.n = (__marks.n || 0) + 1; __marks.stacks.push(performance.now().toFixed(0) + ' ' + JSON.stringify(args) + ' ' + new Error().stack.split('\\n').slice(2, 7).map((x) => x.trim().replace(/\\(http[^)]*\\/src\\//, '(')).join(' < ')); if (!__marks.req) __marks.req = performance.now(); const b = await (await fetch('/__qbatch/batch.bin')).arrayBuffer(); if (!__marks.res) __marks.res = performance.now(); return b; }
    if (cmd === 'atlas_pages') return await (await fetch('/__qbatch/atlas.bin')).arrayBuffer();
    if (cmd === 'batch_version') return 7;
    return inv(cmd, args); };
  window.__lt = []; new PerformanceObserver((l) => l.getEntries().forEach((e) => __lt.push([e.startTime, e.duration]))).observe({ type: 'longtask', buffered: true });
  window.__frames = []; let last = 0; const f = (t) => { if (last) __frames.push(t - last); last = t; requestAnimationFrame(f); }; requestAnimationFrame(f);
})();`;
const med = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
const browser = await chromium.launch({ args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
const rows = [];
try {
  for (let i = 0; i < +runs; i++) {
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' });
    await ctx.addInitScript(stub + patch);
    await ctx.route('**/__qbatch/*', (r) => r.fulfill({ path: dir + '/' + new URL(r.request().url()).pathname.split('/').pop(), headers: { 'content-type': 'application/octet-stream' } }));
    const page = await ctx.newPage();
    const cdp = await ctx.newCDPSession(page); await cdp.send('Performance.enable');
    await page.goto(`${base}/?state=chat`); await page.waitForFunction(() => window.__marks.res, null, { timeout: 60000 });
    await page.waitForTimeout(4000);
    const r = await page.evaluate(() => { const m = __marks; const after = __lt.filter(([s]) => s >= m.res - 5); const gl = document.querySelector('canvas'); return { fetchMs: Math.round(m.res - m.req), longAfter: after.length, longSum: Math.round(after.reduce((s, [, d]) => s + d, 0)), longMax: Math.round(Math.max(0, ...after.map(([, d]) => d))), canvas: !!gl, renderBatchCalls: m.n, stacks: m.stacks }; });
    const heap = (await cdp.send('Performance.getMetrics')).metrics.find((x) => x.name === 'JSHeapUsedSize').value / 1048576;
    // zoom and pan with the wheel and a drag on the canvas
    await page.evaluate(() => { window.__frames.length = 0; });
    const c = page.locator('canvas').first(); const bb = await c.boundingBox(); const cx = bb.x + bb.width / 2, cy = bb.y + bb.height / 2;
    await page.mouse.move(cx, cy);
    for (let k = 0; k < 30; k++) { await page.mouse.wheel(0, k < 15 ? -120 : 120); await page.waitForTimeout(16); }
    await page.mouse.down(); for (let k = 0; k < 40; k++) await page.mouse.move(cx + k * 6, cy + k * 3); await page.mouse.up();
    await page.waitForTimeout(500);
    const fr = await page.evaluate(() => __frames.slice());
    const busy = fr.filter((d) => d > 20);
    rows.push({ ...r, heapMB: +heap.toFixed(0), frames: fr.length, medFrameMs: +med(fr).toFixed(1), worstFrameMs: Math.round(Math.max(...fr)), framesOver50ms: fr.filter((d) => d > 50).length });
    await ctx.close();
  }
} finally { await browser.close(); }
console.log(`== ${label} batch ${dir.split('/').pop()} (${rows.length} runs; median [min..max]) -- Chromium + SwiftShader (software GL): draw times are NOT a GPU's`);
for (const k of Object.keys(rows[0])) { const v = rows.map((r) => r[k]); if (k === 'stacks') { console.log(v[0].join('\n')); continue; } if (typeof v[0] === 'boolean') { console.log(k.padEnd(14), v.join(',')); continue; } console.log(k.padEnd(14), String(med(v)).padStart(8), `[${Math.min(...v)}..${Math.max(...v)}]`); }
