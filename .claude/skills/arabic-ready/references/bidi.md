# Bidi: Arabic mixed with English, numbers and symbols

Contents: How reordering works · The fixes (HTML, CSS, plain text) · Cases: phones, negatives, emails, URLs, code, names + numbers, punctuation, percentages, ranges · Markdown and plain-text channels · Debugging

Every ❌/✅ pair marked *(verified)* was measured in Chromium 141 by comparing glyph x-positions (`evals/verify/node/measure.mjs`).

## How reordering works (the 30-second version)
- In an RTL paragraph, Arabic runs right→left. A run of Latin letters is laid out left→right as one block, and so is a run of digits.
- **Neutral characters** (space, `.` `,` `-` `+` `(` `)` `:` `/` `@` `!` `?`) take the direction of their neighbours. When the neighbours disagree they take the *paragraph* direction (RTL). This is where text breaks.
- A number directly after Latin letters joins the Latin run (`Smith 3` becomes one LTR block).
- Brackets and `< > « » ‹ ›` are *mirrored*: type them in logical order and they display correctly. Arrows `→ ←` are **not** mirrored.

Rule of thumb: **anything that must read left→right as a unit (phone, email, URL, code, version, an English phrase next to a number, a negative number) gets isolated.**

## The fixes

| Context | Isolate an LTR island | Isolate unknown-direction text |
|---|---|---|
| HTML | `<bdi dir="ltr">…</bdi>` or `<span dir="ltr">…</span>` | `<bdi>…</bdi>` (auto) |
| CSS | `unicode-bidi: isolate; direction: ltr;` | `unicode-bidi: isolate;` + `dir="auto"` |
| Inline code | `<code dir="ltr">…</code>` | n/a |
| Block code | `<pre dir="ltr">` | n/a |
| Plain text (SMS, WhatsApp, JSON strings, CSV, push notifications, docx/pptx text) | LRI `U+2066` … PDI `U+2069` | FSI `U+2068` … PDI `U+2069` |
| Plain text, single character fix | LRM `U+200E` (acts as an invisible Latin letter) / RLM `U+200F` (invisible Arabic letter) | n/a |

Prefer `<bdi>` / isolates over LRM/RLM: isolates can't leak into surrounding text. Use LRM/RLM only for one-character problems (a sign, trailing punctuation).

In JS strings: `"⁦" + phone + "⁩"`. In Python: `f"⁦{phone}⁩"`. These characters are invisible but count toward length limits.

## Cases

### Phone numbers *(verified)*
❌ `اتصل على +966 50 123 4567.` → displays `4567 123 50 966+` (groups reversed, `+` moved)
✅ HTML: `اتصل على <bdi dir="ltr">+966 50 123 4567</bdi>.`
✅ Plain text: `اتصل على ⁦+966 50 123 4567⁩.`
✅ Or drop spaces: `0501234567` (one digit run can't reorder). Dashes between digits (`050-123-4567`) also stay together.
Inputs: `<input type="tel" dir="ltr">` *(verified: without it the value displays reversed)*.

### Negative numbers *(verified)*
❌ `درجة الحرارة -5 درجات` → displays `5-`
✅ `درجة الحرارة ‎-5 درجات` (LRM before the sign) or `<bdi dir="ltr">-5</bdi>`
✅ In copy, prefer words: `5 درجات تحت الصفر`
`Intl.NumberFormat('ar-…')` output already contains the right marks (e.g. `؜-` ALM). Use it for computed numbers.

### English name or term followed by a number *(verified)*
❌ `نشر John Smith 3 منشورات` → `3` sticks to `Smith` and appears to its right
✅ `نشر <bdi>John Smith</bdi> 3 منشورات`
Same for product names + numbers: `<bdi>iPhone</bdi> 15`, though `iPhone 15` as one unit is fine if the whole thing is the name.

### Arabic inside an LTR page (English UI showing Arabic user data)
❌ `<li>User إيان: 90 points</li>` → the `90` jumps next to the Arabic name
✅ `<li>User <bdi>إيان</bdi>: 90 points</li>`
Always wrap user-generated strings (names, titles, comments) in `<bdi>` when you interpolate them into a sentence.

### Code, CLI flags, file paths *(verified)*
❌ `أضف الخيار --verbose للتفاصيل` → displays `verbose--`
✅ `أضف الخيار <code dir="ltr">--verbose</code> للتفاصيل`
Code blocks: always `<pre dir="ltr">`. In markdown, fenced code blocks are rendered LTR by most viewers, but inline code inside Arabic paragraphs still needs care. Start the paragraph with Arabic and avoid ending a sentence with inline code.

### Emails and URLs
Usually fine in the middle of an RTL sentence, but:
❌ `<p dir="auto">support@acme.com هو بريد الدعم.</p>`: the first strong character is Latin, so the whole paragraph becomes LTR and the period jumps *(verified)*
✅ `dir="rtl"` for known-Arabic text, or reorder the sentence so it starts with Arabic: `بريد الدعم هو support@acme.com`
URLs with paths/queries: isolate them (`<bdi dir="ltr">acme.com/ar/help?id=3</bdi>`), because `/`, `?` and `=` are neutrals.

### Trailing punctuation after English
`أفضل أداة هي Visual Studio Code!` renders correctly in an RTL paragraph (`!` at the far left) *(verified)*. It breaks only when the paragraph is LTR (missing `dir`), so fix the paragraph direction rather than the punctuation.
If a line *starts* with a Latin word and has no `dir` (plain-text channels), start it with RLM `‏` so the paragraph resolves as RTL.

### Percentages, currency, ranges
- Percent *(verified: Chromium, LibreOffice, Pillow, python-bidi all agree)*: `خصم 30% على` displays the sign on the **left**: `%30`. That is the normal Arabic display, so leave it. If a brand insists on the Latin look `30%`, isolate it: `<bdi dir="ltr">30%</bdi>` / `\u206630%\u2069`. Inside parentheses at the end of a phrase (`(5%)`) it stays `5%`. Don't mix both looks in one document. With Arabic-Indic digits use `٣٠٪`.
- Currency: `49.99 ر.س` / `49.99 درهم` / `$49.99`. Put a Latin symbol on its usual side (`$49.99`) and an Arabic abbreviation *after* the number (`49 ر.س`). With `Intl.NumberFormat`, the locale decides (see writing.md).
- Ranges: `5-10` and `9:00-17:00` stay together *(verified)*. In prose prefer `من 5 إلى 10 أيام`.
- Times: `الساعة 3:30 م` (م = PM, ص = AM).

### Mixed-direction lists and tables
- Each cell/list item is its own paragraph. Give it `dir="auto"` if its content language varies (e.g. product names that are sometimes English).
- A column of English SKUs inside an RTL table: `<td dir="ltr" class="text-end">`.

## Markdown, chat, and plain-text channels
You don't control the renderer, so:
1. **Start every line/paragraph with an Arabic word.** Most viewers (GitHub, Slack, WhatsApp, iOS/Android notifications) choose the direction from the first strong character.
   ❌ `Claude Code هو أداة برمجة`
   ✅ `أداة Claude Code تساعدك على البرمجة`
2. **Don't start a line with a number, emoji or English word.** If you must, prefix RLM `‏`.
3. **Bullet lists in markdown:** `- ` + Arabic text works. Numbered lists `1.` render with the number on the correct side in GitHub/most viewers once the item starts with Arabic.
4. **Tables in markdown:** column order is *not* flipped by most renderers (GitHub renders tables LTR). Put the first logical column on the left anyway, because readers expect it.
5. **Emoji** are neutral. An emoji at the end of an Arabic sentence lands at the left end, which is correct. An emoji at the very start of a line can flip the line's direction on some platforms, so start with a word.
6. **Hashtags:** `#عروض_رمضان` (underscore, not space). Mixed hashtags (`#Ramadan2026`) are fine at the end of a line.

## Debugging checklist
- Wrong side for the whole paragraph? The **paragraph direction** is wrong: add `dir="rtl"` / start with Arabic / prefix RLM.
- One number or symbol misplaced? **Isolate** that island (`<bdi>`, LRI/PDI) or add one LRM.
- Letters disconnected or reversed? That's not bidi, it's **shaping**: see charts-images.md / documents.md.
- In code, print `[hex(ord(c)) for c in s]` to spot stray LRM/RLM/isolates (`200e 200f 2066-2069`) left over from copy-paste.
