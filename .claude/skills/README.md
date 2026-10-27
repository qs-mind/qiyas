# Agent skills for building Qiyas

54 skills that every agent working on Qiyas uses (Claude Code loads this folder in any session opened in this repo,
on a laptop or in the cloud). They are third-party, copied at the commits below, read before they were added.
Their own licences sit in each folder; none of this is shipped in the app.

## Which skill when

| Work | Load |
|---|---|
| Any change | `incremental-implementation`, `debugging-and-error-recovery`, `source-driven-development`, `code-simplification` |
| Rust | `rust-skills` (265 rules), `rust-async-patterns` (agent loop, MCP, streams), `memory-safety-patterns` (locks, Arcs, workspaces) |
| Speed and memory | `performance-optimization` first (measure, fix the biggest cost, measure again), then `rust-profiling`, `linux-perf`, `flamegraphs`, `heaptrack`, `cpu-cache-opt`; window: `react-component-performance`, `react-state-management`, `native-feel`, `fixing-motion-performance`, `performance`, `core-web-vitals` |
| Slow builds | `rust-build-times` |
| Units, lengths, areas, quantities | `dimensional-analysis` |
| Tests | `property-based-testing` (proptest / fast-check), `javascript-testing-patterns`, `vitest`, `e2e-testing-patterns`, `browser-testing-with-devtools` |
| Window code | `typescript-advanced-types`, `pnpm`, `api-and-interface-design` (IPC, crate boundaries) |
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

Scripts were left out, except supply-chain-risk-auditor's own (it reads public npm, PyPI and GitHub data).
To update one: clone the source at a newer commit, read the diff, copy it over and change the commit here.
