These are the drawing and opening batch inputs from the founder's 24 October 2026
villa conversation, in their original order. JSON-string arguments are expanded
for review. The fixture contains no replies, model prose, credentials or rates.

The site rectangles, grid and layers reproduce the desktop's handle allocation.
The test starts with `Session::new_blank(..., 6)` and applies the name-only
`POMI (RICS)` Settings selection before these inputs, as the desktop did.

The first two opening phases ask for 44 distinct openings. The remaining inputs
are the original smaller retries following the live cancellations. Once the
first phases succeed, these retries must refuse overlaps and report fitting
ranges without changing any drawing record, wall membership or history.
Discovery reads, plans, captures and the later abandoned deletion/undo attempt
are omitted: they do not create the design or request additional openings.

`villa_openings.rs` runs the metre replay in the normal suite. Its ignored
four-unit variant checks the full same design in mm, cm, m and inches. The
release speed guard runs from `scripts/check.sh` against the shared fixed CPU
reference, timing whole surface calls, including validation and drawing checks.
