# Goal W design — one workspace per drawing

25 October 2026. Implementation plan; measured results belong in the companion report.

Each tab owns an `Arc<DrawingWorkspace>`. The workspace owns its session, Mizan,
provider choice, window requests, published drawing metadata and bounded event stream.
The registry owns only tab order, the visible id and those references. Switching changes
the visible id; it never moves a session or resets a conversation. A background turn
holds its workspace reference until completion, even while another tab is visible.

IPC takes an optional `workspace` id. At command entry it resolves that id, or the
visible id, exactly once into a scoped app state. Helpers and asynchronous continuations
use that reference. An omitted id is compatibility for existing callers; the window
will send its displayed id. Unknown explicit ids fail rather than falling back. The
no-drawing panel has a separate workspace which can be adopted by its first drawing.

Lock order and publication:

1. Copy workspace references under the registry lock, then release it. Never wait on
   a drawing, provider, filesystem operation or window while holding the registry.
2. External sheet operations hold a separate workspace guard. Turn admission tries that
   guard before reserving Mizan; external work never holds the status lock for its duration.
   A workspace's short Mizan state lock may precede its session lock. The session
   never takes the Mizan state lock. Neither is held across an await.
3. Published metadata and event storage have separate short locks. Readers of tab
   status and reload state do not take a session lock. Session operations publish only
   after the command or entire batch settles; existing core rollback remains authoritative.
4. UI session access uses try-lock: busy interactions are queued by the window with
   their original workspace id. Rendering retains the last committed canvas. Different
   workspaces have no shared drawing lock. Agent steps remain on blocking workers.
5. Conversation index writes serialize separately across all workspaces. Autosave and
   panic recovery try each workspace independently; one busy drawing cannot prevent
   another from saving. File locks follow cached paths, including hidden drawings.

Mizan's host captures the owning workspace id. Open and switch may show another drawing,
but subsequent tools and Save still address the originating workspace. Responses state
the binding explicitly. Window requests include the id; hidden workspaces cannot capture
or select the visible drawing by accident. Public MCP connections must bind once to a
workspace; explicit workspace addressing must not borrow the visible session mid-call.

A process-wide turn scheduler admits three turns by default (Settings can change it).
Queued turns own a cancellation token and report `queued`. Claude Code has an additional
single slot, held through process reaping; queued Claude work must not occupy all ordinary
provider slots. Under memory pressure admission falls to one without cancelling active
work. On Linux use available memory from `/proc/meminfo`; unknown platforms retain the
configured limit. Report the reason plainly, never invent available-memory measurements.

Provider/model/effort choices are copied from Settings for each workspace. Editing one
workspace's choice affects only its next turn, cannot alter an active request, and never
copies credentials. Existing provider/address keychain rules remain the only key path.
Shared autonomy and measurement settings are read at turn start rather than frozen with the
workspace's answerer choice.

Core-owned transcript events have increasing cursors. Reload subscribes before reading a
snapshot, then ignores duplicate cursors, restoring history, in-flight cards and queue
status without starting or cancelling a turn. Tab status distinguishes working, queued,
waiting for the user, done and failed. Hidden completion records a short summary for the
next activation. A bounded operation journal exposes completed operations and changed
handles; an expired cursor explicitly requests resynchronisation.

Implementation steps: registry and routing; agent lifetime and scheduler; frontend
reattachment/provider/status; event feed; offline acceptance and resource measurements.
Existing tab tests which require cancellation or session swapping will be replaced with
assertions for the approved independent-workspace behaviour, retaining undo, unsaved-work,
recovery and stale-read checks. No geometry or quantity algorithm changes are planned.
