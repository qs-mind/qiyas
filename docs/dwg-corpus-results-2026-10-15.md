# DWG corpus results — 15 October 2026

All 11 inputs open. All six DWG version fixtures and the public DXF retain 211 entities, with no skipped records or reader warnings.

| Input | Bytes | Opens | Open ms | Ready ms | Peak RSS MiB | Warnings |
| --- | ---: | :---: | ---: | ---: | ---: | --- |
| example_2000.dwg | 582572 | Yes | 8.03 | 13.86 | 18.23 | 0 |
| example_2004.dwg | 187890 | Yes | 5.61 | 11.15 | 19.19 | 0 |
| example_2007.dwg | 456000 | Yes | 5.57 | 8.84 | 17.91 | 0 |
| example_2010.dwg | 450071 | Yes | 5.94 | 9.12 | 17.32 | 0 |
| example_2013.dwg | 146906 | Yes | 4.89 | 8.30 | 17.24 | 0 |
| example_2018.dwg | 149218 | Yes | 5.34 | 10.95 | 16.83 | 0 |
| example_2018.dxf | 845818 | Yes | 15.86 | 19.50 | 16.84 | 0 |
| host.dxf | 47984 | Yes | 1.78 | 1.80 | 12.38 | Missing xref (expected) |
| missing-image.dxf | 50930 | Yes | 1.79 | 2.13 | 14.09 | Missing image (expected) |
| private-1 | 1609797 | Yes | 32.51 | 301.11 | 119.30 | 0 |
| private-2 | 2646006 | Yes | 91.62 | 1303.78 | 440.96 | 0 |

The 2,646,006-byte private drawing reaches its first CPU batch in 1.304 seconds, below the five-second limit. Its reader reports 60,449 entities with no skipped records or recovered errors. Private names, paths and warning text are absent from the JSON.

Validation: `cargo test --offline -p qiyas-core --test open --test xref --test image`: 31 passed, zero failures. Both examples build with Rust 1.97.0. Rustfmt and Python syntax checks pass. An invalid drawing returns a readable error. A changed fixture fails the SHA-256 check.

The two missing-reference cases are generated DXF hosts, not customer files. The tests also cover image save/reopen in DWG and DXF, and preservation of referenced drawings.

Method: Linux, workspace dev profile, optimised codec/geometry/render dependencies. One process per input. Ready time includes Session::open and first CPU render batch. RSS is measured per probe process. These measurements do not certify an installed desktop build, GPU appearance or AutoCAD interoperability. No dependency revision changes, push or release.

Source, reproduction command and limitations: [dwg-corpus.md](dwg-corpus.md). Raw data: [dwg-corpus-results-2026-10-15.json](dwg-corpus-results-2026-10-15.json).
