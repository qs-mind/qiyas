# Qiyas (قياس)

[![CI](https://github.com/qs-mind/qiyas/actions/workflows/ci.yml/badge.svg)](https://github.com/qs-mind/qiyas/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/qs-mind/qiyas)](https://github.com/qs-mind/qiyas/releases/latest)
[![Licence: Apache-2.0](https://img.shields.io/badge/licence-Apache--2.0-blue)](LICENSE)

Desktop measurement CAD for quantity surveyors, by [QS Mind](https://qsmind.app). Open DWG and
DXF drawings, measure and edit them, plot to PDF or a printer, run scripts headless, and let the
AI assistant (Mizan) build and check bills of quantities against the drawing. English and Arabic.

Tauri 2, React, TypeScript and Rust. Apache-2.0 (`LICENSE`); the libraries we ship and their
licences are listed in `LICENSES/THIRD-PARTY.md`. The DWG/DXF reader and the geometry kernel
([opencadcodec](https://github.com/opencadstudio/opencadcodec),
[opencadkernel](https://github.com/opencadstudio/opencadkernel)) are MPL-2.0.

## Features

- DWG and DXF (AutoCAD 2000 to 2018) with xrefs, images and PDF underlays; saves back with a `.bak`.
- Take-off: lengths, areas and counts; import a BOQ from Excel, compare it with the drawing, export.
- Draw and edit: lines, polylines, arcs, hatches, text, dimensions, tables, blocks, layers, groups,
  parametric constraints; undo for everything.
- Plot to PDF or a printer with CTB/STB plot styles.
- Command scripts in the window, or headless with `qiyas-cli` (convert, run, take-off, batch).
- Mizan, the AI assistant (Claude or any OpenAI-compatible service), and an MCP server for Claude
  Desktop and Claude Code.
- Autosave and crash recovery, automatic updates, English and Arabic.

## Screenshots

Coming with the next release.

## Install

Qiyas is still in development and has no published release yet. The first one will appear on
[Releases](https://github.com/qs-mind/qiyas/releases) as AppImage, deb or rpm on Linux, `setup.exe`
or MSI on Windows and a `.dmg` on macOS (Apple silicon or Intel), built by the release workflow;
installed copies will check that page for updates (About, Check for updates). Until then, build it
yourself (below). The installers will not be code-signed at first, so Windows SmartScreen and macOS
Gatekeeper warn on first open; on a Mac, if Qiyas is reported as "damaged", open it once from
System Settings, Privacy & Security, Open Anyway (or run
`xattr -dr com.apple.quarantine /Applications/Qiyas.app`).

## Build

```bash
pnpm install
scripts/pdfium.sh                                             # PDF support (once per machine)
scripts/check.sh                                              # fmt, clippy, tests, tsc, vitest
pnpm tauri dev --config crates/qiyas-desktop/tauri.conf.json  # run the app
pnpm bundle                                                   # installers in target/release/bundle
```

Needs Rust stable, Node 24 and pnpm. The window's Arabic font is Thmanyah Sans, a licensed
font that is not in the repo: `scripts/fonts.sh` copies it from the company's copy, and
without it the system's Arabic font is used. Drawing text uses Noto Sans and Noto Sans Arabic
(OFL, included).

`CLAUDE.md` describes every part of the app and how we work here; `docs/` holds the specs,
plans, check reports and the decisions carried forward.

## Contribute

Issues and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md) and the
[code of conduct](CODE_OF_CONDUCT.md). Report security problems privately
([SECURITY.md](SECURITY.md)). Changes are listed in [CHANGELOG.md](CHANGELOG.md).
