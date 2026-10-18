# Candidate label language — 2026-10-15

Scope: QSM-462. Local source change only. Technical review and desktop acceptance are separate gates.

`label_spaces` accepts optional `language: "ar" | "en"`. Both system prompts instruct the model to use the request language. This takes priority over the seed name. With no explicit language, Arabic seed names use Arabic; other or missing names use English. The fallback reads characters only; drawing text remains data.

Arabic: `مساحة مرشحة غير معتمدة 51.21 م²`.
English: `Unapproved candidate 51.21 m²`.

The numeric calculation, decimal setting, label position, one-step undo and output metadata stay unchanged. In particular, `measurement_basis=unapproved_net`, `room_type_status=unapproved` and `net_accuracy_eligible=false` remain fixed. The tool has no net approval input.

Tests: authoring 13/13 passed. This includes explicit Arabic with an English seed, explicit English with an Arabic seed, both fallback languages, invalid language rejection, label content, metadata and existing undo checks. Prompt tests 7/7 passed, including the 700-word limit. Tool tests: 23/23 passed. `git diff --check` passed.

Review only the language field/enum, candidate-label construction, schema, bilingual prompt instructions and label tests. The shared checkout contains earlier uncommitted quantity-basis changes and other work. Preserve them. No commit, merge, release or desktop acceptance is claimed.

Tester handoff after technical acceptance: use the same 12 m² fixture from docs/ai/desktop-quantity-basis-2026-10-15; request labels in Arabic and English, including opposite-language seed names. Check visible text in Tauri, unchanged 12.00 value, and both unapproved flags. Keep this separate from the blocking layout and save defects.

Source SHA-256:

```text
d0cd42b8056698921abf523a41fdd2eab2b2f9a4d200c1ea51902bf4c6cc4848  crates/qiyas-ai/src/execute/author.rs
386b120eebb413e1671c0c4359b04f279cf487ea84a782e28d0ae060dc08e584  crates/qiyas-ai/src/tools.rs
60c5684ce34b828cbb4eb0d319e94b78ca3b3297dd3ca2ed00b864626cd76b2d  crates/qiyas-ai/src/prompt.rs
2a15c6662f02e1d4da08e97bae86eee0c6cd0d253e4655986aa671a5838b0a26  crates/qiyas-ai/tests/authoring.rs
```
