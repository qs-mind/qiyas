# Phase 9: shared command words

The registry now owns command words and aliases beside the Rust command variants. The
window retains its existing click tools and parser; words it does not recognize use a
pure schema-driven machine shared with scripts and the AI. New words also appear in
command suggestions and ribbon customization without adding a window dispatch case.

The machine supports ordered prompts, quoted names, numbers, options, point sequences,
Close/Undo, Arabic digit sets and separators, relative and polar points, and the UCS.
Prompt metadata also distinguishes words with different fixed options, such as LAYON
and LAYOFF. Failed input preserves the pending arguments and last point.

Scripts retain their existing grammar first, whole-file validation, line numbers and
path rules. AI typed commands use the existing execution path for folder restrictions,
source attribution, undo, drawing checks and reveal results. Desktop parsing returns
a command to the window's normal apply path. Its pending prompts are cancelled when
the user changes tools or drawings; late replies cannot change a different drawing.

## Deliberate behavior and limits

- The existing window parser remains authoritative for its words and click tools.
  Its original parser and command-line test files are unchanged.
- `MEASURE` keeps its legacy script meaning (an entity handle). The registry word
  describes the window's measure-along command; `ME` reaches that command in scripts.
- Script coordinates remain world coordinates, as before: parsing happens before
  execution. The machine receives the current UCS in desktop and AI calls.
- AI `type` takes a complete line on each call. Incomplete input returns a prompt and
  runs nothing; the caller resubmits the complete line. Relative points refer to
  earlier points in that line.
- Required nested object arguments still use JSON commands or the window's picking
  tools. The typed machine covers points, numbers, names and options.
- The shipped ribbon is unchanged. Its customization catalog gains registry commands,
  initially hidden, with required arguments asked on the command line.
- No dependencies, commits, pushes, release builds, browser or desktop windows.

## Checks

All heavy checks ran serially. Cargo used `CARGO_BUILD_JOBS=1` and `nice -n 19`;
Vitest used `--maxWorkers=1`. Passing suites were not repeated.

| Check | Result |
|---|---|
| `cargo fmt --all` | Passed. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed. |
| `cargo test --offline -p qiyas-core` | Passed: 584 tests across 50 targets, including doc tests. |
| Integer precision regression | Passed: 1 new test, 9 earlier tests filtered out. Checks exact large integer handles without `f64` rounding. |
| `cargo test --offline -p qiyas-ai --lib --test surface` | 50 library tests passed; 1 existing local-port test failed with sandbox `PermissionDenied`. Cargo stopped before `surface`. |
| `cargo test --offline -p qiyas-ai --test surface` | 18 passed; the new path-refusal assertion expected the wrong wording. Corrected it to the actual existing refusal text; that test alone passed on retry (18 filtered out). |
| `cargo test --offline -p qiyas-desktop -p qiyas-cli` | CLI: 17 passed. Desktop library: 73 passed, 1 existing MCP port test blocked by the sandbox. Cargo stopped before its integration targets. |
| `cargo test --offline -p qiyas-desktop --test options --test settings` | Passed: 10 options and 11 settings tests, including registry ribbon customization. |
| `node_modules/.bin/tsc --noEmit` | Passed. |
| `node_modules/.bin/vitest run --maxWorkers=1` | 797 passed, 2 failed initially. Both failing files passed on their one retry: 37 tests. |
| Unknown-command localization regression | Passed: 1 new test, 5 earlier tests filtered out. |

The initial Vitest failures were the existing PURGE test's missing toast before its
short wait expired, and the new cancellation regression whose final correction was
made during that run. `App.test.tsx` and `CommandLine.registry.test.tsx` were the
only files retried; both passed without changing any existing test.

The precision correction followed the full core/Clippy runs; the new regression ran
alone, and the later AI and desktop builds compile the corrected core. The final
unknown-command localization correction followed the TypeScript/full Vitest runs and
has its own focused regression. The changed Rust files were formatted locally.

The blocked library test is
`models::tests::an_unreachable_service_falls_back_to_the_builtin_list`:
`models.rs:247` cannot bind its local port (`Operation not permitted`). Desktop's
`commands::tests::the_mcp_server_follows_the_settings` fails for the same reason at
`commands.rs:1534`. Neither test was retried or changed. The unrestricted AI
integration suite that opens local ports is also left for the reviewer, as requested.
No live GUI check was performed.

## Files touched

Core metadata, parser and guards:

- `crates/qiyas-core/src/commands.rs`
- `crates/qiyas-core/src/dimension.rs`
- `crates/qiyas-core/src/draw.rs`
- `crates/qiyas-core/src/modify.rs`
- `crates/qiyas-core/src/modify2.rs`
- `crates/qiyas-core/src/registry.rs`
- `crates/qiyas-core/src/lib.rs`
- `crates/qiyas-core/src/cmdline/mod.rs`
- `crates/qiyas-core/src/cmdline/input.rs`
- `crates/qiyas-core/src/script.rs`
- `crates/qiyas-core/tests/cmdline.rs`
- `crates/qiyas-core/tests/registry.rs`
- `crates/qiyas-core/tests/script.rs`

AI and desktop:

- `crates/qiyas-ai/src/surface.rs`
- `crates/qiyas-ai/tests/surface.rs`
- `crates/qiyas-desktop/src/cmdline.rs`
- `crates/qiyas-desktop/src/lib.rs`
- `crates/qiyas-desktop/src/ribbon.rs`
- `crates/qiyas-desktop/tests/options.rs`

Window:

- `src/api.ts`
- `src/app/App.tsx`
- `src/app/Header.tsx`
- `src/app/useCommandLine.ts`
- `src/app/useCommandLine.registry.test.tsx`
- `src/app/useCommands.ts`
- `src/app/useShell.ts`
- `src/shell/AliasEditor.tsx`
- `src/shell/CommandLine.tsx`
- `src/shell/CommandLine.registry.test.tsx`
- `src/shell/Ribbon.tsx`
- `src/shell/Ribbon.registry.test.tsx`
- `src/tools/commandParser.ts`
- `src/tools/commandWords.ts`
- `src/tools/commandWords.test.ts`

Documentation:

- `CLAUDE.md`
- `CHANGELOG.md`
- `docs/2026-10-20-qiyas-phase9-check.md`
