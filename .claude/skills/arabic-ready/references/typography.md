# Arabic typography

Contents: Font picks · Loading fonts · Size and line-height · Never do · Tashkeel · Weights and emphasis · Justification · Mixed Latin/Arabic · Font stacks per platform

All fonts below are free (OFL) on Google Fonts, and every one was downloaded and rendered in Chromium with Arabic, Latin, digits and tashkeel (`evals/verify/out/fonts.png`). All of them include Latin, Western and Arabic-Indic digits, and tashkeel marks.

## Font picks

| Use | Font | Weights | Notes |
|---|---|---|---|
| UI / product (default) | **IBM Plex Sans Arabic** | 100–700 | Neutral, very legible at small sizes. Pairs with IBM Plex Sans / Inter |
| UI, widest coverage | **Noto Sans Arabic** | 100–900 (variable) | Safest fallback. Also covers Persian/Urdu letters |
| Marketing, headings | **Cairo** | 200–1000 (variable) | Geometric, bold headlines. Wide letters, so give it room |
| Marketing, light/modern | **Tajawal** | 200, 300, 400, 500, 700, 800, 900 | Compact. Tight vertical metrics: use line-height ≥ 1.7 with tashkeel |
| Gulf / Saudi brands | **Almarai** | 300, 400, 700, 800 | Clean, popular in KSA |
| Bilingual brand (matched Latin) | **Readex Pro** | 200–700 (variable) | Arabic and Latin designed together |
| Display / headlines (Kufi) | **Noto Kufi Arabic** | 100–900 (variable) | Geometric Kufi. Headings only |
| Long reading, documents, news | **Noto Naskh Arabic** | 400–700 | Classic Naskh text face. Good tashkeel |
| Books, formal, religious, poetry | **Amiri** | 400, 700 | Traditional book Naskh. Very tall ascenders/descenders (needs line-height ≥ 1.9) |
| Fully vocalized text (Quran, education) | **Scheherazade New** | 400–700 | Designed for heavy tashkeel |

Pick one sans for UI and optionally one Naskh for long-form. Don't use more than two Arabic families.

❌ `font-family: Arial` / browser default for an Arabic product (falls back to whatever the OS has: Times-like Naskh on some systems, Tahoma on others)
❌ Latin-only fonts (`Inter`, `Roboto`, `Poppins`) first in the stack. Arabic then falls back to a random system font, mismatched in size and weight
✅ `font-family: "IBM Plex Sans Arabic", "Noto Sans Arabic", Tahoma, system-ui, sans-serif;`

## Loading fonts

HTML / plain CSS:
```html
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans+Arabic:wght@400;500;700&display=swap" rel="stylesheet">
```
Next.js: `next/font/google` with `subsets: ["arabic"]` (see web.md).
Request only the weights you use. Each Arabic weight is roughly 100–200 KB.
Canvas: `await document.fonts.load('20px "IBM Plex Sans Arabic"')` before `fillText`. Otherwise the first draw uses a fallback font *(verified)*.

## Size and line-height

| Text | Arabic | (Latin equivalent) |
|---|---|---|
| Body | 16–18px, line-height **1.7–1.9** | 16px, 1.5 |
| UI labels/buttons | ≥ 14px, line-height 1.5 | 12–14px |
| Headings | line-height 1.3–1.5 | 1.1–1.2 |

Arabic letters have a smaller x-height-equivalent and taller ascenders/descenders than Latin at the same font-size. Go one step larger than the Latin design and loosen line-height.

❌ `line-height: 1.2` on Arabic paragraphs (lines collide, tashkeel overlaps the line above)
✅ `p:lang(ar) { line-height: 1.8; }`

## Never do

| ❌ | Why | ✅ |
|---|---|---|
| `letter-spacing` / `tracking-*` on Arabic | Arabic is cursive. LibreOffice/Word and other renderers insert gaps that break letter joining *(verified in LibreOffice)*. Chromium 141 ignores it for Arabic, but you can't rely on that | `letter-spacing: normal`. For airier headings use a lighter weight or more `word-spacing` |
| `text-transform: uppercase` | Arabic has no case, so it's meaningless and it breaks Latin words in mixed text | Use weight or color for emphasis |
| `font-style: italic` | Arabic fonts have no italics, so browsers draw a synthetic slant leaning the wrong way *(verified)* | Bold, color, background highlight, or a different family |
| Fixed-height boxes with `overflow: hidden` | Tashkeel and tall letters (ل ك ط) get clipped *(verified)* | `min-height` and padding, not `height` + `overflow:hidden` |
| Breaking words with `word-break: break-all` | Splits a word mid-join | `overflow-wrap: anywhere` (only for URLs) |
| Faux bold (loading only 400, using `font-weight:700`) | Smeared, shapes fill in | Load the real 700 file |
| Underline crossing descenders | Hits dots below (ب ي ج) | `text-underline-offset: 0.3em; text-decoration-skip-ink: auto;` |

## Tashkeel (diacritics)

- **UI and most content: no tashkeel.** Native readers don't need it, and partial tashkeel looks amateurish.
- Add marks only to disambiguate (`عِلم` vs `عَلَم`), in children's/education content, Quran/hadith/poetry, or names readers might mispronounce.
- Quranic text: use a font made for it (Amiri, Scheherazade New) and keep the full original marks. Never auto-generate them.
- Give vocalized text room: line-height ≥ 2 and no clipping containers.
- Never strip or add tashkeel silently when processing user text. Note: `arabic_reshaper.reshape()` **deletes tashkeel by default**. `scripts/shape_text.py` turns that off.

## Weights and emphasis
- Use 400 for body and 500–700 for emphasis/headings. Arabic 300 and below get thin at small sizes, so avoid them below 18px.
- Emphasis: **bold**, color, or a highlight background. Not italic, not uppercase, not letter-spacing.
- Quotes: «…» for quotations. Use `"…"` only in technical text.

## Justification and kashida
- `text-align: justify` in browsers stretches **word spaces** only (no kashida). Fine for long paragraphs in wide columns, ugly in narrow ones (rivers of white).
- ❌ Inserting tatweel `ـ` manually to stretch words (`الســـلام`): breaks search, copy-paste and screen readers.
- ✅ `text-align: start` for UI and narrow columns. Use justify only for wide long-form text, with `text-justify: inter-word`.
- Documents: in LibreOffice, plain justify (`w:jc w:val="both"`) on an RTL paragraph already inserts kashida automatically *(verified, `docx_kashida.png`)*. The Word-specific values `lowKashida` / `mediumKashida` / `highKashida` are ignored by LibreOffice, which falls back to start alignment *(verified)*. Their effect in Word was not tested, so use `both` for portability.

## Mixed Latin + Arabic
- Use an Arabic font that ships matching Latin (all fonts above do), or pair: Plex Arabic + IBM Plex Sans, Noto Sans Arabic + Noto Sans/Inter, Readex Pro alone.
- If Latin from the Arabic font looks off, list the Latin font *first* for `:lang(en)` spans, or use `unicode-range` so the Arabic family only serves Arabic code points:
```css
@font-face { font-family: "Brand"; src: url(inter.woff2); unicode-range: U+0000-024F; }
@font-face { font-family: "Brand"; src: url(plex-arabic.woff2); unicode-range: U+0600-06FF, U+0750-077F, U+08A0-08FF, U+FB50-FDFF, U+FE70-FEFF; }
```
- Latin often looks bigger next to Arabic at the same size. A Latin `font-size: 0.95em` inside Arabic text is sometimes needed. Judge on render.

## Font stacks per platform (when you can't ship a web font)
- Web: `"IBM Plex Sans Arabic", "Noto Sans Arabic", "Segoe UI", Tahoma, "Geeza Pro", system-ui, sans-serif`
- Email: `Tahoma, Arial, "Segoe UI", "Geeza Pro", sans-serif`
- Word/PowerPoint (installed everywhere): `Arial`, `Tahoma`, `Times New Roman`, `Traditional Arabic`, `Simplified Arabic`, `Sakkal Majalla` (Windows). Prefer `Arial` / `Tahoma` for portability. Embed fonts or use a Google font only if the reader has it.
- matplotlib / Pillow / PDF: register a TTF explicitly (see charts-images.md and documents.md). The default DejaVu Sans has Arabic glyphs, but they look poor. Always set an Arabic font.
- iOS system Arabic: SF Arabic (system font). Android: Noto Naskh/Sans Arabic (system).
