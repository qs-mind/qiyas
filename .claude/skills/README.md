# Agent skills for building Qiyas

92 skills that every agent working on Qiyas uses (Claude Code loads this folder in any session opened in this repo,
on a laptop or in the cloud). They are third-party, copied at the commits below, read before they were added.
Their own licences sit in each folder; none of this is shipped in the app.

## Which skill when

| Work | Load |
|---|---|
| Any change | `incremental-implementation`, `debugging-and-error-recovery`, `source-driven-development`, `code-simplification`, `verification-before-completion` |
| A plan for Codex | `writing-plans` (tasks with exact files, signatures and tests; hand-off is to Codex), `test-driven-development` (red, green, refactor in each task) |
| A hard bug | `systematic-debugging` (root cause before any fix); `diagnosing-bugs` when it needs a reproduction loop first, e.g. a speed regression or a click-through in the window. They differ on hypotheses (one at a time against 3–5 ranked): pick one per bug |
| Tauri: IPC, capabilities, CSP, plugins, release | `tauri-architecture`, `tauri-rust`, `tauri-frontend`, `tauri-security`, `tauri-plugins`, `tauri-setup`, `tauri-testing`, `tauri-release` (publishing is the founder's); the plugins Qiyas uses: `tauri-app-dialog`, `tauri-app-updater`, `tauri-app-single-instance`, `tauri-app-window-state`, `tauri-app-window-menu` (links to the docs only; their upstream examples were wrong and are left out) |
| Rust | `rust-skills` (265 rules), `rust-async-patterns` (agent loop, MCP, streams), `memory-safety-patterns` (locks, Arcs, workspaces), `rust-best-practices` and `rust-review-idiomatic` (their cargo runs are full builds: scope with `-p`, niced, one at a time; their lint config is not adopted) |
| `unsafe` or FFI (libc, pdfium, ONNX Runtime) | `unsafe-rust-review` |
| Parsers of other people's files (DWG, DXF, CTB/STB, expressions, scripts, command JSON) | `cargo-fuzz` (needs a nightly toolchain: the founder's OK first; keep `fuzz/` outside the workspace), else `property-based-testing` over arbitrary bytes on stable |
| A bug found once, look for its siblings | `variant-analysis` (ripgrep path; CodeQL needs the founder's OK) |
| Do the tests catch bugs? | `mutation-testing` (use `cargo-mutants`, one crate, niced, overnight, in a separate worktree) |
| Speed and memory | `performance-optimization` first (measure, fix the biggest cost, measure again), then `rust-profiling`, `linux-perf`, `flamegraphs`, `heaptrack`, `cpu-cache-opt`; window: `react-component-performance`, `react-state-management`, `native-feel`, `fixing-motion-performance`, `performance`, `core-web-vitals` |
| Slow builds | `rust-build-times` |
| Units, lengths, areas, quantities | `dimensional-analysis` |
| Tests | `property-based-testing` (proptest / fast-check), `javascript-testing-patterns`, `vitest`, `e2e-testing-patterns`, `browser-testing-with-devtools` |
| Window code | `typescript-advanced-types`, `pnpm`, `api-and-interface-design` (IPC, crate boundaries), `react-best-practices` and `composition-patterns` (the server, hydration, script-tag and Next.js rules don't apply; better-all, swr, svgo would be new dependencies: `supply-chain-risk-auditor` first) |
| The viewport's WebGL2 | `webgl2-systems-architect-skill` (WebKitGTK has no timer queries: time by difference; WebGPU is background reading) |
| Crate and module seams, QS vocabulary | `codebase-design`, `domain-modeling` (decisions in dated `docs/` files, Arabic terms in `docs/arabic-glossary.md`) |
| Arabic and English | `arabic-ready`, `dualkit-rtl-layout`, `dualkit-bilingual-copy`, `dualkit-type-pairing`, `dualkit-gulf-formats` (money in minor units; KWD, BHD, OMR take three decimals) |
| Screenshots of the window without your screen | `screenshotting-gui` (Xvfb `:99`; needs `xorg-server-xvfb` and `python-xlib`) |
| Mizan's take-off and BOQ checks | `takeoff-review-checklist`, `unit-rate-sanity-check` (prompt patterns; Mizan still asks instead of assuming) |
| Plans and docs against the code | `egm-drift-report` (writes `docs/YYYY-MM-DD-drift-report.md`), `spec-to-code-compliance` |
| UI and UX | `frontend-design`, `baseline-ui`, `improve-ui`, `create-design-md`, `accessibility`, `fixing-accessibility` |
| Mizan and MCP | `prompt-engineering-patterns`, `llm-evaluation` (no paid runs without the founder), `mcp-builder` |
| Security | `security-and-hardening`, `stride-analysis-patterns`, `sharp-edges`, `best-practices` |
| A new or bumped dependency | `supply-chain-risk-auditor` |
| Review before delivery | `code-review-and-quality`, `spec-to-code-compliance` (code against CLAUDE.md and docs/), `differential-review` |
| Logs and diagnostics | `observability-and-instrumentation` |
| Decisions, docs, quality bar | `documentation-and-adrs`, `constraint-driven-development` |
| Releases | `shipping-and-launch` |
| Quanta, website, market (QS Mind work kept here too) | `postgresql-table-design`, `prefer-container-queries`, `competitive-landscape`, `market-sizing-analysis` |

Limits on this laptop: one build or profiler at a time, niced; anything needing sudo or a system setting
(for example `perf_event_paranoid`) needs the founder's OK.

## Sources

| Skill | Source | Commit | Licence |
|---|---|---|---|
| rust-skills | leonardomso/rust-skills | fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9 | MIT |
| flamegraphs, linux-perf, heaptrack, rust-profiling, rust-build-times, cpu-cache-opt | mohitmishra786/low-level-dev-skills | bdc58472fa9f309ed1b3f7d985a0d8e9bd8f4608 | MIT |
| react-component-performance | Dimillian/Skills | 05ba982bfeb0d77d3c97d4542b0ee15034d05f84 | MIT |
| native-feel | yetone/native-feel-skill | 9bd88c6378e1a80a4da165f3ee7bebb761a74945 | MIT |
| property-based-testing, dimensional-analysis, supply-chain-risk-auditor, sharp-edges, spec-to-code-compliance, differential-review | trailofbits/skills | 196a9073ab10d35bacf111f945e02c00b2c6883a | CC-BY-SA-4.0 (Trail of Bits) |
| vitest, pnpm | JetBrains/skills snapshot of antfu/skills | 3f53782dec7b5424bf6e8efbb90a46ee26f31798 | MIT |
| mcp-builder, frontend-design | anthropics/skills | dbd4588f9e1033efb41dad4bef2f7947c8993d44 | Apache-2.0 |
| performance-optimization, debugging-and-error-recovery, code-review-and-quality, code-simplification, incremental-implementation, security-and-hardening, browser-testing-with-devtools, observability-and-instrumentation, api-and-interface-design, documentation-and-adrs, shipping-and-launch, source-driven-development, constraint-driven-development | addyosmani/agent-skills | aeac41d4d005999432bb7312f7ff06383a91d59d | MIT |
| rust-async-patterns, memory-safety-patterns, typescript-advanced-types, javascript-testing-patterns, react-state-management, llm-evaluation, prompt-engineering-patterns, stride-analysis-patterns, e2e-testing-patterns, postgresql-table-design, competitive-landscape, market-sizing-analysis | wshobson/agents | 0874a27b17a22e2b7c2e2bcd725768e0803cad06 | MIT |
| prefer-container-queries | flornkm/skills | cf1c0094e1826230623ecdb6e7b3c44bdfc593c3 | MIT |
| performance, core-web-vitals, accessibility, best-practices | addyosmani/web-quality-skills | afa8da942115f2961fdbfa80807ea0b232ff6c00 | MIT |
| baseline-ui, fixing-accessibility, fixing-motion-performance, improve-ui, create-design-md | ibelick/ui-skills | main, Oct 2026 | MIT |

| tauri-architecture, tauri-frontend, tauri-plugins, tauri-release, tauri-rust, tauri-security, tauri-setup, tauri-testing | fxckcode/tauri-skills | d3959ffae74a04ae0ae8e4ad577e72b66c5e02d4 | MIT |
| tauri-app-dialog, tauri-app-updater, tauri-app-single-instance, tauri-app-window-state, tauri-app-window-menu (SKILL.md only) | full-stack-skills/tauri-skills | 77b57c4913873a0f8cab7db45116038dc18a27f0 | Apache-2.0 |
| webgl2-systems-architect-skill | Emily2040/webgl2-systems-architect-skill | 9a37ad9a145838eac62be7176aa884367b43c639 | MIT |
| screenshotting-gui (with its three scripts) | EvilFreelancer/screenshotting-gui | 6b8527d7912997c7e383dafa9a725fcd0a40d579 | MIT |
| unsafe-rust-review | google/rust-skills (`unsafe_rust_review`) | 34b87c594bf7386290a1e3698d5b8e06b6b9edd7 | Apache-2.0 |
| rust-best-practices, rust-review-idiomatic (upstream `rust-review`) | morganlinton/BeautifulRust | a9cdcfa94d1330c0d41c7f67764c5f7786fbb71d | MIT |
| cargo-fuzz, mutation-testing, variant-analysis | trailofbits/skills | 2be6433b39e009cd2b6fd3be1895f6b1faa5af9a | CC-BY-SA-4.0 (Trail of Bits) |
| react-best-practices, composition-patterns | vercel-labs/agent-skills | 1a2c79d4e6c4a2283dc2c614b6538cbe1ed5ca62 | MIT (stated in each SKILL.md) |
| test-driven-development, systematic-debugging, verification-before-completion, writing-plans | obra/superpowers | 2ce8aa15d05bd57213e6f746f953aafb7ded7a8b | MIT |
| codebase-design, domain-modeling, diagnosing-bugs | mattpocock/skills | 4ae2317ddb22b046e26455e4f9c9b3aaaf62212d | MIT |
| egm-drift-report | google/elephant-goldfish-model | 403d8881d62022490637d740657b33f01e321ea8 | Apache-2.0 |
| dualkit-rtl-layout, dualkit-bilingual-copy, dualkit-type-pairing, dualkit-gulf-formats | dualkit/dualkit | 6a0e098fb85f4202b05a7d4e57c3cde38830a7ec | MIT |
| arabic-ready | jubyuhanna-create/arabic-ready | 3e1ebb5e869116c0159205fe2034b587244b780f | MIT |
| takeoff-review-checklist, unit-rate-sanity-check (unmodified) | constructelligence-lab/construction-agent-skills | f05ffdf70b650582a4a12181d6fedcc9b3ae56f1 | CC-BY-4.0 (Constructelligence, https://constructelligence.co, https://creativecommons.org/licenses/by/4.0/) |

### Where CLAUDE.md wins

The skills are general; Qiyas's own rules come first.

- Arabic font: Thmanyah Sans with `"swsh"` headings, local and sealed. The skills' IBM Plex Sans Arabic, Readex Pro, Noto and Google Fonts advice does not apply to the window.
- Arabic size: the skills' 14 px floor and 1.6–1.7 line height are not adopted; the window's sizes stay as they are unless the founder rules otherwise.
- Arabic buttons: verbal nouns (حفظ، إلغاء), as `dualkit-bilingual-copy` and `ar.json` do, not the imperatives in `arabic-ready`.
- Mizan never assumes: the construction checklists ask the engineer, as CLAUDE.md requires.

### Local edits to the copies

Made so the skills fit this repo; everything else is as upstream.

- npm and npx became pnpm (`test-driven-development`, `systematic-debugging`, `tauri-setup`, `react-best-practices`).
- Links to skills that are not here were removed (`superpowers:` prefixes, using-git-worktrees, writing-skills, EGM plugin docs).
- `writing-plans`: the hand-off goes to Codex on the founder's computer, and each commit follows `scripts/check.sh`, never a push.
- `domain-modeling`, `diagnosing-bugs`, `codebase-design`: no `GLOSSARY.md` or `docs/adr/` without asking; guard tests are never deleted; one build at a time.
- `screenshotting-gui`: no `git checkout .`/`git clean`, no `pkill` (kill by PID), logs in a scratch folder, `-nolisten tcp`, and the Wayland note for Hyprland.
- `arabic-ready`: its script steps replaced with the checklist and the app.
- `egm-drift-report`: writes `docs/YYYY-MM-DD-drift-report.md`.
- Renamed: BeautifulRust's `rust-review` to `rust-review-idiomatic`; Vercel's `vercel-react-best-practices` and `vercel-composition-patterns` to their folder names.

Looked at and left out (2 November 2026): Trail of Bits' `rust-review` and EGM's other six skills (plugins: as bare folders they break), `cad-drawings` and the PDF-to-priced-BOQ skill (manuals for their own Python scripts; the BOQ one runs "without stopping to ask"), Takeoff-Lens (calls MCP tools Qiyas doesn't have), uiverify (sends captures to its service), `tauri-mobile` (desktop only).

Scripts were left out, except supply-chain-risk-auditor's own (it reads public npm, PyPI and GitHub data), screenshotting-gui's three (X11 input, window list and `import` screenshots on the display it is given; no network) and diagnosing-bugs' `hitl-loop.template.sh` (prompts a person, nothing else).
To update one: clone the source at a newer commit, read the diff, copy it over and change the commit here.
