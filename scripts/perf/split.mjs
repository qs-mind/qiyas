// usage: node split.mjs <baseUrl> <query> : where a layer click's time goes (script, style, layout, paint)
import { createRequire } from 'module'; import fs from 'fs';
// Playwright is not a dependency of the app: PLAYWRIGHT_FROM is the package.json of any folder that has it installed.
const require = createRequire(process.env.PLAYWRIGHT_FROM ?? new URL('../../package.json', import.meta.url).pathname); const { chromium } = require('playwright');
const stub = fs.readFileSync(new URL('stub.js', import.meta.url), 'utf8') + fs.readFileSync(new URL('bigdata.js', import.meta.url), 'utf8');
const b = await chromium.launch(); const c = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' }); await c.addInitScript(stub);
const p = await c.newPage(); const cdp = await c.newCDPSession(p); await cdp.send('Performance.enable');
await p.goto(process.argv[2] + '/?' + process.argv[3]); await p.waitForSelector('[role=tab]'); await p.waitForTimeout(1500);
const get = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
for (const what of ['Visible: A-03-LAYER-3', 'Isolate: A-05-LAYER-5', 'Visible: A-03-LAYER-3']) {
  const a = await get(); const t = Date.now();
  await p.getByRole('button', { name: what, exact: true }).click({ force: true }); await p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))); await p.waitForTimeout(200);
  const z = await get(); const d = (k) => Math.round((z[k] - a[k]) * 1000);
  console.log(what.padEnd(24), `script ${d('ScriptDuration')} ms, style ${d('RecalcStyleDuration')} ms, layout ${d('LayoutDuration')} ms, task ${d('TaskDuration')} ms; layouts ${z.LayoutCount - a.LayoutCount}, styles ${z.RecalcStyleCount - a.RecalcStyleCount}, nodes ${z.Nodes}`);
}
await b.close();
