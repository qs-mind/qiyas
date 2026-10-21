# Benchmark source tools

These are optional development tools. Data and the Python environment live only in
`.local/benchmark/`; nothing is installed in `~/.qiyas` or shipped with Qiyas.

The source catalogue follows the research report and its notes. Only Medical-Dental
Clinic, Duplex Apartment, ResPlan and Swiss Dwellings are downloadable. Restricted
tenders and non-commercial datasets are metadata-only candidates; the downloader
has no path for fetching them.

```sh
scripts/benchmark/fetch.sh --plan      # offline: pending provenance and candidates
scripts/benchmark/fetch.sh all         # reviewer runs with network access
python3 -m venv .local/benchmark/.venv
.local/benchmark/.venv/bin/pip install -r scripts/benchmark/requirements.txt
.local/benchmark/.venv/bin/python scripts/benchmark/convert.py swiss-dwellings \
  --input .local/benchmark/sources/swiss-dwellings/geometries.csv --limit 150
.local/benchmark/.venv/bin/python scripts/benchmark/convert.py resplan \
  --input .local/benchmark/sources/resplan/ResPlan.zip --limit 150
# Repeat for each downloaded discipline IFC; one DXF and truth per storey:
.local/benchmark/.venv/bin/python scripts/benchmark/convert.py buildingsmart-duplex \
  --input '.local/benchmark/sources/buildingsmart-duplex/Duplex_A_20110907.ifc'
```

The report does **not** contain checksums or a direct ResPlan data URL. The fetcher
pins the current publisher Git commit and verifies Git blob identities or Git LFS
SHA-256. Swiss uses the checksum in Zenodo record 7070952 and records a SHA-256 too.
Each saved file has a provenance entry, source URL, licence and attribution. A
checksum failure removes the partial download. Git LFS pointers remain beside the
download they authenticate.

If ResPlan's data is external to Git, the script saves the official README and
stops for that source. Pass `--resplan-url` and `--resplan-sha256` only after checking
the official release and obtaining a verified SHA-256. It never invents a digest
or substitutes a mirror. ResPlan pickle loading permits only geometry/array/graph
classes; a format change fails explicitly and must be reviewed. NetworkX is included
in this development venv for its optional room-connectivity graph metadata.

Conversion refuses inputs not verified in the source log, never touches holdout,
and refuses to overwrite cases. Use `--prefix` for a fresh derived set. Swiss CSV
column names are checked; `--group-column` and `--geometry-column` make any schema
mapping explicit. Malformed geometries fail rather than silently altering truth.
No external data could be fetched in the implementation sandbox, so upstream
schema compatibility and quantities still require verification after download.

All output is in metres. Opening and fixture geometry becomes blocks; polygons
retain net areas, including holes (partitioned into closed regions whose positive
areas are checked to sum to the net source area; boundary-only intersections are
excluded from area citations). Wall polygons
produce area truth, not invented wall centreline lengths. IFC sections are cut at
storey elevation +1.2 m, with projected silhouettes for objects outside the cut.
Mesh volume is included only for closed meshes and cites the model GUID; current
2D-only handle remeasurement cannot verify those volume items. Failed IFC objects
are recorded in a conversion audit. Tessellation precision, COBie count comparison
and the research's independent 10% sample review are explicit limitations, recorded
in every IFC truth file, and must be checked before treating these as reviewed gold.

The converter dependencies are development-only: IfcOpenShell (LGPL), Shapely (BSD,
with its GEOS dependency), NetworkX (BSD) and ezdxf (MIT). They are intentionally outside Cargo and
the product. The repository gains no runtime dependency from these tools.

Run the network-free verification tests with
`PYTHONDONTWRITEBYTECODE=1 python3 scripts/benchmark/test_tools.py`.
