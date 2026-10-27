import { createRequire } from 'module'; import fs from 'fs';
// Playwright is not a dependency of the app: PLAYWRIGHT_FROM is the package.json of any folder that has it installed.
const require = createRequire(process.env.PLAYWRIGHT_FROM ?? new URL('../../package.json', import.meta.url).pathname); const { chromium } = require('playwright');
const stub = fs.readFileSync(new URL('stub.js', import.meta.url), 'utf8') + fs.readFileSync(new URL('bigdata.js', import.meta.url), 'utf8');
const [base, query = 'state=chat', tool = ''] = process.argv.slice(2);
const b = await chromium.launch(); const c = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' }); await c.addInitScript(stub);
const p = await c.newPage(); const cdp = await c.newCDPSession(p); await cdp.send('Performance.enable');
await p.goto(base + '/?' + query); await p.waitForSelector('canvas'); await p.waitForTimeout(1500);
if (tool) await p.getByRole('button', { name: tool, exact: true }).click();
const bb = await p.locator('canvas').first().boundingBox();
const get = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
await p.evaluate(() => { window.__calls = 0; });
const a = await get(); const N = 400;
for (let i = 0; i < N; i++) { await p.mouse.move(bb.x + 100 + (i * 3) % 600, bb.y + 100 + (i * 2) % 400); }
await p.waitForTimeout(300);
const z = await get(); const d = (k) => ((z[k] - a[k]) * 1000 / N).toFixed(2);
console.log(`${tool || 'default tool'}: per pointer move: script ${d('ScriptDuration')} ms, task ${d('TaskDuration')} ms, style ${d('RecalcStyleDuration')} ms, layout ${d('LayoutDuration')} ms (layouts ${z.LayoutCount - a.LayoutCount}, styles ${z.RecalcStyleCount - a.RecalcStyleCount})`);
await b.close();
