#!/usr/bin/env bash
# Local checks. Run before every commit. Stops at the first failure.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Where scripts/pdfium.sh installed pdfium, its tests must run, never be ignored.
lib=crates/qiyas-desktop/resources/pdfium
if [ -e "$lib/libpdfium.so" ] || [ -e "$lib/libpdfium.dylib" ] || [ -e "$lib/pdfium.dll" ]; then
  pdf=$(cargo test -p qiyas-pdf --test pdf 2>&1) || { echo "$pdf"; exit 1; }
  echo "$pdf" | grep 'test result'
  if echo "$pdf" | grep -Eq 'test result: .* [1-9][0-9]* ignored'; then
    echo "pdfium is installed in $lib but its tests were ignored" >&2
    exit 1
  fi
fi
# Release-only speed guard for the pick index (codegen-units = 1 in Cargo.toml).
cargo test -p qiyas-render --release pick_index_one_million
# Release-only speed guard for fills inside blocks (triangulated once per fill entity).
cargo test -p qiyas-render --release --test fills fill_in_a_block_inserted_500_times
# Release-only speed guard for the constraint solver: 200 constraints in under 50 ms.
cargo test -p qiyas-param --release --test solver_speed
# Release-only speed guard for 500 joined walls, including room holes.
cargo test -p qiyas-geom --release walls_speed
# Release-only end-to-end wall-opening guard against the fixed CPU reference.
cargo test -p qiyas-ai --release --test villa_openings villa_speed -- --ignored --nocapture
# Release-only full clinic guard, including labels, fields, schedules and failed batches.
cargo test -p qiyas-ai --release --test clinic_performance clinic_speed -- --ignored --nocapture
pnpm typecheck
pnpm test
