# Ellipse arc length: local verification

The two small ellipse arcs now pass the 1% relative error limit.
The previous absolute 0.001 m limit also passes before and after this change.

| Drawing | Entity chain | New length (m) | Relative error (%) |
|---|---|---|---|
| real-a | 3EEE>1E21 | 0.009893335917769587 | 0.000000001708591127 |
| real-b | 11BE5>AB4D>1E26 | 0.009893335918363983 | 0.000000002774779484 |

The previous relative error was 1.244629075%. The JSON file beside this report
keeps full precision results for all six sampled ellipses. All six pass both limits.
The drawing labels match the original report through their complete entity chains.

Partial ellipse lengths previously used display chords. Their absolute chord
error can cause a large relative error on small arcs. The new calculation uses
1024 Simpson intervals to integrate the curve speed in parameter space.
Uniform scale, reflection, rotation and translation keep their existing behavior.
Tilted normals and nonuniform transforms keep the existing fallback.
Full ellipse lengths and ellipse areas keep their existing formulas.

The independent Python calculation uses ezdxf 1.4.4, transformed curve points,
and dense chords with a parameter step of 0.00002 radians.
The read-only accuracy_dump example measures the original private DWGs.
The checker reads new inspection DXFs exported from those drawings.
This proves independent geometry arithmetic on decoded data. It does not prove
independent DWG decoding or desktop acceptance.

The regression tests use a separate reference of 100,000 chords per arc.
They cover small spans, parameter wrap, nearly zero minor axes, opposite normals,
reflections, rotations, translations and scales from 0.001 to 1000.
The relative error assertion is 0.00001 (0.001%). Both new tests pass. All 55 qiyas-geom tests pass.

Validation commands use the installed stable Rust toolchain, offline dependencies,
and CARGO_BUILD_JOBS=1. Builds and tests run one at a time:

- cargo test --offline --locked -p qiyas-geom --test ellipse_length
- cargo build --offline --locked -p qiyas-core --example accuracy_dump
- accuracy_dump on each source DWG, with three samples per group
- check_accuracy.py on each exported DXF and its ellipse measurement rows
- cargo test --offline --locked -p qiyas-geom

The full scripts/check.sh is not run. No commit, push or release is made.
The working tree contains earlier changes, including the hatch boundary fix.
This change adds only the ellipse length branch, its tests and this evidence.
Peer review was accepted on 2026-10-15 by Qiyas DWG Engineer.
The reviewer confirmed all three hashes below and checked 20 independent arc
cases against 100,000 chords. The maximum relative error was 0.000124565%.
The reviewer did not repeat Rust tests. Acceptance covers the local correction
only; desktop acceptance, push and release have separate gates.

Review snapshot SHA-256:

- exact.rs: 18048ab99ed764673a960b936e9f1a8477a5a9355e7a71aabe34788c26cbf7ce
- ellipse_length.rs: 9ac81a82f143c21e4d871e81f7f88a458f97138821d0b29716bbcf8d7769457b
- numeric JSON: 59b3d56f42e523c0e63527685954cbdfb8f9ab5f5a9628dd7cc540eed4fd7f25
