# Subscription agent evaluation — 22 October 2026

Qiyas can be the tool host while a subscription agent supplies the reasoning. The harness
makes no model API calls, reads no API credentials or Qiyas settings, and uses no budget
ledger. The reviewer, not the implementation agent, runs live evaluations.

From the repository root, after building `qiyas-cli` and preparing the development cases:

```sh
nice -n 19 ./target/debug/qiyas-cli eval-agent --set live-dev --agent codex --model gpt-6.1-sol --effort medium --timeout-min 20
```

`--case ID` selects one case; `--cases a,b` selects several. `--root` defaults to
`.local/benchmark`. The default Codex model is `gpt-6.1-sol`, effort `medium`; Astra is
never selected implicitly. Each case has a fresh run folder, initially containing only
its drawing copy. Truth, recognition recipes, project sidecars and visible-input files
are not copied. The outer harness reads case metadata/truth and scores after the client
stops; the server and ordinary agent workflow never receive truth.

## Headless MCP

```sh
./target/debug/qiyas-cli mcp --drawing /absolute/plan.dxf --workdir /absolute/run
```

This uses rmcp's stdio transport and the production MCP handler, with `surface::route`,
`surface::tools`, source `mcp` and the same undo rules. stdout is reserved for MCP. There
are exactly three tools: `qiyas_read`, `qiyas_execute`, `qiyas_capture`. The initialize
instructions equal `registry().index_text()` byte for byte, the same generated workflow
index included in Mizan's prompt. Domain guidance is not duplicated in the CLI.

Read digest, read takeoff_kit, adapt one batch to drawing evidence, check the generated
BOQ, then call `qiyas_execute` with `op: "finish"` and `args: {"answer":"..."}`.
`finish` saves without adding an undo step. A command or batch still creates one undo
step and `undo_own` targets `mcp`. A client can continue editing after a checkpoint;
stdin EOF writes the latest state again and retains the supplied answer.

The server opens a standalone Session, ignores adjacent project sidecars, rejects linked
external drawing resources, and uses eval's PDF page-1 preparation when needed. PDF units
remain uncalibrated paper size until the agent establishes a visible scale. Missing PDFium
is a real preparation failure. Headless pictures work; screen capture and selection answer
`needs_window`. The single-drawing host supports listing/saving/finishing the drawing;
opening another drawing, creating a tab or running a project needs a separate host.

File paths are confined to the workdir with the existing drawing-folder restrictions and
eval's symlink/holdout checks. Explicit truth, recipes and settings paths are refused.
If the input drawing is outside the workdir, only that drawing is copied in. If `--workdir`
is omitted, the drawing's directory is used and the drawing is saved there.

Finish or stdin EOF writes:

- The drawing and its Qiyas project sidecar.
- `takeoff.json`, generated from accepted recognition and BOQ mappings, using eval's writer.
- `measurement.json`, `takeoff.xlsx` with generated BOQ/dimensions, `conflicts.json`.
- `self-check.json`, `end-report.json` with answer, preparation, quantities and limitations.
- `mcp-events.jsonl` with tool calls, including rejected file attempts; `mcp-exit.json`
  acknowledges completion of the EOF save before scoring.

The server does not score. The harness retains `agent-events.jsonl`, `agent-stderr.log`,
and `report.json`. Each case has eval's `run`, `case`, `status`, `elapsed_ms`, `drawing`,
`takeoff`, `score` and log fields, plus `agent`, `model`, `effort`, `tool_calls` and
`cost: "subscription"`. Combined reports merge the existing scorer's metrics and rates;
contaminated cases remain listed but contribute nothing to the aggregate denominator.
A failed/empty clean case contributes a zero-submission score. Empty completion is a failure.

## Codex invocation and other clients

The flags were checked against this machine's `codex exec --help` and `codex mcp --help`.
The runner constructs arguments directly, without shell interpolation:

```sh
codex exec --json --skip-git-repo-check --sandbox read-only --ignore-user-config --ignore-rules \
  -C /absolute/run \
  -c 'mcp_servers.qiyas.command="/absolute/qiyas-cli"' \
  -c 'mcp_servers.qiyas.args=["mcp","--drawing","/absolute/run/drawing.dxf","--workdir","/absolute/run"]' \
  -c 'mcp_servers.qiyas.default_tools_approval_mode="approve"' \
  -c 'forced_login_method="chatgpt"' -c project_doc_max_bytes=0 \
  -m gpt-6.1-sol -c 'model_reasoning_effort="medium"' '<plain take-off request>'
```

Ancestor project instruction discovery and user exec rules are disabled for this run.
Subscription login is forced; common API credential environment variables are removed
without reading their values. Missing subscription authentication must fail rather than
fall back to a paid API. The request comes from the same function as Mizan eval, with
instructions to use MCP, finish and stay within the run. Read-only sandboxing applies to
agent shell actions; the explicitly configured MCP server writes its own output files.

`--agent-cmd` is a POSIX shell template. Supported placeholders are `{cli}`, `{drawing}`,
`{workdir}`, `{request}`, `{model}`, `{effort}`, `{mcp_config}`. Each placeholder becomes
one shell-quoted argument; do not surround it with extra quotes. Substitution occurs once,
so quotes, dollar signs and backticks inside values are literal. Unknown placeholders fail.
The JSON MCP configuration is generated in the separate combined-report directory, not
copied into the case workspace. For example, with an existing Claude subscription login:

```sh
./target/debug/qiyas-cli eval-agent --case office-15000 --agent claude \
  --agent-cmd 'claude -p {request} --mcp-config {mcp_config} --output-format stream-json --verbose'
```

Custom clients must be configured for subscription use and must allow the three Qiyas
tools non-interactively. Their own authentication/plugin policies remain their responsibility;
the runner cannot prove that an arbitrary user-supplied shell command never bills a service.
No Claude invocation was tested against a real client.

## Contamination and limits

The audit scans logged shell commands, file-read arguments and MCP inputs for truth,
recipes, benchmark-store paths and repository access outside the exact run subtree.
It ignores ordinary assistant prose and tool results. Attempts count even when a read
fails. A contaminated case has `status: "contaminated"`, a null score and retained audit
reasons. This is an evidence guard, not an adversarial filesystem sandbox: implicit or
unlogged reads and deliberately concealed paths cannot be established from incomplete
client logs. Clients must emit their command/file actions. The original drawings remain
outside the agent workspace and the scorer verifies traceability against them.

The timeout terminates the process group on Unix, including child MCP processes. Normal
exit waits briefly for the EOF save acknowledgement, then stops remaining descendants before
scoring. A prior
finish checkpoint remains scoreable; a hard-killed process cannot perform an EOF save.
Without a submission, the case scores zero. Live recognition quality, subscription limits
and sandbox behavior are for reviewer validation. Offline tests prove protocol and scoring
wiring only. No real Codex or Claude evaluation was run.

## Offline verification

Tests generate a measured rectangle independently of the scripted client's decisions.
The in-process rmcp client launches the actual stdio server, checks instruction equality,
uses digest → kit → one adapted batch → finish, tests MCP-source undo, captures a picture,
and verifies file/window refusals. A second run closes stdin without finish. Both retain a
non-empty BOQ scored by the existing scorer. A Python fake agent drives the same protocol
through `eval-agent`; its deliberate negative variant logs and reads a test truth file and
is excluded from the combined score. No real provider is involved.

All Cargo commands ran serially with `CARGO_BUILD_JOBS=4 nice -n 19`, with no helper
agents or background builds. No new crates were introduced: the CLI now directly depends
on the existing Apache-2.0 rmcp dependency.

| Check | Actual result |
|---|---|
| `cargo fmt --all` | Passed once. Later CLI hardening edits were formatted with rustfmt; final rustfmt and whitespace checks passed. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed once. Subsequent CLI hardening edits compiled and passed the CLI tests below; Clippy was not repeated. |
| `cargo test --offline -p qiyas-ai --lib --test surface` | Library: 73 passed, 1 failed because localhost binding is denied (`models::tests::an_unreachable_service_falls_back_to_the_builtin_list`). Cargo stopped before surface. |
| Completion of the unrun `qiyas-ai --test surface` target | All 49 passed. |
| `cargo test --offline -p qiyas-cli -p qiyas-mcp` | CLI: all 42 unit and 23 integration tests passed, including the 3 new agent/MCP tests. MCP bridge: stdin-close test passed; 3 tests failed on denied localhost binds. |
| Desktop tests | Not run: no desktop source changed. |
| CLI help and `git diff --check` | Passed. |

The reviewer must run the AI localhost test and these bridge tests outside this sandbox:
`a_dead_url_answers_not_running`, `lists_and_calls_through_the_bridge`, `a_wrong_token_says_so`.
No tests were removed, weakened or ignored. No real subscription-agent evaluation was run,
and there were no paid API calls, settings/key access, git staging, commits or pushes.

## Third-run metadata and scan corrections

The common submission writer retains each measured element’s drawing-coordinate centroid or
insertion point and its own label text, falling back to the measurement rule text. Explicit
owned labels and name properties take precedence; a single enclosed text is usable, while
multiple enclosed texts remain ambiguous. Labels are not expanded with synonyms or rewritten
to benchmark names. PDF geometry has already passed through its calibration and insertion
transform; exports do not rescale it again. PDF source citations identify pages, not original
CAD handles. Grouped totals retain their BOQ description and an unweighted source centre.

Saved-run replay reads the same metadata directly from the saved drawing, without resolving
underlays or opening sidecars. It preserves the measured quantities and dimension lines.
Matching remains the reviewer’s tiered scorer; quantities never select a correspondence.
A calibrated page origin is not necessarily the original CAD origin, so recording positions
does not guarantee a match against older truth positions.

`./target/debug/qiyas-cli bench regenerate-scans` refreshes only existing generator scans from
their saved seeds. A margin of 4% of the longest drawing dimension protects boundary strokes;
the longest raster side is 1000 pixels. Prepared vision recipe points and calibration anchors
are transformed from the old page frame to the new one. Clean/vector cases, truth quantities,
seeds and saved agent workspaces are preserved. The command never starts agents.
