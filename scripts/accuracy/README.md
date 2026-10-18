# Measurement comparison

Use Python 3 with `ezdxf==1.4.4`. Python computes geometry separately from Rust.
The tolerance is an absolute 0.001 metres, square metres, or pieces, after
unit conversion. Counts must agree exactly because they are integers.

Build only when no other Rust build or heavy test is running:

```sh
CARGO_BUILD_JOBS=1 cargo build --locked -p qiyas-core --example accuracy_dump
python scripts/accuracy/test_checker.py
python scripts/accuracy/make_synthetic.py "$PAPERCLIP_SCRATCH_DIR/fixtures"
target/debug/examples/accuracy_dump drawing.dwg 3 "$PAPERCLIP_SCRATCH_DIR/inspection.dxf" > "$PAPERCLIP_SCRATCH_DIR/dump.jsonl"
python scripts/accuracy/check_accuracy.py "$PAPERCLIP_SCRATCH_DIR/inspection.dxf" "$PAPERCLIP_SCRATCH_DIR/dump.jsonl" --min-matches 10 --md report.md
```

The optional third argument exports an inspection DXF to a **new** path.
It never overwrites the input. A DXF exported this way shares Qiyas' DWG
reader: this verifies independent measurement arithmetic on the geometry
Qiyas read. It does **not** verify independent DWG decoding. A source DXF
from a separate CAD program is needed for that stronger claim. Store private
inspection copies and raw dumps in the run scratch directory. Do not commit
them or their file/layer names. Keep anonymous numeric reports only.

Run `accuracy_dump` on each generated DXF, then compare it against the same
DXF using `check_accuracy.py`. The generator also writes hand-derived answers
for units 0/1/2/4/5/6, bulges, nested mirrored and scaled blocks, hatch holes
and circular arc boundaries, and a rotated xref scaled by 0.5. These source
DXFs are generated independently of Qiyas.

The dump samples evenly within groups of entity type and block membership.
It measures model space, follows block chains to depth five, and measures an
INSERT as one piece. This is not a total block-count command. It does not
prove UI picking, paper-space quantities, untested scales, or all drawing
entities. The checker keeps differences and unsupported rows in the report.
Unsupported rows do not establish coverage; no matching rows is a failure.
Use `--min-matches` to enforce the required comparison count per input.

For full precision evidence, add `--json numeric.json` to the checker. It keeps
unrounded Qiyas and reference values, absolute error/pass at `--tol`, and
relative error/pass at 1%. Relative failure does not change the absolute-gate
exit code. Zero reference gives relative 0 only for an exact zero result;
otherwise relative status is unavailable. Do not derive relative error from
the six-decimal Markdown display. Exclude INSERT rows from length/area rates.

Aggregate count evidence:

```sh
python scripts/accuracy/check_count.py drawing.dwg inspection.dxf counts.json
```

This chooses the most frequent visible ordinary named model-space block and
independently lists every matching INSERT handle using ezdxf. It compares the
complete handle list and total with CountBlock for all visible layers, one
layer filter, hidden layer, and a different layer filter. Nested block contents
are outside this top-level counting scope. The Rust example accepts
`QIYAS_COUNT_BLOCK`, `QIYAS_COUNT_LAYER` and `QIYAS_HIDE_LAYER` for this mode.
A fresh Session is used for each case; source drawings are never saved.
Private block/layer names remain in memory; the count artifact contains handles.
