// Which components render on a pointer move (React dev build, DevTools hook stand-in)
import { createRequire } from 'module'; import fs from 'fs';
// Playwright is not a dependency of the app: PLAYWRIGHT_FROM is the package.json of any folder that has it installed.
const require = createRequire(process.env.PLAYWRIGHT_FROM ?? new URL('../../package.json', import.meta.url).pathname); const { chromium } = require('playwright');
const stub = fs.readFileSync(new URL('stub.js', import.meta.url), 'utf8') + fs.readFileSync(new URL('bigdata.js', import.meta.url), 'utf8');
const [base, query = 'state=chat', moves = '60'] = process.argv.slice(2);
const hook = `(() => { const stats = new Map(); let commits = 0; window.__rstats = stats; window.__commits = () => commits; window.__on = false; window.__diff = new Map(); const WATCH = new Set(['AssistantPanel','Ribbon','RibbonBody','DataGrid','ArtifactFrame','SidePanel','Panels','Header','StatusBar','FileTabs','SheetTabs','CommandLine','LayersPanel','Viewport','PropertiesPanel','OutputsMenu','WorkspacePicker','CanvasMenu','QuestionList','ToolCard']);
  const name = (f) => { const t = f.type; return typeof t === 'function' ? (t.displayName || t.name || 'anon') : t && t.type ? (t.type.displayName || t.type.name || 'memo') : t && t.render ? (t.render.displayName || t.render.name || 'fwd') : null; };
  const walk = (f) => { while (f) { const nm = name(f); if (nm && f.alternate && f.flags & 1 /*PerformedWork*/ ) { let kids = 0; for (let c = f.child; c; c = c.sibling) kids += c.actualDuration || 0; const self = Math.max(0, (f.actualDuration || 0) - kids); const s = stats.get(nm) || [0, 0]; s[0]++; s[1] += self; stats.set(nm, s); if (WATCH.has(nm) && f.alternate.memoizedProps && f.memoizedProps !== f.alternate.memoizedProps) { const a = f.memoizedProps || {}, b2 = f.alternate.memoizedProps || {}; for (const k of new Set([...Object.keys(a), ...Object.keys(b2)])) if (a[k] !== b2[k]) { const key = nm + '.' + k; window.__diff.set(key, (window.__diff.get(key) || 0) + 1); } } } if (f.child && (f.subtreeFlags & 1 || f.flags & 1 || !f.alternate)) walk(f.child); f = f.sibling; } };
  window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = { supportsFiber: true, renderers: new Map(), inject() { return 1; }, onScheduleFiberRoot() {}, onCommitFiberUnmount() {}, checkDCE() {}, onCommitFiberRoot(id, root) { if (!window.__on) return; commits++; walk(root.current); } }; })();`;
const b = await chromium.launch(); const c = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' }); await c.addInitScript(hook + stub + fs.readFileSync(new URL('bigdata.js', import.meta.url), 'utf8'));
const p = await c.newPage(); await p.goto(base + '/?' + query); await p.waitForSelector('canvas'); await p.waitForTimeout(1500);
const bb = await p.locator('canvas').first().boundingBox();
if (process.env.TOOL) await p.getByRole('button', { name: process.env.TOOL, exact: true }).click();
await p.evaluate(() => { window.__on = true; window.__rstats.clear(); });
const N = +moves; for (let i = 0; i < N; i++) { await p.mouse.move(bb.x + 100 + (i * 7) % 600, bb.y + 100 + (i * 5) % 400); await p.waitForTimeout(20); }
const diffs = await p.evaluate(() => [...window.__diff]);
const r = await p.evaluate(() => ({ commits: window.__commits(), rows: [...window.__rstats].map(([k, v]) => [k, v[0], v[1]]) }));
console.log('commits', r.commits, 'for', N, 'moves');
console.log('-- props that changed (name.prop, times)'); for (const [k, n] of diffs.sort((a, b) => b[1] - a[1])) console.log(String(n).padStart(5), k);
r.rows.sort((a, b) => b[2] - a[2]);
for (const [k, n, t] of r.rows.slice(0, 25)) console.log(String(n).padStart(5), 'renders', t.toFixed(1).padStart(8), 'ms self', k);
await b.close();
