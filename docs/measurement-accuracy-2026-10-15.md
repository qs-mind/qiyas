# Measurement accuracy — 15 October 2026

Local evidence for [QSM-304](/QSM/issues/QSM-304). **200 comparisons match within an absolute 0.001** after
conversion to m, m² or pieces. All absolute errors in this sample are within that limit; this does not imply a 1% relative pass.
Professional review [QSM-419](/QSM/issues/QSM-419) accepted lengths and areas within the stated scope, with aggregate count evidence required. Code review [QSM-420](/QSM/issues/QSM-420) accepted the change without blocking edits.
This is local arithmetic evidence, not a release or desktop acceptance.

Two private office drawings are identified only as real-a and real-b. Both
have INSUNITS=6 and no reader problems. Their names, layers, source bytes and
raw dumps are excluded. Numbers below show six decimals; the comparison uses
unrounded floating-point values. Reproduce with scripts/accuracy/README.md.
Python: ezdxf 1.4.4. Qiyas base: fee27f5 plus the local changes listed below.

## Scope and limits

Qiyas' read-only MeasureEntity command produced the measured values and full
block chains. Python used ezdxf and separately implemented placement matrices,
curve sampling (2e-5 rad/chord), shoelace areas and even-odd hatch holes.
The private DWGs and test-plan were exported to scratch DXF with Qiyas' reader.
Thus this is independent geometry arithmetic on the decoded data, not independent
DWG decoding. Synthetic source DXFs came directly from ezdxf and also have
hand-derived expected answers. Reader correctness, on-screen selection and a
released installer are separate gates. INSERT count means one reference, not
an aggregate CountBlock result. The existing core tests cover aggregate counts
and hidden layers; the real-drawing aggregate check below now supplies that separate evidence.

The real samples have 27 non-comparable records: Face3D, Point, MultiLeader,
Ole2Frame and Wipeout. All remain visible in the appendix and do not count as
successes. Three evenly spaced candidates per type/block-membership group were
sampled from each real drawing. This is not exhaustive entity coverage.

## Twenty selected measurements

| Drawing | Entity chain | Type | Qiyas | Independent | Status | Absolute difference |
|---|---|---|---|---|---|---|
| real-a | 37D5 | Arc | 0.301593 m | 0.301593 | ok | 0.000000 |
| real-a | 3841 | Circle | 0.180956 m2 | 0.180956 | ok | 0.000000 |
| real-a | 3AA4 | Dimension | 3.364723 m | 3.364723 | ok | 0.000000 |
| real-a | 3AB4>20DE | Ellipse | 0.085682 m | 0.085798 | ok | 0.000116 |
| real-a | 3627 | Hatch | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| real-a | 363D | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| real-a | 362F | Line | 0.240000 m | 0.240000 | ok | 0.000000 |
| real-a | 3628 | LwPolyline | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| real-b | 37D5 | Arc | 0.301593 m | 0.301593 | ok | 0.000000 |
| real-b | 3841 | Circle | 0.180956 m2 | 0.180956 | ok | 0.000000 |
| real-b | 3AA4 | Dimension | 3.364723 m | 3.364723 | ok | 0.000000 |
| real-b | 3AB4>20DE | Ellipse | 0.085682 m | 0.085798 | ok | 0.000116 |
| real-b | 3627 | Hatch | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| real-b | 363D | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| real-b | 362F | Line | 0.240000 m | 0.240000 | ok | 0.000000 |
| real-b | 3628 | LwPolyline | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| test-plan | 47 | Arc | 6.283185 m | 6.283185 | ok | 0.000000 |
| test-plan | 48 | Circle | 7.068583 m2 | 7.068583 | ok | 0.000000 |
| test-plan | 4C | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| test-plan | 44 | LwPolyline | 108.000000 m2 | 108.000000 | ok | 0.000000 |

## Coverage

| Input | Matched | Differences | Not comparable |
|---|---:|---:|---:|
| real-a | 54 | 0 | 12 |
| real-b | 54 | 0 | 15 |
| test-plan | 39 | 0 | 0 |
| hatch | 2 | 0 | 0 |
| nested | 10 | 0 | 0 |
| units_0 | 6 | 0 | 0 |
| units_1 | 6 | 0 | 0 |
| units_2 | 6 | 0 | 0 |
| units_4 | 6 | 0 | 0 |
| units_5 | 6 | 0 | 0 |
| units_6 | 6 | 0 | 0 |
| xr | 2 | 0 | 0 |
| xref | 3 | 0 | 0 |

`nested`: mirrored/rotated three-level blocks, uniform and nonuniform placements.
`hatch`: circular bulge boundary with square hole, and a line/semicircle edge boundary.
`units_*`: rectangle, bulged circle, circle, 3-4-5 line, quarter arc and open bulge.
`xref`: external rectangle and circle, rotation 90°, scale 0.5; `xr` is its source.
`test-plan`: 39 measured instances; area, length and single-reference counts.

## Defect found and fixed

The radius-5 semicircular hatch edge path measured **39.25955865821409 m²**;
Python computed **39.269908167253476 m²** (analytic 25π/2 = 39.269908169872416).
The error, **0.010349509039386 m²**, exceeded tolerance. The prior exact-area
routine only accepted polyline hatch paths and fell back to sampled chords for
edge paths. `qiyas-geom/src/exact.rs` now integrates line, circular and elliptic
edges analytically, retaining signed contributions until the complete ring is
summed. It translates the integral's origin for numerical stability. Island
subtraction, self-crossing handling and spline/nonuniform fallbacks remain in
the existing caller. The post-fix hatch output is in the appendix.

The Python gate also now rejects zero matching rows and nonfinite results,
accepts a minimum-match requirement, and includes DIFF and n/a rows in reports.
Its regression test exercises empty, unsupported, matching, differing, NaN and
insufficient-coverage inputs.

## Validation

- qiyas-geom suite: 52 tests passed; the final exact_hatch target then passed both
  regression tests, including the additional chord/mirrored-scale case.
- qiyas-core `--test measure`: 15 passed after the production change.
- Rebuilt accuracy_dump and reran every input: 200 matches, zero differences.
- Python comparison gate: six subcases passed.
- rustfmt check passed on all three Rust files touched by this work.
- No commit, push or release. Full scripts/check.sh is reserved for the tester.

The new Rust regressions cover circular and elliptic semicircles in both
orientations, origins at 0 and 1e9, and a quarter-circle segment with its chord
under a mirrored 2× scale.

## Complete comparison tables

### real-a

Tolerance: 0.001. Matches: 54. Differences: 0. Not comparable: 12. Required matches: 10.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 37D5 | Arc | 0.301593 m | 0.301593 | ok | 0.000000 |
| 40BB | Arc | 0.020000 m | 0.020000 | ok | 0.000000 |
| 792B | Arc | 0.919207 m | 0.919207 | ok | 0.000000 |
| 36EE>25C | Arc | 1.188934 m | 1.188934 | ok | 0.000000 |
| 40D2>9A9 | Arc | 0.001358 m | 0.001358 | ok | 0.000000 |
| 7A8C>B13 | Arc | 0.000766 m | 0.000766 | ok | 0.000000 |
| 3841 | Circle | 0.180956 m2 | 0.180956 | ok | 0.000000 |
| 4184 | Circle | 0.123201 m2 | 0.123201 | ok | 0.000000 |
| 7ABE | Circle | 0.120866 m2 | 0.120866 | ok | 0.000000 |
| 36EE>259 | Circle | 0.001810 m2 | 0.001810 | ok | 0.000000 |
| 3A6F>1E8A | Circle | 0.065144 m2 | 0.065144 | ok | 0.000000 |
| 7701>2DF>259 | Circle | 0.002234 m2 | 0.002234 | ok | 0.000000 |
| 3AA4 | Dimension | 3.364723 m | 3.364723 | ok | 0.000000 |
| 430E | Dimension | 2.694410 m | 2.694410 | ok | 0.000000 |
| 7D9E | Dimension | 2.880000 m | 2.880000 | ok | 0.000000 |
| 39E7>1EB1 | Dimension | 0.958587 m | 0.958587 | ok | 0.000000 |
| 3A6F>1EB1 | Dimension | 0.958587 m | 0.958587 | ok | 0.000000 |
| 771B>1EB1 | Dimension | 0.958587 m | 0.958587 | ok | 0.000000 |
| 3AB4>20DE | Ellipse | 0.085682 m | 0.085798 | ok | 0.000116 |
| 3EEE>1E21 | Ellipse | 0.009770 m | 0.009893 | ok | 0.000123 |
| 79C1>1D7E | Ellipse | 0.023184 m | 0.023220 | ok | 0.000036 |
| 3C12>1861 | Face3D | — | — | n/a | This entity has no length or area |
| 78AF>1861 | Face3D | — | — | n/a | This entity has no length or area |
| 7AE9>1861 | Face3D | — | — | n/a | This entity has no length or area |
| 3627 | Hatch | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| 3920 | Hatch | 0.057600 m2 | 0.057600 | ok | 0.000000 |
| 7443 | Hatch | 0.414720 m2 | 0.414720 | ok | 0.000000 |
| 39AE>52C | Hatch | 0.035599 m2 | 0.035599 | ok | 0.000000 |
| 3B96>1A5F | Hatch | 0.000027 m2 | 0.000027 | ok | 0.000000 |
| 7994>B3C | Hatch | 0.002056 m2 | 0.002056 | ok | 0.000000 |
| 363D | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 4050 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 7909 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 3987>743 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 3987>7BC | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 76B4>760 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 362F | Line | 0.240000 m | 0.240000 | ok | 0.000000 |
| 3E05 | Line | 1.247473 m | 1.247473 | ok | 0.000000 |
| 7687 | Line | 0.090008 m | 0.090008 | ok | 0.000000 |
| 363D>2D3 | Line | 0.864000 m | 0.864000 | ok | 0.000000 |
| 404B>9E4 | Line | 0.010998 m | 0.010998 | ok | 0.000000 |
| 79C1>1E04 | Line | 0.011875 m | 0.011875 | ok | 0.000000 |
| 3628 | LwPolyline | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| 4015 | LwPolyline | 1.391707 m | 1.391707 | ok | 0.000000 |
| 78B8 | LwPolyline | 0.564561 m | 0.564561 | ok | 0.000000 |
| 363D>2D7 | LwPolyline | 0.005530 m2 | 0.005530 | ok | 0.000000 |
| 40D2>994 | LwPolyline | 0.031414 m | 0.031414 | ok | 0.000000 |
| 7AD6>9B9 | LwPolyline | 0.012631 m | 0.012631 | ok | 0.000000 |
| 3A8D | MultiLeader | — | — | n/a | This entity has no length or area |
| 41B6 | MultiLeader | — | — | n/a | This entity has no length or area |
| 79A5 | MultiLeader | — | — | n/a | This entity has no length or area |
| 8A45 | Point | — | — | n/a | This entity has no length or area |
| 8AB4 | Point | — | — | n/a | This entity has no length or area |
| 8ACE | Point | — | — | n/a | This entity has no length or area |
| 8AB1 | Solid | 0.005400 m2 | 0.005400 | ok | 0.000000 |
| 8AB2 | Solid | 0.005400 m2 | 0.005400 | ok | 0.000000 |
| 8ACB | Solid | 0.005400 m2 | 0.005400 | ok | 0.000000 |
| 3ABB>F37 | Solid | 0.001516 m2 | 0.001516 | ok | 0.000000 |
| 782C>F41 | Solid | 0.001212 m2 | 0.001212 | ok | 0.000000 |
| 7B3F>F65 | Solid | 0.000199 m2 | 0.000199 | ok | 0.000000 |
| 3A85>1D22 | Spline | 0.021026 m | 0.021101 | ok | 0.000075 |
| 40FF>9CA | Spline | 0.044651 m | 0.044715 | ok | 0.000064 |
| 799D>1D56 | Spline | 0.039140 m | 0.039152 | ok | 0.000012 |
| 3AB8>1A61 | Wipeout | 0.001044 m2 | — | n/a | no independent calc for WIPEOUT |
| 3F46>FE1 | Wipeout | 0.340147 m2 | — | n/a | no independent calc for WIPEOUT |
| 79CE>1A49 | Wipeout | 0.304519 m2 | — | n/a | no independent calc for WIPEOUT |

### real-b

Tolerance: 0.001. Matches: 54. Differences: 0. Not comparable: 15. Required matches: 10.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 37D5 | Arc | 0.301593 m | 0.301593 | ok | 0.000000 |
| B35F | Arc | 0.235619 m | 0.235619 | ok | 0.000000 |
| B864 | Arc | 1.008935 m | 1.008935 | ok | 0.000000 |
| 36EE>25C | Arc | 1.188934 m | 1.188934 | ok | 0.000000 |
| FFED>AD0D | Arc | 0.112296 m | 0.112296 | ok | 0.000000 |
| 1199A>AEFA>2384 | Arc | 0.113724 m | 0.113724 | ok | 0.000000 |
| 3841 | Circle | 0.180956 m2 | 0.180956 | ok | 0.000000 |
| 11196 | Circle | 0.053093 m2 | 0.053093 | ok | 0.000000 |
| 128CC | Circle | 0.053093 m2 | 0.053093 | ok | 0.000000 |
| 36EE>259 | Circle | 0.001810 m2 | 0.001810 | ok | 0.000000 |
| 103B4>AA68>2C9 | Circle | 0.001466 m2 | 0.001466 | ok | 0.000000 |
| 11BE5>A806>2C5 | Circle | 0.002774 m2 | 0.002774 | ok | 0.000000 |
| 3AA4 | Dimension | 3.364723 m | 3.364723 | ok | 0.000000 |
| 4314 | Dimension | 6.261456 m | 6.261456 | ok | 0.000000 |
| 7DA6 | Dimension | 2.930214 m | 2.930214 | ok | 0.000000 |
| 39E7>1EB1 | Dimension | 0.958587 m | 0.958587 | ok | 0.000000 |
| 3A6F>1EB1 | Dimension | 0.958587 m | 0.958587 | ok | 0.000000 |
| 771B>1EB1 | Dimension | 0.958587 m | 0.958587 | ok | 0.000000 |
| 3AB4>20DE | Ellipse | 0.085682 m | 0.085798 | ok | 0.000116 |
| 103B4>AE95>1E0E | Ellipse | 0.163935 m | 0.164041 | ok | 0.000107 |
| 11BE5>AB4D>1E26 | Ellipse | 0.009770 m | 0.009893 | ok | 0.000123 |
| 3C12>1861 | Face3D | — | — | n/a | This entity has no length or area |
| 103B4>AA95>18EA | Face3D | — | — | n/a | This entity has no length or area |
| 11BE5>AA94>1973 | Face3D | — | — | n/a | This entity has no length or area |
| 3627 | Hatch | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| BC63 | Hatch | 18.860000 m2 | 18.860000 | ok | 0.000000 |
| 112EC | Hatch | 0.061229 m2 | 0.061229 | ok | 0.000000 |
| 39AE>52C | Hatch | 0.035599 m2 | 0.035599 | ok | 0.000000 |
| 103B4>A925>52C | Hatch | 0.031346 m2 | 0.031346 | ok | 0.000000 |
| 11BE5>A922>52C | Hatch | 0.031346 m2 | 0.031346 | ok | 0.000000 |
| 363D | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 403B | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 78AF | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 3987>743 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 1073F>AC39 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 11BE5>AE4E | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 362F | Line | 0.240000 m | 0.240000 | ok | 0.000000 |
| D573 | Line | 0.320000 m | 0.320000 | ok | 0.000000 |
| 11C4C | Line | 0.950000 m | 0.950000 | ok | 0.000000 |
| 363D>2D3 | Line | 0.864000 m | 0.864000 | ok | 0.000000 |
| 103B4>A8D3>892 | Line | 0.006524 m | 0.006524 | ok | 0.000000 |
| 11BE5>A785>4A5 | Line | 0.017496 m | 0.017496 | ok | 0.000000 |
| 3628 | LwPolyline | 0.322971 m2 | 0.322971 | ok | 0.000000 |
| D2FE | LwPolyline | 0.250000 m2 | 0.250000 | ok | 0.000000 |
| 11E9E | LwPolyline | 15.000000 m | 15.000000 | ok | 0.000000 |
| 363D>2D7 | LwPolyline | 0.005530 m2 | 0.005530 | ok | 0.000000 |
| 103B4>ACF3>E9F | LwPolyline | 0.094909 m | 0.094909 | ok | 0.000000 |
| 11BE5>AC70>9DD | LwPolyline | 0.026083 m | 0.026083 | ok | 0.000000 |
| 3A8D | MultiLeader | — | — | n/a | This entity has no length or area |
| 41B6 | MultiLeader | — | — | n/a | This entity has no length or area |
| 79A5 | MultiLeader | — | — | n/a | This entity has no length or area |
| 8D05>8C99 | Ole2Frame | — | — | n/a | This entity has no length or area |
| C930>8C99 | Ole2Frame | — | — | n/a | This entity has no length or area |
| 10C69>8C99 | Ole2Frame | — | — | n/a | This entity has no length or area |
| 8A45 | Point | — | — | n/a | This entity has no length or area |
| 8AB4 | Point | — | — | n/a | This entity has no length or area |
| 8ACE | Point | — | — | n/a | This entity has no length or area |
| 8AB1 | Solid | 0.005400 m2 | 0.005400 | ok | 0.000000 |
| 8AB2 | Solid | 0.005400 m2 | 0.005400 | ok | 0.000000 |
| 8ACB | Solid | 0.005400 m2 | 0.005400 | ok | 0.000000 |
| 3ABB>F37 | Solid | 0.001516 m2 | 0.001516 | ok | 0.000000 |
| 103B4>ACFA>F41 | Solid | 0.001039 m2 | 0.001039 | ok | 0.000000 |
| 11BE5>ABE7>F65 | Solid | 0.000540 m2 | 0.000540 | ok | 0.000000 |
| 3A85>1D22 | Spline | 0.021026 m | 0.021101 | ok | 0.000075 |
| FFED>A9EB>1A13 | Spline | 0.092860 m | 0.092860 | ok | 0.000001 |
| 1199A>AC70>9C9 | Spline | 0.093724 m | 0.093983 | ok | 0.000259 |
| 3AB8>1A61 | Wipeout | 0.001044 m2 | — | n/a | no independent calc for WIPEOUT |
| 103B4>ACD0>FE1 | Wipeout | 0.306740 m2 | — | n/a | no independent calc for WIPEOUT |
| 11BE5>AAE2>FE1 | Wipeout | 0.306740 m2 | — | n/a | no independent calc for WIPEOUT |

### test-plan

Tolerance: 0.001. Matches: 39. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 47 | Arc | 6.283185 m | 6.283185 | ok | 0.000000 |
| 4C>3B | Arc | 1.413717 m | 1.413717 | ok | 0.000000 |
| 4D>3B | Arc | 1.413717 m | 1.413717 | ok | 0.000000 |
| 4E>3B | Arc | 1.413717 m | 1.413717 | ok | 0.000000 |
| 48 | Circle | 7.068583 m2 | 7.068583 | ok | 0.000000 |
| 4C | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 4D | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 4E | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 4F | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 50 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 51 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 52 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 53 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 54 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 55 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 56 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 57 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 58 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 59 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 45 | Line | 9.000000 m | 9.000000 | ok | 0.000000 |
| 46 | Line | 5.000000 m | 5.000000 | ok | 0.000000 |
| 4C>3A | Line | 0.900000 m | 0.900000 | ok | 0.000000 |
| 4D>3A | Line | 0.900000 m | 0.900000 | ok | 0.000000 |
| 4E>3A | Line | 0.900000 m | 0.900000 | ok | 0.000000 |
| 4F>3F | Line | 1.200000 m | 1.200000 | ok | 0.000000 |
| 50>3F | Line | 1.200000 m | 1.200000 | ok | 0.000000 |
| 51>3F | Line | 1.200000 m | 1.200000 | ok | 0.000000 |
| 52>3F | Line | 1.200000 m | 1.200000 | ok | 0.000000 |
| 53>3F | Line | 1.200000 m | 1.200000 | ok | 0.000000 |
| 44 | LwPolyline | 108.000000 m2 | 108.000000 | ok | 0.000000 |
| 49 | LwPolyline | 58.290000 m2 | 58.290000 | ok | 0.000000 |
| 4A | LwPolyline | 22.090000 m2 | 22.090000 | ok | 0.000000 |
| 4B | LwPolyline | 17.390000 m2 | 17.390000 | ok | 0.000000 |
| 54>43 | LwPolyline | 0.160000 m2 | 0.160000 | ok | 0.000000 |
| 55>43 | LwPolyline | 0.160000 m2 | 0.160000 | ok | 0.000000 |
| 56>43 | LwPolyline | 0.160000 m2 | 0.160000 | ok | 0.000000 |
| 57>43 | LwPolyline | 0.160000 m2 | 0.160000 | ok | 0.000000 |
| 58>43 | LwPolyline | 0.160000 m2 | 0.160000 | ok | 0.000000 |
| 59>43 | LwPolyline | 0.160000 m2 | 0.160000 | ok | 0.000000 |

### hatch

Tolerance: 0.001. Matches: 2. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 2F | Hatch | 70.539816 m2 | 70.539816 | ok | 0.000000 |
| 30 | Hatch | 39.269908 m2 | 39.269908 | ok | 0.000000 |

### nested

Tolerance: 0.001. Matches: 10. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 41>3F>38>33 | Circle | 0.000314 m2 | 0.000314 | ok | 0.000000 |
| 41>3F>3A>33 | Circle | 0.000236 m2 | 0.000236 | ok | 0.000000 |
| 41 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 41>3F | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 41>3F>38 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 41>3F>3A | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 41>3F>38>34 | Line | 0.111803 m | 0.111803 | ok | 0.000000 |
| 41>3F>3A>34 | Line | 0.152069 m | 0.152069 | ok | 0.000000 |
| 41>3F>38>32 | LwPolyline | 0.005000 m2 | 0.005000 | ok | 0.000000 |
| 41>3F>3A>32 | LwPolyline | 0.003750 m2 | 0.003750 | ok | 0.000000 |

### units_0

Tolerance: 0.001. Matches: 6. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 33 | Arc | 0.003142 m | 0.003142 | ok | 0.000000 |
| 31 | Circle | 0.000007 m2 | 0.000007 | ok | 0.000000 |
| 32 | Line | 0.005000 m | 0.005000 | ok | 0.000000 |
| 2F | LwPolyline | 0.000012 m2 | 0.000012 | ok | 0.000000 |
| 30 | LwPolyline | 0.000013 m2 | 0.000013 | ok | 0.000000 |
| 34 | LwPolyline | 0.009142 m | 0.009142 | ok | 0.000000 |

### units_1

Tolerance: 0.001. Matches: 6. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 33 | Arc | 0.079796 m | 0.079796 | ok | 0.000000 |
| 31 | Circle | 0.004560 m2 | 0.004560 | ok | 0.000000 |
| 32 | Line | 0.127000 m | 0.127000 | ok | 0.000000 |
| 2F | LwPolyline | 0.007742 m2 | 0.007742 | ok | 0.000000 |
| 30 | LwPolyline | 0.008107 m2 | 0.008107 | ok | 0.000000 |
| 34 | LwPolyline | 0.232196 m | 0.232196 | ok | 0.000000 |

### units_2

Tolerance: 0.001. Matches: 6. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 33 | Arc | 0.957557 m | 0.957557 | ok | 0.000000 |
| 31 | Circle | 0.656693 m2 | 0.656693 | ok | 0.000000 |
| 32 | Line | 1.524000 m | 1.524000 | ok | 0.000000 |
| 2F | LwPolyline | 1.114836 m2 | 1.114836 | ok | 0.000000 |
| 30 | LwPolyline | 1.167454 m2 | 1.167454 | ok | 0.000000 |
| 34 | LwPolyline | 2.786357 m | 2.786357 | ok | 0.000000 |

### units_4

Tolerance: 0.001. Matches: 6. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 33 | Arc | 0.003142 m | 0.003142 | ok | 0.000000 |
| 31 | Circle | 0.000007 m2 | 0.000007 | ok | 0.000000 |
| 32 | Line | 0.005000 m | 0.005000 | ok | 0.000000 |
| 2F | LwPolyline | 0.000012 m2 | 0.000012 | ok | 0.000000 |
| 30 | LwPolyline | 0.000013 m2 | 0.000013 | ok | 0.000000 |
| 34 | LwPolyline | 0.009142 m | 0.009142 | ok | 0.000000 |

### units_5

Tolerance: 0.001. Matches: 6. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 33 | Arc | 0.031416 m | 0.031416 | ok | 0.000000 |
| 31 | Circle | 0.000707 m2 | 0.000707 | ok | 0.000000 |
| 32 | Line | 0.050000 m | 0.050000 | ok | 0.000000 |
| 2F | LwPolyline | 0.001200 m2 | 0.001200 | ok | 0.000000 |
| 30 | LwPolyline | 0.001257 m2 | 0.001257 | ok | 0.000000 |
| 34 | LwPolyline | 0.091416 m | 0.091416 | ok | 0.000000 |

### units_6

Tolerance: 0.001. Matches: 6. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 33 | Arc | 3.141593 m | 3.141593 | ok | 0.000000 |
| 31 | Circle | 7.068583 m2 | 7.068583 | ok | 0.000000 |
| 32 | Line | 5.000000 m | 5.000000 | ok | 0.000000 |
| 2F | LwPolyline | 12.000000 m2 | 12.000000 | ok | 0.000000 |
| 30 | LwPolyline | 12.566371 m2 | 12.566371 | ok | 0.000000 |
| 34 | LwPolyline | 9.141593 m | 9.141593 | ok | 0.000000 |

### xr

Tolerance: 0.001. Matches: 2. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 30 | Circle | 3.141593 m2 | 3.141593 | ok | 0.000000 |
| 2F | LwPolyline | 40.000000 m2 | 40.000000 | ok | 0.000000 |

### xref

Tolerance: 0.001. Matches: 3. Differences: 0. Not comparable: 0. Required matches: 1.

| Entity chain | Type | Qiyas | Independent | Status | Difference or reason |
|---|---|---|---|---|---|
| 32>30 | Circle | 0.785398 m2 | 0.785398 | ok | 0.000000 |
| 32 | Insert | 1.000000 pcs | 1.000000 | ok | 0.000000 |
| 32>2F | LwPolyline | 10.000000 m2 | 10.000000 | ok | 0.000000 |

## Full-precision relative evaluation (continuation)

Re-extracted the same real drawings and recomputed the reference without rounding.
The numeric artifact `measurement-relative-accuracy-2026-10-15.json` records
Qiyas, independent, absolute error/pass and relative error/pass separately.
Relative error is abs(Qiyas − reference) / abs(reference); the separate limit is
0.01 (1%). INSERT rows are excluded from the length/area denominator.

**94/96 length and area rows pass 1%; two fail; none remain indeterminate.**
All 108 comparable rows (including individual INSERTs) still pass absolute 0.001.
The two failing ellipse arcs are real-a/3EEE>1E21 and real-b/11BE5>AB4D>1E26,
each with relative error approximately 0.01244629075 (1.244629075%).
This is a known relative-precision defect, not an absolute-tolerance failure.

For real-a/3B96>1A5F (hatch): Qiyas = 2.6942564051295346e-05 m²;
reference = 2.6942564147702797e-05 m²; absolute error =
9.640745030092303e-14 m²; relative error = 3.578258170692451e-09.
It passes both gates. Six-decimal rounding had obscured this result.

## Aggregate CountBlock evidence

The professional review condition is now covered on both real drawings.
`check_count.py` chooses the most frequent visible ordinary named model-space
block, then lists **all** matching INSERT handles independently with ezdxf.
It compares the integer total and the complete handle set with CountBlock,
using a fresh Session per scenario. All ten scenarios pass exactly, with
zero extra xref points. Nested contents, dynamic anonymous blocks and paper
space are outside this declared aggregate scope. No source drawing was saved.
Source DXF still shares Qiyas decoding; independent DWG decoding is not claimed.

| Drawing | Scenario | Qiyas total | Independent total | Complete handle set |
|---|---|---:|---:|---|
| real-a | all visible layers | 220 | 220 | exact match |
| real-a | selected layer filter | 212 | 212 | exact match |
| real-a | selected layer hidden + filter | 0 | 0 | exact match |
| real-a | selected layer hidden, all layers | 8 | 8 | exact match |
| real-a | other layer filter | 0 | 0 | exact match |
| real-b | all visible layers | 139 | 139 | exact match |
| real-b | selected layer filter | 133 | 133 | exact match |
| real-b | selected layer hidden + filter | 0 | 0 | exact match |
| real-b | selected layer hidden, all layers | 6 | 6 | exact match |
| real-b | other layer filter | 0 | 0 | exact match |

`measurement-count-accuracy-2026-10-15.json` preserves both complete handle
lists and block/layer SHA-256 identifiers without private names. This is
aggregate evidence, separate from the 200 prior sampled calculations.

Continuation validation: the updated accuracy_dump example built successfully
with the existing Rust 1.97 toolchain, one build job, offline; both Python tests
pass (six original gate subcases plus a full-precision relative-failure case).
Python syntax checks and rustfmt checks pass. Ten count scenarios pass.
The original production code reviews remain [QSM-419](/QSM/issues/QSM-419)
and [QSM-420](/QSM/issues/QSM-420); no additional production code changed here.
Relative ellipse precision remains tracked in [QSM-439](/QSM/issues/QSM-439).
No desktop acceptance, commit, push or release is claimed.
