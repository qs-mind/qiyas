# Qiyas subscriptions: permissions and release gates

The founder supplied the reviewer's live verification dated **23 October 2026** and
instructed this work not to fetch the pages again. The earlier DNS failure is retained in
[the preflight transcript](../.local/goal-run/subscription-preflight/transcript.txt) as history;
it is no longer the reason for stopping implementation.

Claude Code is implemented for Unix. ChatGPT sign-in is enabled using the exact SIWC wire
specification supplied by the reviewer on the same date. Automated verification uses fake
credentials and mock servers only. This is not a guarantee against future provider policy
changes or account restrictions; the founder performs live acceptance himself.

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

Users must be able to use their ChatGPT plan without paying Qiyas. A structured provider
eligibility error is shown in plain English and Arabic; Qiyas does not maintain a guessed list of eligible plans.

The OS keychain entry `qiyas` / `chatgpt-plan` holds the entire account vault, including the
opaque `urn:uuid:<random UUID>` host ID. It is generated once and survives sign-out. Each
account has its own issued client ID and tokens. The account picker exposes only a random
local ID, display name and connection state. No registration or credential is saved in
settings.json, a conversation, a token file or logs. Mock tests use a synthetic signing key and fake credentials.

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
- Complete the socket-capable mock integration checks, platform support and the founder's
  live acceptance. The reviewed button label is exactly **Continue with ChatGPT** /
  **المتابعة باستخدام ChatGPT** and has comparable prominence to the other account controls. Policy
  confirmation is not evidence that an untested implementation works.

## Implemented SIWC contract

- Authorization: `https://auth.openai.com/api/accounts/authorize`; token exchange and refresh:
  `https://auth.openai.com/api/accounts/oauth/token`, form encoded, with no client secret.
- Discovery: `https://auth.openai.com/.well-known/openid-configuration`; the issuer must be
  `https://auth.openai.com`. JWKS and revocation URLs come from discovery. Redirects are
  disabled. The browser callback binds only to `127.0.0.1`, on a fresh port, at `/auth/callback`.
- First authorization uses `dynamic_agent_client`, `agent_name_hint=Qiyas`, `ext_agent_host_id`,
  fresh state/nonce and S256 PKCE. The exact scope is `openid profile email offline_access
  resource.invoke chatgpt.tokens.use.direct`; resource is `https://api.openai.com/v1`.
  Reauthorization reuses the issued `oaiapp_...` ID and omits the first-registration hint.
  The implementation omits optional login and ID-token hints, so authorization URLs contain
  no tokens. Callback state and client ID must match; consent denial also requires state.
- Token exchange uses the callback's issued client ID and the exact redirect. An invalid code
  is discarded and permits one fresh browser attempt. The ID token is checked against the
  discovered signing keys for signature, issuer, audience, expiry and nonce; `sub` identifies
  the account. A different account cannot replace an existing registration's identity.
- Missing plan scope keeps the sign-in but blocks model discovery and inference. Refresh
  omits scope, includes resource, and replaces access/refresh tokens, expiry and granted
  scopes together under a session lock. A started rotation and its keychain write keep the
  lock through cancellation, so sign-out revokes the latest token. Queued requests cannot
  move to another account. Missing scopes cannot silently inherit plan access.
  Temporary failures retain saved credentials. Terminal refresh errors clear only tokens
  and allow one OAuth recovery with the saved client ID. `invalid_client` is a setup error.
- Account changes cancel requests and reload `GET https://api.openai.com/v1/models`.
  Only `models[]` entries with `visibility=list` appear; labels use `display_name` and requests
  send `slug`. This provider never uses OpenAI-compatible base URL settings.
- Inference uses `POST https://api.openai.com/v1/responses`, bearer authorization,
  `store:false`, `stream:true`. Only `response.completed` succeeds. Tools, reasoning and
  usage map to Mizan's existing provider events. Incomplete, failed and broken streams stop.
- Admission 401 and policy 403 errors do not erase credentials or trigger OAuth loops.
  Ineligible accounts and setup errors stop. Plan limits pause requests with no invented
  reset time; the user checks ChatGPT settings and explicitly chooses to try again.
  Temporary failures use Qiyas's bounded backoff (at most four retries over two minutes).
  Unsupported optional capabilities are removed only at the identified parameter; the body
  must change before retry, with a visible notice. Unknown or required parameters stop.
- Sign-out cancels requests before revocation. It posts the refresh token, its type hint and
  issued client ID to the discovered revocation endpoint. Empty HTTP 200 is success; network
  and server failures permit two retries. Local tokens are removed even if confirmation fails,
  with a message asking the user to remove Qiyas in ChatGPT settings. Host ID and registrations
  remain in the keychain. Other accounts retain their separate credentials.

## Verification boundary

No real Claude binary, ChatGPT login, live service or paid call is used during checks.
Socket-dependent Goal S tests carry an explicit `Requires a socket-capable runner` annotation.
The reviewer runs them with the build prefix appropriate to their checkout:

```sh
CARGO_TARGET_DIR=/home/kareem/Work/qs-mind/qiyas/.local/wt/goal-s/target \
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
cargo test --offline -p qiyas-ai --lib chatgpt_plan:: -- --ignored --test-threads=1
```

These tests cover mock callback/token/discovery/Responses endpoints, rotation, terminal
refresh failures, revocation, model lists, unsupported features, limits and broken streams.
They are separate from the non-socket security/unit tests and the stdio MCP drawing acceptance
jobs. See the Goal S report for actual commands, counts and artifact paths.
