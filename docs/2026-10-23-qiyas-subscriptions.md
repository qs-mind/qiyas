# Qiyas subscriptions: permissions and release gates

The founder supplied the reviewer's live verification dated **23 October 2026** and
instructed this work not to fetch the pages again. The earlier DNS failure is retained in
[the preflight transcript](../.local/goal-run/subscription-preflight/transcript.txt) as history;
it is no longer the reason for stopping implementation.

Claude Code is implemented for Unix. ChatGPT's security and transport components are
implemented, but production sign-in remains disabled pending the exact SIWC registration
and endpoint specification requested from the reviewer. This document does not claim that
ChatGPT sign-in is ready or that an account can never be restricted by its provider.

## What the verified sources allow

### ChatGPT

Sources: [open-source token sharing](https://developers.openai.com/siwc/token-sharing-open-source)
and [Sign in with ChatGPT terms](https://openai.com/policies/sign-in-with-chatgpt-terms/).

The reviewed flow covers open-source, locally hosted applications. It uses browser OAuth,
PKCE, state, nonce, a loopback callback, first-time `dynamic_agent_client` registration,
`agent_name_hint` set to `Qiyas`, and a persisted opaque host ID. The host ID must never be
an email or user ID. A valid ID token alone does not authorize inference: the granted scope
must include `chatgpt.tokens.use.direct`.

Mizan's own requests use the Responses API with `store: false` and `stream: true`. The
reviewed restriction is: “Use the user's plan only for the application they connected.
Do not provide general-purpose API access for other tools.” Qiyas exposes no subscription
inference endpoint through MCP and never passes the ChatGPT token to Claude Code.

Users must be able to use their ChatGPT plan without paying Qiyas. The provider's own
eligibility error is shown; Qiyas does not maintain a guessed list of eligible plans.

Tokens belong only in the OS keychain, service `qiyas`, account `chatgpt-plan`. The host ID
file contains only a random UUID. UI responses contain display name and connection state,
never token values. Mock tests use a synthetic signing key and fake credentials.

### Claude Code

Sources: [legal and compliance](https://code.claude.com/docs/en/legal-and-compliance),
[headless CLI](https://code.claude.com/docs/en/headless), and the reviewer's supplied quote
from support article **15036540, Use the Claude Agent SDK with your Claude plan**, updated
7 October 2026.

The reviewed legal page prohibits third-party Claude.ai login and collecting, storing or
intermediating Claude.ai credentials. It also says this does not prevent an end user from
signing in to the unmodified Claude Code binary with their own subscription, including
where a platform hosts Claude Code. The binary must not be modified, and advertised Pro/Max
limits assume ordinary individual use. The help article says that `claude -p` and
third-party apps can still use subscription limits.

Qiyas follows the narrower founder rules: no SDK or ACP adapter, no Claude credential
access, no bundled/downloaded/patched executable, and no scripted login. The user runs
`claude`, then `/login`, in their own terminal. Qiyas invokes only documented CLI commands.
It checks help at runtime, refuses missing isolation flags, and limits tools to
`mcp__qiyas__qiyas_read`, `mcp__qiyas__qiyas_execute` and `mcp__qiyas__qiyas_capture`.

The child receives an allowlist of ordinary environment settings. Provider credentials,
custom endpoints, headers, proxies and cloud-provider overrides are excluded. Built-in
tools and user/project settings/hooks are disabled through documented CLI options. The
working directory is an empty `claude-work` folder under Qiyas's data home (debug:
`.local/qiyas-home/claude-work`). Qiyas does not inspect Claude's credential directories.

There is one inference process per user message, no background inference, no parallel
sessions and no automatic Qiyas retries. Claude Code owns its own transient retry policy;
authentication failures, rate limits, billing errors and account holds stop the turn.
Cancel sends SIGINT, followed by SIGTERM after five seconds if necessary. The next turn
cannot begin until the old child has been reaped. Non-Unix platforms currently refuse this
provider because equivalent cancellation has not been implemented there.

Claude Code currently requires the existing **Work autonomously** plan-approval mode.
Qiyas refuses per-job and per-phase approval modes before starting the process: the native
CLI owns its internal tool loop, so Qiyas must not silently bypass those approval settings.
Adding those pauses remains separate acceptance work.

`--bare`, `--dangerously-skip-permissions` and budget tricks are never used. In particular,
the reviewed headless page says `--bare` skips OAuth and needs an API key.

`qiyas-cli eval-agent --agent claude` is unconditionally refused, including when a custom
agent command is supplied. Benchmarks use ChatGPT, Codex or an API key. No test in this work
runs the real installed Claude Code, starts real ChatGPT sign-in or makes a paid call.

## Before paid, hosted or commercial release

- For a paid or remotely hosted ChatGPT integration, complete OpenAI's interest form linked
  from the official SIWC page and satisfy the resulting requirements before release. The
  reviewer's quote is: “If you're interested in offering it in a paid or remotely hosted
  app, complete the interest form.” Submission alone is not recorded as acceptance.
- Products running Claude Code must comply with Anthropic's applicable
  [Commercial Terms](https://www.anthropic.com/legal/commercial-terms) and the Claude Code
  legal page. Personal subscription use is not assumed to authorize a commercial hosted
  service or pooled accounts.
- Complete the SIWC wire specification and button guidance review, mock security and
  integration checks, platform support, and the founder's live acceptance. Policy
  confirmation is not evidence that an untested implementation works.

## Remaining SIWC details

The policy excerpts omit the exact authorization/token/Responses endpoints, issuer/JWKS
URL, host-ID parameter name, how registration returns/reuses a client ID, the resulting
ID-token audience, refresh-specific rules and button guidelines. A question requesting
those public details is pending. The code deliberately has no guessed production
configuration; the Settings button explains that sign-in is unavailable in this build.
The generic implementation and mocks are preparation for that final wiring, not proof of
compatibility with OpenAI's current SIWC service.
