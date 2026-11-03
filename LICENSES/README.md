# Licences of the software in Qiyas

Qiyas is QS Mind's own software. It is built with the open-source libraries, fonts and tools
listed in `THIRD-PARTY.md` (every Rust crate and JavaScript package, with its version and
licence; written by `scripts/third-party.sh`). This folder ships in every installer as
`licenses/`, beside the app.

Goal S also uses three already-listed Rust dependencies directly: `libc` (MIT OR
Apache-2.0) for child-process signals, `ring` (Apache-2.0 AND ISC) for PKCE randomness,
hashing and ID-token signatures, and `url` (MIT OR Apache-2.0) for OAuth encoding.
No Claude Code binary, SDK or adapter is bundled. The fake CLI and generated OIDC signing
key under `crates/qiyas-ai/tests/fixtures/` are test-only Apache-2.0 fixtures, not account
credentials. The third-party inventory was regenerated; these dependencies were already
present and its entries did not change.

## Licence texts here

- `opencadcodec-MPL-2.0.txt`, `opencadkernel-MPL-2.0.txt`: the Mozilla Public License 2.0 (it
  covers every MPL-2.0 library below).
- `nalgebra-Apache-2.0.txt`: the Apache License 2.0, for nalgebra and every other Apache-2.0
  library in `THIRD-PARTY.md` (simba and approx among them).
- `pdfium-BSD-3.txt`: pdfium (BSD-3-Clause), the PDF reader, from
  [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) release
  `chromium/8086`; the licences of the libraries built into it ship beside it in
  `resources/pdfium/`.
- `foldhash-Zlib.txt`: foldhash (Zlib).
- `clipboard-win-error-code-BSL-1.0.txt`: clipboard-win and error-code (BSL-1.0, Windows).
- `tslib-0BSD.txt`: tslib (0BSD), a helper library of the window's UI kit (Base UI, cmdk, Headless
  Tree, TanStack, Sonner: all MIT, listed in `THIRD-PARTY.md`).
- `NotoSans-OFL.txt`, `NotoSansArabic-OFL.txt`, `InstrumentSans-OFL.txt` (added by the
  installer): the SIL Open Font License 1.1 of Noto Sans, Noto Sans Arabic and Instrument Sans.

## Source of the MPL-2.0 libraries

Qiyas uses these MPL-2.0 libraries unmodified, built into its programs. Their source is
available where they are published:

- opencadcodec 0.5.5 (DWG and DXF), commit `a35f43e`:
  <https://github.com/HakanSeven12/opencadcodec/tree/a35f43e>
- opencadkernel 0.1.0 (2D geometry), commit `99b6b48`:
  <https://github.com/HakanSeven12/opencadkernel/tree/99b6b48>
- cssparser, cssparser-macros, dtoa-short, option-ext and selectors (through Tauri and its web
  view), at the versions `THIRD-PARTY.md` lists: <https://crates.io/crates/NAME/VERSION>
