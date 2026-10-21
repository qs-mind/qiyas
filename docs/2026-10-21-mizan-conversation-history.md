# Mizan conversation history

Conversations belong to the application, outside the drawing's shared `.qiyas` project.
`<app config>/conversations/<id>.json` holds the model history and visible transcript;
`index.json` holds list metadata only. Files are atomically replaced with owner-only access
(0600 on Unix). The store keeps the 100 most recently updated chats.

Mizan saves at turn completion, before a conversation reset, and at recoverable checkpoints
during a turn. Startup repairs an index interrupted after a conversation file was written;
ordinary listing reads only the index. Panic and clean exit also attempt a checkpoint save.
The first message supplies
the title (60 Unicode characters at most), and the drawing association follows Save As.
History initially lists the current drawing's chats, with All drawings one click away.
Individual and bulk deletion are supported. An empty thread offers Continue for its drawing.

Resume replays the saved events through the live transcript reducer and finishes old Undo
buttons. It switches to an already open drawing or uses the ordinary file-opening and
autosave recovery flow. A missing or unsaved drawing continues on the drawing on screen,
with a notice. Resuming while a turn runs is refused.

The saved system prompt stays byte-identical, and existing compaction still applies. Provider
memory includes Gemini's call signatures and refused optional request fields. A different
provider or model drops thinking, raw provider blocks and that memory. The first resumed
message tells Mizan to reread the drawing. Pictures and thumbnails become `[picture removed]`;
no API key is part of a conversation.

Approval modes are `per_job` (default), `per_phase` and `autonomous`. A multi-phase plan
pauses for job approval; per-phase mode also pauses when a phase completes. Approve sends
`Approved, continue.`; Change focuses the composer. Unfixed errors introduced by Mizan
stop progress in every mode.

Error attribution is explicit: before the first edit of a phase, Mizan records the existing
Error findings by `(sheet, rule, sorted handles)`. Successful mutations accumulate their
`added`, `changed` and `removed` handles. At a phase becoming done, changing the active phase,
or ending a turn, it reruns the drawing check on those handles. A current Error absent from
the baseline triggers approval in every mode; fixing it within the phase removes the reason
to pause. The baseline and touched handles survive turns and restart in `phase_state`; another
drawing clears them, while Save As preserves them. Approval alone does not erase unresolved
errors. The checker exposes an untruncated report for this comparison so the usual 200-row
display cap cannot misidentify an old error. Existing checker rules and analysis bounds remain
unchanged; this attributes errors detected by those checks, not every possible drawing defect.

Measurement method selection stores a name. Local rule files live in the Qiyas data home's
`methods/` directory; release uses `~/.qiyas`, debug uses `.local/qiyas-home` in the repository,
and `QIYAS_HOME` overrides both. The data home creates `methods/` and `projects/` on first use.
Only the version 1 envelope is checked: name, title, based_on, source_note and an array of
opaque rules. The original envelope without a version means version 1; unsupported explicit
versions and malformed envelopes are ignored. The state reports the selected name and
whether rules are loaded. Missing or empty rules mean geometric quantities. Phase 17 owns
rule interpretation; this change ships no method content.

The provider-switching test previously changed the Model select before its asynchronously
loaded option existed. It now waits for the requested option. Vitest 5 calls its repetition
flag `--repeats`; `--repeat` is unsupported in the installed version.

No manual Tauri session or live provider request was run. The reviewer owns those checks,
including the Gemini restart test with Kareem's approval and key.
