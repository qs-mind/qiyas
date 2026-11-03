# Parser fuzzing and mutation testing

`fuzz/` is a separate Cargo workspace, excluded from the app workspace. Normal app
builds do not include the harnesses or the fuzz runtime. Run the commands below
from the repository root, when the machine is free for one Rust job. Do not run
them alongside the shared laptop's agent builds.

## Run one target

Install the tools once (a C++ compiler is needed for libFuzzer):

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --version 0.13.1 --locked
```

Run one worker per core, building with every core. AddressSanitizer, debug
assertions and overflow checks use cargo-fuzz's defaults:

```sh
cargo +nightly fuzz run expression -j "$(nproc)" -- \
  -max_total_time=180 -timeout=15 -rss_limit_mb=2048 \
  -malloc_limit_mb=512 -max_len=16384
```

`-j` sets the fuzz workers (each writes `fuzz-<n>.log`). The
input bounds are checked inside every harness, including artifact replay. The
RSS, allocation and timeout flags limit parser expansion and hangs; input size
alone cannot bound a compressed workbook's expanded contents.

| Target | Production entry point | Input bound |
| --- | --- | --- |
| `plot_style` | `PlotStyle::parse`, pen lookup, serialization and reparse (CTB/STB and text) | 64 KiB |
| `expression` | `Expr::parse`, names, evaluation with/without variables, display and reparse | 16 KiB |
| `cmdline` | `cmdline::Machine`, up to 128 newline-separated steps, cancellation and arbitrary last coordinates | 16 KiB |
| `raw_input` | `Session::validate_input` and `Session::run_input` on a fresh blank drawing | 16 KiB |
| `records_where` | `Vec<Where>` decoding and `Session::records` over an entity, layers and header | 16 KiB |
| `dxf` | `Drawing::open` on a temporary `.dxf`, including opencadcodec's failsafe options and paper restoration | 256 KiB |
| `excel_boq` | `qiyas_boq::import_boq`, including calamine and BOQ header/number parsing; `.xls` for OLE bytes, `.xlsx` otherwise | 256 KiB |
| `xdata_json` | `decode_legacy_unicode`, JSON decoding, UTF-8 chunking and `knowledge::get` on an XRecord envelope | 16 KiB |
| `mcp_request` | `rmcp::model::ClientJsonRpcMessage` decoding, serialization and reparse | 16 KiB |

Use `-max_len=65536` for plot styles and `-max_len=262144` for DXF/Excel. Invalid
inputs and ordinary parser errors are expected; panics, sanitizer failures,
timeouts and excess memory are findings. No panic is swallowed by the harnesses.

The raw command target validates every decoded JSON value. Invalid commands go
through `run_input`'s rejection path. Valid execution is limited to lines, circles,
text, records and batches containing only those commands, so mutated commands
cannot deliberately open or export arbitrary paths. All other valid commands
stop at validation. Each iteration has a fresh drawing in a temporary directory;
the source alternates between User, Ai and Mcp. This is validation coverage, not
coverage of every command's execution. The MCP target uses the same rmcp wire
types as Qiyas's HTTP and stdio transports without starting either transport.

## Seeds and crashes

Small seeds live in `corpus/<target>/`. Binary copies are `office.ctb` and
`named.stb` from `crates/qiyas-plot/tests/fixtures/`, and `test-boq.xlsx` from
`samples/`. Plain plot-style seeds are the decompressed text from those CTB/STB
fixtures, with the CTB reduced to its first three pens, so mutations reach the
grammar without first repairing zlib checksums.

Text seeds are extracted or adapted from existing tests:

- `expression`: arithmetic, variables and invalid syntax from `crates/qiyas-param/tests/expr.rs`.
- `cmdline`: prompted line/polyline input and Arabic coordinates from `crates/qiyas-core/tests/cmdline.rs`.
- `raw_input`: valid and invalid commands/batches from `crates/qiyas-core/tests/validation.rs`.
- `records_where`: layer conditions from `crates/qiyas-core/tests/records.rs`, with each `WhereOp` represented.
- `dxf`: minimal ASCII DXF encoding of the line in `crates/qiyas-core/src/fixtures.rs::sample_doc`, plus the junk input from `tests/open.rs`; the fixture is generated in Rust, so there is no checked-in DXF to copy.
- `xdata_json`: legacy Arabic, surrogate pairs and escaped-backslash cases from `crates/qiyas-core/src/knowledge.rs` tests, including the stored envelope.
- `mcp_request`: ping and initialize from `crates/qiyas-ai/tests/mcp.rs`, plus its `qiyas_read` call encoded as a wire request.

Keep named seed files small. Generated SHA-1 corpus entries, build outputs,
coverage and crash artifacts are ignored. No customer drawings or large binaries
belong here. The separate lockfile is generated locally; CI uploads its resolved
copy with the toolchain version and crash logs for reproduction.

Reproduce a saved crash with the same input limits:

```sh
cargo +nightly fuzz run expression \
  fuzz/artifacts/expression/crash-<hash> -- \
  -runs=1 -timeout=15 -rss_limit_mb=2048 -malloc_limit_mb=512
```

Replace `<hash>` with the artifact's actual name. Turn a confirmed finding into a
small regression test in the owning crate; retain a minimized seed here.

`.github/workflows/fuzz.yml` runs nightly at 02:23 UTC and on manual dispatch. It
builds once, runs all nine targets sequentially for three minutes each, continues
after a target fails, and fails the job if any target failed. The upload step runs
even after failure and retains crashes, timeout/OOM artifacts, logs, the nightly
version and resolved dependencies for 14 days.

## First mutation campaign

The configuration in `.cargo/mutants.toml` mutates only
`crates/qiyas-param/src/expr.rs` and tests only `qiyas-param`, including its unit and
integration tests. It keeps the normal baseline run.
Use cargo-mutants' temporary copy of the source tree; never pass `--in-place`.
Run it with every core:

```sh
cargo install cargo-mutants --locked
cargo mutants -p qiyas-param -j "$(nproc)"
```

`-j` runs that many mutation experiments at once. Results go to the ignored `mutants.out/`. Inspect surviving mutants for
missing assertions or equivalent changes; timeouts are inconclusive. This setup
does not run the campaign. Configuration keys follow the
[cargo-mutants config reference](https://mutants.rs/config-file.html).

## Development tooling licences

These tools are not shipped in Qiyas. Existing runtime dependencies in this
workspace reuse the production libraries; no application dependency was added.
The generated `LICENSES/THIRD-PARTY.md` remains the app's dependency inventory.

| Tool | Version | Licence |
| --- | --- | --- |
| `cargo-fuzz` | 0.13.1 | MIT OR Apache-2.0 |
| `libfuzzer-sys` | 0.4.13 | (MIT OR Apache-2.0) AND NCSA |
| `arbitrary` | 1.4.2 | MIT OR Apache-2.0 |
| `cargo-mutants` | installed separately | MIT |

`libfuzzer-sys`'s Rust wrapper is MIT/Apache-2.0; its bundled libFuzzer C++ runtime
is NCSA, as declared in the crate's manifest and
[upstream licence note](https://github.com/rust-fuzz/libfuzzer/blob/0.4.13/README.md#license).
This detail applies to the explicitly requested development tool only, not the
application dependency allowlist.
