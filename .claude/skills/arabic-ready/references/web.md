# Web and app RTL

Contents: Setup · Logical CSS · Tailwind · Layout (flex, grid, position, transforms) · Icons · Forms · Tables · Bilingual sites · Email HTML · React Native · Flutter

Everything under "Setup" through "Tables" was render-checked in Chromium (Playwright). React Native and Flutter sections follow the frameworks' documented APIs and were not render-tested here.

## Setup

### Plain HTML
❌
```html
<html>
<body style="direction: rtl; text-align: right">
```
✅
```html
<html lang="ar" dir="rtl">
```
`lang` selects Arabic font fallback, hyphenation and the screen-reader voice. `dir` flips the whole box model, flex/grid order, scrollbars and form controls. Set them on `<html>` only, and add `dir` on inner elements only where the direction changes.

User-generated or unknown-direction content: `dir="auto"` (direction picked from the first strong character).
```html
<p dir="auto">{comment.body}</p>
<input name="search" dir="auto">
```
Caveat (verified): `dir="auto"` on Arabic text that *starts* with an email/URL/English word becomes LTR. For known-Arabic content use `dir="rtl"`.

### Next.js (App Router)
```tsx
// app/layout.tsx
import { IBM_Plex_Sans_Arabic } from "next/font/google";
const plex = IBM_Plex_Sans_Arabic({ subsets: ["arabic"], weight: ["400", "500", "700"], display: "swap" });

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ar" dir="rtl" className={plex.className}>
      <body>{children}</body>
    </html>
  );
}
```
With i18n routing (`app/[locale]/layout.tsx`): `dir={locale.startsWith("ar") ? "rtl" : "ltr"}`.
Pages Router: set the same attributes on `<Html>` in `pages/_document.tsx`.

### React SPA / Vue (client-side switch)
```js
document.documentElement.lang = "ar";
document.documentElement.dir = "rtl";
```

## Logical CSS

| ❌ Physical | ✅ Logical |
|---|---|
| `margin-left` / `margin-right` | `margin-inline-start` / `margin-inline-end` |
| `padding-left` / `padding-right` | `padding-inline-start` / `padding-inline-end` |
| `left: 0` / `right: 0` | `inset-inline-start: 0` / `inset-inline-end: 0` |
| `text-align: left` / `right` | `text-align: start` / `end` |
| `float: left` / `right` | `float: inline-start` / `inline-end` |
| `border-left` | `border-inline-start` |
| `border-top-left-radius` | `border-start-start-radius` |
| `margin: 0 16px 0 8px` (shorthand) | `margin-block: 0; margin-inline: 8px 16px` |
| `width` / `height` (fine) | `inline-size` / `block-size` (optional) |

Things that do **not** flip even with `dir="rtl"`:
- `transform: translateX(10px)` and `rotate()`. Override them under `[dir="rtl"]`.
- `background-position: left`. Use `background-position: right` under `[dir="rtl"]`, or the `left`/`right` keywords set per direction.
- `linear-gradient(to right, …)`. Flip under `[dir="rtl"]`.
- `box-shadow` / `text-shadow` x-offsets. Usually fine either way. Flip them if the shadow implies a light source.
- Keyframe animations that slide in from the left.

```css
.drawer { inset-inline-start: 0; transform: translateX(-100%); }
[dir="rtl"] .drawer { transform: translateX(100%); }
```

## Tailwind CSS (v3.3+ and v4)

| ❌ | ✅ |
|---|---|
| `ml-4 mr-2 pl-3 pr-1` | `ms-4 me-2 ps-3 pe-1` |
| `left-0 right-4` | `start-0 end-4` |
| `text-left text-right` | `text-start text-end` |
| `rounded-l-lg rounded-tr-md` | `rounded-s-lg rounded-se-md` |
| `border-l-2 border-r` | `border-s-2 border-e` |
| `float-left` | `float-start` |
| `space-x-4` (margin-based) | `flex gap-4` (direction-proof) |
| `bg-gradient-to-r` | `bg-gradient-to-r rtl:bg-gradient-to-l` |
| `translate-x-full` | `translate-x-full rtl:-translate-x-full` |
| `-ml-2` | `-ms-2` |

The `rtl:` / `ltr:` variants apply when an ancestor has `dir="rtl"` / `dir="ltr"`:
```html
<svg class="size-4 rtl:-scale-x-100">…</svg>          <!-- mirror a directional icon -->
<div class="ltr:origin-left rtl:origin-right">…</div>
```
Never use `tracking-*` on Arabic text. If a design system applies `tracking-wide` to headings, add `rtl:tracking-normal` or scope it with `ltr:tracking-wide`.

## Layout

- **Flexbox/grid follow `dir` automatically.** `flex-row` starts at the right in RTL, and `grid-template-columns` first column is on the right.
  ❌ `flex-row-reverse` or `order` hacks to fake RTL (they double-flip once `dir="rtl"` is set)
  ✅ plain `flex-row` / `grid` with `dir="rtl"` on the root
- **`justify-content: flex-start`** = right side in RTL. `justify-content: left` does not flip, so avoid it.
- **Absolute positioning:** use `inset-inline-start/end` (`start-*`/`end-*`).
- **Horizontal scroll / carousels:** with `dir="rtl"`, `scrollLeft` is 0 at the start and *negative* toward the end (current spec, Chromium/Firefox/Safari). Libraries: Swiper `dir="rtl"` attribute on the container; Embla `direction: 'rtl'` option; Keen `rtl: true`. Swipe "next" goes to the left.
- **Sliders/progress:** `<input type="range">` and `<progress>` fill from the right automatically under `dir="rtl"`. Custom ones need `inset-inline-start` for the fill.
- **Charts embedded in pages:** keep time axes left→right. See charts-images.md.

### What flips
Navigation order and sidebar side, breadcrumbs, pagination (« السابق on the right, التالي » on the left), steppers, tabs, carousels, table columns, toasts/drawers, close buttons (top-left), dropdown alignment, tooltips' arrow side, avatars' status badges.

### What doesn't
Phone numbers, card numbers, code blocks (`<pre dir="ltr">`), terminal output, video controls, timeline/time-series axes, logos, maps, media progress (video scrubbers stay LTR on YouTube/Netflix Arabic).

## Icons

Mirror icons whose meaning is direction (it implies motion along the reading line):

| Mirror | Don't mirror |
|---|---|
| arrow-left/right, back/forward, chevrons, caret in breadcrumbs | play, pause, fast-forward, rewind (media keeps LTR time) |
| send/paper-plane, reply, reply-all, forward (email), undo/redo | checkmark, close ×, plus, minus, search 🔍 |
| logout/login door arrows, external-link | clock, refresh/sync (circular), download/upload (vertical) |
| list with bullets on one side, text-indent, "trending" flat arrow | logos, brand icons, flags, emoji, numbers, charts icons |
| progress/battery/volume bars that fill directionally | cart*, phone, camera, help ?, question mark |

*A shopping cart is mirrored in some Arabic stores. Either is acceptable, so pick one and stay consistent.

❌
```tsx
<ChevronRight className="size-4" />
```
✅
```tsx
<ChevronRight className="size-4 rtl:-scale-x-100" />
```
```css
[dir="rtl"] .icon-directional { transform: scaleX(-1); }
```
Or swap the component based on direction: `const Next = isRTL ? ChevronLeft : ChevronRight`.

Material Symbols (web font) do **not** auto-mirror. Add the flip class. Flutter's `Icons.arrow_back` etc. **do** auto-mirror.

Text arrows: `→` is not a mirrored character, so in Arabic "next" it points backwards. Use `←` (or a mirrored icon). Brackets `(` `)` `<` `>` `›` `«` are bidi-mirrored, which means type them in logical order and they display correctly.

## Forms

- Labels on the right, inputs fill from the right: automatic with `dir="rtl"`.
- **Phone, email, URL, username, password, code, IBAN, card number:** `dir="ltr"` so `+`, `@` and digit groups don't reorder. Keep them right-aligned to line up with the form.

❌ (verified: shows `4567 123 50 966+`)
```html
<input type="tel" value="+966 50 123 4567">
```
✅
```html
<input type="tel" dir="ltr" class="text-end" placeholder="05X XXX XXXX" autocomplete="tel">
<input type="email" dir="ltr" class="text-end" autocomplete="email">
```
- Placeholders in Arabic need no special handling, but placeholders that *show a format* (`name@example.com`) live in an LTR input, which is correct.
- Validation messages: write in Arabic, and wrap echoed values: `البريد <bdi>{value}</bdi> غير صالح`.
- Checkbox/radio sit on the right of their label automatically, so don't add `ml-2`. Use `gap-2` on a flex row.

## Tables
- Column order flips automatically with `dir="rtl"` (first column on the right).
- Numeric columns: `text-align: end` is right in RTL, so prices line up on the right edge. For decimal alignment use `font-variant-numeric: tabular-nums`.
- Sort arrows ▲▼ are vertical, so don't mirror them.

## Bilingual / language switch
```tsx
const dir = locale.startsWith("ar") ? "rtl" : "ltr";
<html lang={locale} dir={dir}>
```
- The switch button shows the target language in that language: `English` on the Arabic page, `العربية` on the English page.
- English brand names inside Arabic copy are fine. Isolate them when they sit next to numbers (see bidi.md).
- Don't put the whole English site in Arabic fonts or the reverse. Use `:lang(ar)` / `:lang(en)` to pick font stacks.

```css
:root { font-family: "IBM Plex Sans Arabic", "Noto Sans Arabic", system-ui, sans-serif; }
:lang(en) { font-family: Inter, system-ui, sans-serif; }
```

## Email HTML (newsletters, transactional)
Many email clients (Outlook desktop, older Gmail apps) ignore logical properties and `dir` inheritance. Use physical properties **plus** `dir` here. This is the one case where `rtl_check.py` warnings are expected, so mark those lines with `rtl-ok`.
```html
<html lang="ar" dir="rtl">
<body dir="rtl" style="margin:0">
  <table role="presentation" width="100%" dir="rtl">
    <tr><td align="right" dir="rtl" style="text-align:right; font-family: Tahoma, Arial, sans-serif; line-height:1.8"> <!-- rtl-ok -->
      مرحباً أحمد،
    </td></tr>
  </table>
</body>
```
Safe email fonts for Arabic: `Tahoma`, `Arial`, `Segoe UI`, `Geeza Pro` (Apple), with web fonts as progressive enhancement.

## React Native (not render-tested here)
```tsx
import { I18nManager } from "react-native";
I18nManager.allowRTL(true);
I18nManager.forceRTL(true);   // takes effect after an app reload (e.g. Updates.reloadAsync() in Expo)
```
Expo: in `app.json` set `"expo": { "extra": { "supportsRTL": true } }` (and `"forcesRTL": true` to force it), plus `expo-localization` to read the device locale.

| ❌ | ✅ |
|---|---|
| `marginLeft`, `paddingRight` | `marginStart`, `paddingEnd` |
| `left: 0` | `start: 0` |
| `borderTopLeftRadius` | `borderTopStartRadius` |
| `textAlign: 'left'` | omit (`'auto'`) and set `writingDirection: 'rtl'` on Text if needed |
| `flexDirection: 'row-reverse'` to fake RTL | `'row'`, which flips automatically when `I18nManager.isRTL` |

RN swaps `left`/`right` style props in RTL by default (`I18nManager.swapLeftAndRightInRTL`), but use start/end anyway. They are explicit and behave the same on react-native-web.
Icons: `style={{ transform: [{ scaleX: I18nManager.isRTL ? -1 : 1 }] }}` on directional icons only.
Phone/email `TextInput`: `style={{ writingDirection: 'ltr', textAlign: 'right' }}` + `keyboardType="phone-pad"` / `"email-address"`.
Fonts: load an Arabic font (e.g. `@expo-google-fonts/ibm-plex-sans-arabic`) and set `fontFamily` per weight. Android does not synthesize weights for custom fonts.

## Flutter (not render-tested here)
```dart
MaterialApp(
  locale: const Locale('ar'),
  supportedLocales: const [Locale('ar'), Locale('en')],
  localizationsDelegates: const [
    GlobalMaterialLocalizations.delegate,
    GlobalWidgetsLocalizations.delegate,
    GlobalCupertinoLocalizations.delegate,
  ],   // from package:flutter_localizations
  theme: ThemeData(fontFamily: 'IBMPlexSansArabic'),
  ...
)
```
The locale sets `Directionality` to RTL for the whole tree. For a sub-tree use `Directionality(textDirection: TextDirection.rtl, child: ...)`.

| ❌ | ✅ |
|---|---|
| `EdgeInsets.only(left: 16)` | `EdgeInsetsDirectional.only(start: 16)` |
| `EdgeInsets.fromLTRB(16, 0, 8, 0)` | `EdgeInsetsDirectional.fromSTEB(16, 0, 8, 0)` |
| `Alignment.centerLeft` | `AlignmentDirectional.centerStart` |
| `Positioned(left: 0)` | `PositionedDirectional(start: 0)` |
| `TextAlign.left` | `TextAlign.start` |
| `BorderRadius.only(topLeft: …)` | `BorderRadiusDirectional.only(topStart: …)` |

`Row` children start on the right automatically under RTL. Material `Icons.arrow_back`, `Icons.chevron_right` etc. mirror automatically. For custom directional icons: `Transform.flip(flipX: Directionality.of(context) == TextDirection.rtl, child: icon)`.
Phone/email fields: `TextField(textDirection: TextDirection.ltr, textAlign: TextAlign.end, keyboardType: TextInputType.phone)`.
