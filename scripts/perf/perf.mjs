// usage: node perf.mjs <baseUrl> <label> [query] [runs]
import { createRequire } from 'module';
import fs from 'fs';
// Playwright is not a dependency of the app: PLAYWRIGHT_FROM is the package.json of any folder that has it installed.
const require = createRequire(process.env.PLAYWRIGHT_FROM ?? new URL('../../package.json', import.meta.url).pathname);
const { chromium } = require('playwright');
const here = new URL('.', import.meta.url).pathname;
const stub = fs.readFileSync(here + 'stub.js', 'utf8') + fs.readFileSync(here + 'bigdata.js', 'utf8');
const [base, label, query = 'state=chat', runs = '5'] = process.argv.slice(2);
const probe = `
window.__lt = []; window.__ready = null;
new PerformanceObserver((l) => l.getEntries().forEach((e) => __lt.push([Math.round(e.startTime), Math.round(e.duration)]))).observe({ type: 'longtask', buffered: true });
const check = () => { if (!window.__ready && document.querySelector('[role=tab]')) window.__ready = performance.now(); else if (!window.__ready) requestAnimationFrame(check); };
requestAnimationFrame(check);`;
const med = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
const browser = await chromium.launch();
const rows = [];
try {
  for (let i = 0; i < +runs; i++) {
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: process.env.MOTION ? 'no-preference' : 'reduce' });
    await ctx.addInitScript(stub + probe);
    const page = await ctx.newPage();
    const cdp = await ctx.newCDPSession(page);
    await cdp.send('Performance.enable');
    const errors = []; page.on('pageerror', (e) => errors.push(e.message));
    await page.goto(`${base}/?${query}`, { waitUntil: 'load' });
    await page.waitForFunction(() => window.__ready, null, { timeout: 30000 });
    await page.waitForTimeout(1500);
    const m = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
    const info = await page.evaluate(() => {
      const nav = performance.getEntriesByType('navigation')[0];
      const res = performance.getEntriesByType('resource');
      return { fcp: Math.round(performance.getEntriesByName('first-contentful-paint')[0]?.startTime ?? -1), dcl: Math.round(nav.domContentLoadedEventEnd), load: Math.round(nav.loadEventEnd), ready: Math.round(window.__ready),
        requests: res.length, kb: Math.round(res.reduce((s, r) => s + (r.transferSize || r.encodedBodySize || 0), 0) / 1024), longTasks: window.__lt.length, longMs: window.__lt.reduce((s, [, d]) => s + d, 0), nodes: document.getElementsByTagName('*').length };
    });
    const a = await cdp.send('Performance.getMetrics'); const g = (r, n) => r.metrics.find((x) => x.name === n).value;
    await page.waitForTimeout(10000);
    const b = await cdp.send('Performance.getMetrics');
    const idle = { cpuPct: +((g(b, 'TaskDuration') - g(a, 'TaskDuration')) / 10 * 100).toFixed(2), scriptPct: +((g(b, 'ScriptDuration') - g(a, 'ScriptDuration')) / 10 * 100).toFixed(2), layouts: g(b, 'LayoutCount') - g(a, 'LayoutCount'), styles: g(b, 'RecalcStyleCount') - g(a, 'RecalcStyleCount') };
    rows.push({ ...info, heapMB: +(m.JSHeapUsedSize / 1048576).toFixed(1), heapTotalMB: +(m.JSHeapTotalSize / 1048576).toFixed(1), loadScriptMs: Math.round(m.ScriptDuration * 1000), loadTaskMs: Math.round(m.TaskDuration * 1000), ...idle, errors: errors.length });
    await ctx.close();
  }
} finally { await browser.close(); }
const keys = Object.keys(rows[0]);
console.log(`== ${label} ${query} (${rows.length} runs; median [min..max])`);
for (const k of keys) { const v = rows.map((r) => r[k]); console.log(k.padEnd(14), String(med(v)).padStart(8), `[${Math.min(...v)}..${Math.max(...v)}]`); }
fs.writeFileSync(`${process.env.PERF_OUT ?? '/tmp'}/${label}.json`, JSON.stringify(rows));
