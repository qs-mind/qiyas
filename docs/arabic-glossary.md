# Terms and Arabic in Qiyas and Mizan

`src/i18n/ar-glossary.json` is the one reference for every term Qiyas uses, in English and
Arabic (founder's ruling, 2 November 2026): engineering terms only. Each concept has its English
term (`en`, with `en_avoid`) and its Arabic term (`preferred`, with `avoid`), and a `context` where
two meanings meet. The window, Mizan's prompts and answers, exports, docs and code comments follow
it; a new term is added there before it is used anywhere.

Settled on 2 November 2026: a drawing **revision** is **إصدار** (رقم الإصدار), a **review** stays
**مراجعة**; a BOQ–drawing **discrepancy** is **تعارض** (never "conflict" in English text) and
what is raised to the engineer is a **query**, **استفسار**; a building **element** and a CAD
**entity** are both **عنصر** (عنصر الرسم when both appear); the job is the **take-off**, **حصر**,
and a hand-measured row a **manual measurement**, **قياس يدوي**; a rule's multiplier is a
**factor**, **معامل**, and a named parametric value a **parameter**, **متغير**; **waste allowance**
نسبة الهالك, **lap** تراكب, **allowance** سماحية, **rounding** تقريب.

The machine-readable style contract lives in the same file. Use familiar Egyptian and Gulf engineering Arabic,
with short, direct sentences. Keep established labels that already read naturally.

- BOQ is **مقايسة**, take-off **حصر**, a BOQ item **بند**, a drawing entity
  **عنصر**, sheet **لوحة**, layer **طبقة** and CAD block **بلوك**. A workbook
  worksheet remains **ورقة عمل**. A work plan is **خطة**, a floor plan **مسقط**.
  An engineering query is **استفسار**; a data query is **استعلام**.
- Write **قيد التنفيذ** or **ميزان يعمل الآن**. Avoid forms whose meaning depends
  on a vowel mark, especially the old working label. Use **توازي**, **تساوي**
  and **محاذي للخط** rather than words that lose their final letter without tanween.
- UI and authored Arabic messages contain no diacritics. The diacritic allow-list
  is currently empty. Exact quotations, names and user drawing content are not
  rewritten. Any future unavoidable authored quotation needs an explicit exception
  and its reason; do not exempt an entire file.
- Separate Arabic from digits, Latin text and units: **19 بند**, **لوحتان A1**,
  **1.50 م**. Keep DWG, PDF and Excel unchanged. Superscripts in **م²** and **م³**
  are unit notation, not counts. Spaces around substitutions also matter.
- For unrestricted counts use **عدد البنود: {n}** or **البنود (العدد: {n})**.
  This avoids a fixed plural that fails for 1, 2 or 19. Preserve explicit singular,
  dual and plural forms where the caller selects them. No tanween after numbers.
- Use **،** and **؟** in Arabic prose. Keep punctuation inside command syntax,
  coordinates, JSON, filenames and formats exact. English halves of bilingual Rust
  messages retain their English punctuation. Never change interpolation names.

Both style tests read this JSON. They check terms, marks, touching letters/digits
and substitutions. The frontend also checks keys, placeholder multisets, empty
values and untranslated text. Rust scans string literals, including raw/multiline
strings, and Arabic prompts. Exact legacy input recognizers and test fixtures have
named, reasoned exceptions: accepting old user text must not change with UI style.
These are source checks, not runtime rewriting or restrictions on Mizan's decisions.
