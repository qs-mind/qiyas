# Qiyas brand

Qiyas uses the one QS Mind brand: the geodesic sphere with its upper-right face filled with Qiyas's metal, **Anodised titanium** (`#C9A6E0 → #6E4A9E`). Copper is QS Mind's only. The one slogan: **كل رقم له مصدر** / **Every number has a source**.

These files are Qiyas's own copy of the brand, kept in this repo. The QS Mind house kit and the full guide live in the website repo (`qs-mind/website`, `brand/`). Change the brand there first, then copy the files here.

## Files

Hand-structured, transparent SVGs with no fonts and no fixed size; text is drawn as shapes.

```
favicon/favicon.svg
lockup/qiyas-lockup-ar-white.svg
lockup/qiyas-lockup-ar.svg
lockup/qiyas-lockup-white.svg
lockup/qiyas-lockup.svg
mark/qiyas-mark-white.svg
mark/qiyas-mark.svg
```

- `mark/` the sphere: `-mark.svg` ink lines for light grounds, `-mark-white.svg` white lines for dark grounds.
- `lockup/` mark + "QS Mind | Qiyas" (`-ar` files: كيو إس مايند | قياس).
- `favicon/favicon.svg` the full mark; switches ink/white with light and dark mode.

App icons, favicons and PNGs in this repo are rendered from these files.

## Rules

- One mark at every size: resize the full mark. There is no simplified small mark.
- Ink files on light grounds, white files on dark grounds.
- The face stays upper right in Qiyas's metal. Never rotate the sphere, add shadows or glow, or put it on a tile. (The only opaque ground is the Stone `#F2EEE7` square that iOS and Android force on home-screen icons.)
- Clear space around the mark: one quarter of its width.
- The product name always sits next to a product mark.
- Arabic text uses Thmanyah Sans; titles turn on its swash letters (`swsh`). Font files never go into a repo.
