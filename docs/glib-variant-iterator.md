# glib VariantStrIter alert — 25 October 2026

The medium Dependabot alert remains open. The lockfile still contains glib 0.18.5;
this work does not claim to patch the dependency.

Tauri 2.12.1 uses GTK 0.18 on Linux. GTK 0.18.2, Gio 0.18.4, Pango 0.18.3 and the
WebKit stack require the glib 0.18 line. Forcing a later incompatible glib version
would leave the affected copy in that stack or break its types. The offline cache
contains no replacement on the 0.18 line. We have not claimed that a network
dependency update was tested.

The affected Rust iterator is constructed by `Variant::array_iter_str`. Its private
constructor cannot be called by Qiyas. In the resolved sources, the only calls are
glib's own documentation and tests. There are no references in Qiyas, Tauri, GTK,
Gio, WebKit, Wry, Tao or their resolved Rust dependencies. The iterator is a Rust
API; it is not exposed to JavaScript through Qiyas IPC. Reading a drawing or an
assistant answer does not select this iterator. This is a reachability assessment
of the current dependency graph, not a general assertion that glib is sound.

Keep the alert open until an upstream-compatible dependency update removes it.
At each Tauri/GTK dependency update, review the alert and rerun:

```sh
CARGO_TARGET_DIR="$PWD/target" cargo tree --offline -p qiyas-desktop -i glib
CARGO_TARGET_DIR="$PWD/target" cargo metadata --offline --format-version 1 > /tmp/qiyas-dependencies.json
python3 scripts/audit-glib-iterator.py /tmp/qiyas-dependencies.json
```

The guard fails if the reviewed glib version changes or another dependency refers
to the affected API. It deliberately includes unused feature sources and test
modules within dependency `src` folders, so a hit requires review. Also inspect
glib's own `src/variant.rs` and `src/variant_iter.rs` on an update. Do not suppress
the alert or introduce `array_iter_str` in app code to make a build pass.
