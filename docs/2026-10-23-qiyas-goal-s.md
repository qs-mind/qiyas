# Goal S: subscription providers

Worktree: `.local/wt/goal-s`, branch `goal/s`. No commit or push was made.

**Goal S is not ready for production.** The Claude Code path is implemented for Unix and
Work autonomously mode. ChatGPT security and Responses components are implemented, but
production sign-in is deliberately disabled until the reviewer supplies the exact public
SIWC wire specification. The founder's supplied live-policy verification is accepted;
no network fetch was attempted after the instruction not to fetch again.

## 1. Changes by requested item

| Item | Before | After |
| --- | --- | --- |
| ChatGPT plan provider | No OAuth or Responses adapter | PKCE/state/nonce, bounded loopback callback, RSA ID-token validation, explicit plan scope, keychain-only tokens, opaque stable host ID, refresh and streamed Responses components with synthetic tests. Real sign-in is disabled; exact registration and endpoint wiring and official button guidance remain incomplete. |
| Claude subscription provider | No native subscription provider | Uses only the selected installed executable or PATH. Runtime help checks, strict private MCP, exactly three allowed tools, disabled built-in tools/settings/hooks, sanitized environment, dedicated empty working folder, session resume, plain activity/errors, and cancellation signals. No SDK, adapter, bundled executable, credentials or login automation. |
| Individual use | No unconditional Claude evaluation prohibition | One inference process per user message, one active process across conversations, no Qiyas retries or polling, and unconditional case-insensitive `eval-agent --agent claude` refusal, including custom commands. |
| API-key providers | Scoped keys and provider errors from Goal E | Existing API transports and defaults retained. New subscription kinds are explicitly excluded from API-key lookup and model discovery. |
| Settings and language | Two providers | Four named providers with one-sentence English/Arabic descriptions. Claude has terminal login instructions, path selection, explicit status check and official installation link. ChatGPT has account/sign-in/sign-out/plan-limit controls; this build explains that sign-in is unavailable. Existing Arabic font rules remain in place. |
| Documentation and release gates | No subscription implementation guide | Subscription permission/release document, this report, CLAUDE.md behavior updates and license notes. OpenAI paid/hosted interest form and Anthropic Commercial Terms remain release gates. |
| Credentials and safety tests | API-key tests only | Fake CLI records arguments/environment/cwd; Linux sentinel watches credential-file opens; mock keychain and synthetic OIDC keys; scope rejection, token redaction, stable host ID, CLI cancellation, resume and native-turn retry/approval guards. HTTP integration tests retain their socket requirements. |

The Claude native loop cannot currently enforce Qiyas's per-job/per-phase approval pauses.
Those modes are refused before any process starts; they are never silently bypassed.
Non-Unix platforms also refuse the provider because safe cancellation is not implemented
there. The private MCP attributes changes to Mizan so Undo works and preserves the original
user text for file-access authority. A cancelled request is not merged into the next native
CLI prompt.

## 2. Files touched

- `CLAUDE.md`
- `Cargo.lock`
- `LICENSES/README.md`
- `crates/qiyas-ai/Cargo.toml`
- `crates/qiyas-ai/src/agent.rs`
- `crates/qiyas-ai/src/chatgpt_plan.rs`
- `crates/qiyas-ai/src/chatgpt_plan/oauth.rs`
- `crates/qiyas-ai/src/chatgpt_plan/responses.rs`
- `crates/qiyas-ai/src/claude_code.rs`
- `crates/qiyas-ai/src/lib.rs`
- `crates/qiyas-ai/src/mcp.rs`
- `crates/qiyas-ai/src/provider.rs`
- `crates/qiyas-ai/src/provider_error.rs`
- `crates/qiyas-ai/tests/fixtures/README.md`
- `crates/qiyas-ai/tests/fixtures/claude.py`
- `crates/qiyas-ai/tests/fixtures/fake-oidc-jwks.json`
- `crates/qiyas-ai/tests/fixtures/fake-oidc-private.der`
- `crates/qiyas-cli/src/eval_agent.rs`
- `crates/qiyas-desktop/src/ai.rs`
- `crates/qiyas-desktop/src/commands.rs`
- `crates/qiyas-desktop/src/lib.rs`
- `crates/qiyas-desktop/src/secrets.rs`
- `crates/qiyas-desktop/src/settings.rs`
- `crates/qiyas-desktop/src/subscriptions.rs`
- `docs/2026-10-23-qiyas-goal-s.md`
- `docs/2026-10-23-qiyas-subscriptions.md`
- `src/ai/AssistantPanel.tsx`
- `src/ai/messages.test.ts`
- `src/ai/messages.ts`
- `src/api.ts`
- `src/i18n/ar.json`
- `src/i18n/en.json`
- `src/i18n/errors.ts`
- `src/shell/SettingsDialog.test.tsx`
- `src/shell/SettingsDialog.tsx`
- `src/shell/SubscriptionSettings.test.tsx`
- `src/shell/SubscriptionSettings.tsx`

Worktree-local evidence helpers: `.local/mcp_client.py`, `.local/inspect_audit_xlsx.py`,
`.local/subscription_drawing_jobs.py`, `.local/subscription_sample_boq.py`, and
`.local/subscription_cli_refusal.py`. Their inputs, transcripts, PNGs and retained binary
are under `.local/goal-run/`; none was written to another checkout.

The `libc`, `ring` and `url` versions were already in the lockfile and third-party inventory.
The inventory was regenerated with no entry changes; LICENSES/README.md records their direct
use and the synthetic fixtures. No Claude SDK, adapter or binary is bundled.

## 3. Checks and actual results

All Cargo commands used the shared target directory, four build jobs, disabled debug
information and disabled incremental compilation. No real provider, installed Claude
executable, account credential, login or paid call was used.

| Check | Result | Evidence under `.local/goal-run/` |
| --- | --- | --- |
| `cargo fmt --all` | Passed; later Rust edits were formatted directly with rustfmt | `subscription-fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed; final AI follow-up recorded below | `subscription-clippy.log` |
| `cargo test --offline -p qiyas-ai -p qiyas-desktop -p qiyas-cli --no-fail-fast` | 430 passed, 46 failed, one existing private-fixture test ignored. Every failure reported a denied local socket | `subscription-rust-tests.log`, `subscription-rust-summary.json` |
| Required `qiyas-ai --lib --test surface --test agent --no-fail-fast` after the native approval/retry fixes | 160 passed, 35 failed, all at denied local socket binds. Library: 100/5; surface: 57/0; agent: 3/30 | `subscription-ai-required.log` |
| `tsc --noEmit` | Passed | `subscription-tsc-final.log` |
| Full `vitest run --maxWorkers=2` | 118 files, 872 tests passed | `subscription-vitest-final.log` |
| Translation/account tests after the approval-mode message was added | Three files, nine tests passed | `subscription-translations-final.log` |
| License inventory | Regenerated successfully; entries unchanged | `subscription-licenses.log` |
| Production credential-path scan and fake child environment | No credential paths matched, no provider credential variables passed, sentinel never opened | `subscription-fakes/credential-scan.json` |

The complete socket-failure list is in `subscription-port-failures.md`. It includes the
new mock OAuth callback, OAuth token/JWKS, Responses and private Claude MCP tests; the
existing model-discovery, agent HTTP, key-safety and MCP tests; and two existing desktop
socket tests. Tests were not skipped, weakened or relabelled as passes.

Earlier development checks caught an unresolved import while Cargo waited on an older
manifest and a source-scan false positive on the official `code.claude.com` installation
link. Both were corrected. The scan still rejects credential paths; it excludes only the
official URL and translation-key prefix. A later review found that HTTP retry diagnostics
needed to scrub refresh and ID tokens as well as the access token; that is fixed and
covered by a passing regression test.

Additional final checks:

- CLI build passed: `subscription-cli-build.log`. The retained binary is
  `.local/goal-run/qiyas-cli`; its SHA-256 is in `subscription-cli-sha256.txt`.
- Final AI lint after the retry-redaction, approval and callback fixes passed:
  `subscription-clippy-ai-final.log`.
- Final OAuth callback regression passed (one test): `subscription-callback-final.log`.
  Mixed success/error callbacks and token-bearing callbacks are rejected without exposing secrets.
- `git diff --check` passed.

## 4. End-to-end evidence

All paths below are inside this worktree. The CLI snapshot was built from this worktree;
the stdlib client invokes `qiyas-cli mcp --drawing ... --workdir ...` over stdin/stdout.
The later OAuth-only callback hardening is checked separately; the retained CLI snapshot
covers the headless and evaluation-refusal paths exercised here.

### Drawing and Arabic, four units

`.local/subscription_drawing_jobs.py` passed in **mm, cm, m and inches**. Each starts with
one seed line, creates a closed 4 m × 3 m outline, adds a 12 m² takeoff and English/Arabic
labels, reads back after every mutation, saves, reads the takeoff sidecar and reopens the
saved geometry. The original geometry and both exact label strings survive. A request
for the unavailable window returns `needs_window` and makes no drawing change.

For each unit, use:

- `.local/goal-run/subscription-drawing-v3-<unit>/transcript.jsonl`
- `.local/goal-run/subscription-drawing-v3-<unit>/reopened/transcript.jsonl`
- `.local/goal-run/subscription-drawing-v3-<unit>/0004-before-0.png`
- `.local/goal-run/subscription-drawing-v3-<unit>/0025-after-0.png`

The seed PNG and all four final PNGs were opened and inspected. The outline and both
labels are clear. Numerical results are in `subscription-drawing-summary.json`, with
12 m² in all four units within floating-point tolerance.

The first harness incorrectly expected saving a takeoff not to add its derived area
outline. The corrected check verifies the one new outline's layer and 12 m² area, and
that every original entity survives. The next harness incorrectly expected standalone
MCP reopening to load the adjacent project. `headless.rs` explicitly excludes sidecars;
the corrected check reads the saved project separately, verifies its quantity and source
handle, and verifies that standalone reopening retains geometry without importing the
sidecar. The earlier transcripts and failed assertions remain in the non-v3 folders.
No product test was weakened and no production behavior changed to satisfy the harness.

### Sample takeoff and BOQ

The copied audit workflow `.local/subscription_sample_boq.py` passed. Quantities are
**42 m, 14 m, 58.29 m², 22.09 m², 3 doors and 5 windows**. Comparison identifies exactly
**A.4 and B.2**. Both exported workbooks were reopened with the stdlib XML/ZIP reader;
quantity totals are 56 m, 80.38 m² and 8 items. No rate calculation was added.

Evidence:

- `.local/goal-run/subscriptions-sample-boq-v2/transcript.jsonl`
- `.local/goal-run/subscriptions-sample-boq-v2/verified-results.json`
- `.local/goal-run/subscriptions-sample-boq-v2/takeoff-workbook-readback.json`
- `.local/goal-run/subscriptions-sample-boq-v2/conflicts-workbook-readback.json`
- `.local/goal-run/subscriptions-sample-boq-v2/0125-sample-plan-0.png`

The sample PNG was opened and inspected. Its room geometry and both conflict annotations
are visible; the smaller conflict note remains dense at this overview scale. This is not
a claim that the parallel drawing-quality goal has passed. The first copied workbook
inspector still restricted paths to `.local/audit/`; its worktree copy now restricts reads
to this worktree's `.local/goal-run/` instead.

The transcript scan `subscription-e2e-readback-scan.json` found no repeated mutation
requests, no unexpected tool errors, and no reverted requested geometry in these jobs.
Only the four deliberate headless screen requests return errors.

### Fake CLI, credentials and evaluation refusal

- `.local/goal-run/subscription-fakes/claude-argv-environment.jsonl`: fake help/version,
  two user turns, exact permitted tools, strict MCP, isolated settings, resume and cwd.
- `subscription-fakes/claude-events.txt`: the plain stream delivered to Mizan.
- `subscription-fakes/sentinel.json` and `credential-scan.json`: no sentinel opens,
  no forbidden environment variables, no production credential-path access found.
- `subscription-fakes/claude-sigint.jsonl` and `claude-sigterm.jsonl`: interruption and
  escalation. The passing test checks the five-second wait and permit lifetime.
- `.local/goal-run/subscription-cli-refusal/transcript.json`: the built CLI refuses bare,
  uppercase and custom-command `--agent claude` forms; the fake command's marker was never
  created.

The fake process tests pass without sockets. The full fake-process/private-HTTP-MCP test
is present but cannot bind here. Stdio MCP jobs above establish drawing/tool behavior,
not a completed native-provider HTTP integration. No real provider chat or live Settings
screen was used as acceptance evidence.

The existing headless end report still labels these drawing/manual-takeoff jobs as failed
because they have no recognized submitted items. Quantity and geometry assertions above
pass independently. That existing job-classification behavior belongs to parallel Goal D;
this branch does not change it or claim that its report is successful.

## 5. Incomplete work, risks and founder decisions

1. **ChatGPT is disabled.** Supply the exact public SIWC endpoint/issuer/JWKS, host-ID
   parameter, dynamic registration/client-ID/audience, refresh and button/limits-link
   details requested from the reviewer. Finish production registration and refresh wiring
   against that specification. Generic OAuth mocks do not prove current SIWC compatibility.
2. Run the unchanged socket-based tests in an environment that permits local ports. They
   are listed individually in `.local/goal-run/subscription-port-failures.md`. The native
   Claude private-MCP and OAuth/Responses HTTP paths have not passed end-to-end here.
3. Claude is Unix-only and requires Work autonomously. Add and verify other cancellation
   platforms and enforced plan-approval pauses before advertising those modes.
4. The founder alone performs the requested live acceptance using his installed unmodified
   Claude Code and real ChatGPT login, after the missing wiring and mock checks pass.
   Actual CLI compatibility, account eligibility, official button appearance, full desktop
   screenshots and live plan limits have not been verified by this session.
5. Complete the paid/hosted/commercial release gates in
   [the subscriptions guide](2026-10-23-qiyas-subscriptions.md). No implementation can
   guarantee that a provider will never restrict an account; this branch follows the
   supplied policy constraints and does not promise immunity.
