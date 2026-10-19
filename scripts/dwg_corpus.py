#!/usr/bin/env python3
"""Fetch public version fixtures to an external cache and probe each in a fresh process.

Build first: cargo build -p qiyas-core --example corpus_open --example corpus_fixtures
Run: python3 scripts/dwg_corpus.py --cache <outside-repo> --output <report.json>
Optional --local <drawing> adds a private file without recording its path or name.
Public fixture bytes are not distributed by Qiyas. See docs/dwg-corpus.md.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import urllib.request

REVISION = "f860b0fb76d5f8effb5a70769a3389cb1e0308c8"
ROOT = Path(__file__).resolve().parents[1]
VERSIONS = {2000: b"AC1015", 2004: b"AC1018", 2007: b"AC1021",
            2010: b"AC1024", 2013: b"AC1027", 2018: b"AC1032"}
HASHES = {
    "example_2000.dwg": "34574244d7556d1ef7b437443d9b3d1ad8662e1c669c42d80cff6a8a19799be9",
    "example_2004.dwg": "e72d5e86d5d36d64b08822fb25a46079f592fd895a6157b1b8d9b07775e06108",
    "example_2007.dwg": "fe2d27229f67ae801b64cc01b50d4f7cdf53770de3e23640be9c24a14de5bc12",
    "example_2010.dwg": "457fb3d06cacd16dd945b7342651bebdefde03d7eca0d1bd5743c6730e436634",
    "example_2013.dwg": "ce80953e4881899cde8c42b97919feb2697259554ca54dc4005b19068a12c18f",
    "example_2018.dwg": "7bc7721224003062b237845842b7a976e46771c37eba6833b9ed3712e7a5a65a",
    "example_2018.dxf": "e64b0cfb6e1587f9d2bb4387d59064efa5bc667cda9ecbc00313459b304bb997",
}


def probe(binary, path):
    # A fresh Python parent measures only its one child, not a cumulative RSS maximum.
    wrapper = """
import json, resource, subprocess, sys
run = subprocess.run(sys.argv[1:], capture_output=True, text=True)
result = json.loads(run.stdout.strip().splitlines()[-1])
result['peak_rss_kib'] = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
result['exit_code'] = run.returncode
print(json.dumps(result))
sys.exit(run.returncode)
"""
    run = subprocess.Popen([sys.executable, "-c", wrapper, str(binary), str(path)],
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           text=True, start_new_session=True)
    try:
        stdout, _ = run.communicate(timeout=60)
        return json.loads(stdout.strip().splitlines()[-1])
    except subprocess.TimeoutExpired:
        os.killpg(run.pid, signal.SIGKILL)
        run.communicate()
        return {"opened": False, "error": "Probe exceeded 60 seconds"}
    except (ValueError, IndexError):
        return {"opened": False, "error": "Probe failed"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/examples/corpus_open")
    parser.add_argument("--local", type=Path, action="append", default=[])
    args = parser.parse_args()
    cache = args.cache.resolve()
    if cache == ROOT or ROOT in cache.parents:
        parser.error("The fixture cache must be outside the repository")
    cache.mkdir(parents=True, exist_ok=True)
    results = []
    fixtures = [(f"example_{version}.dwg", signature) for version, signature in VERSIONS.items()]
    fixtures.append(("example_2018.dxf", None))
    for name, signature in fixtures:
        url = f"https://raw.githubusercontent.com/LibreDWG/libredwg/{REVISION}/test/test-data/{name}"
        path = cache / name
        if not path.exists():
            with urllib.request.urlopen(url, timeout=30) as response:
                path.write_bytes(response.read())
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != HASHES[name]:
            raise ValueError(f"Fixture checksum mismatch: {name}")
        if signature and data[:6] != signature:
            raise ValueError(f"Wrong DWG version: {name}")
        row = {"fixture": name, "source": url, "bytes": len(data),
               "sha256": digest}
        row.update(probe(args.binary.resolve(), path))
        report = row.get("report", {})
        row["content_checks"] = (
            report.get("entities") == 211 and report.get("skipped") == 0
            and report.get("recovered_errors") == 0 and not report.get("dropped")
            and row.get("ranges") == 194 and row.get("layers") == 5
            and row.get("sheets") == 3 and not row.get("warnings")
            and (signature is None or row.get("segments") == 22672)
        )
        results.append(row)
        print(name, row.get("opened"), round(row.get("ready_ms", 0)), flush=True)
    with tempfile.TemporaryDirectory(prefix="references-", dir=cache) as work:
        fixtures = Path(work) / "drawings"
        subprocess.run([str(args.binary.resolve().with_name("corpus_fixtures")), str(fixtures)],
                       check=True, capture_output=True, timeout=60)
        for name, warning in [("host.dxf", "Xref plan-arch not found"),
                              ("missing-image.dxf", "Image pic.png not found")]:
            path = fixtures / name
            row = {"fixture": name, "source": "Qiyas generated missing-reference fixture",
                   "bytes": path.stat().st_size}
            row.update(probe(args.binary.resolve(), path))
            row["expected_warning"] = any(warning in text for text in row.get("warnings", []))
            results.append(row)
    for index, path in enumerate(args.local):
        # No private filename, absolute path, or diagnostics that can contain either.
        row = {"fixture": f"private-{index + 1}", "bytes": path.stat().st_size}
        measured = probe(args.binary.resolve(), path.resolve())
        for key in ("opened", "open_ms", "ready_ms", "peak_rss_kib", "report", "segments", "ranges"):
            if key in measured:
                row[key] = measured[key]
        row["warning_count"] = len(measured.get("warnings", []))
        row["under_5_seconds"] = measured.get("ready_ms", float("inf")) < 5000
        results.append(row)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for row in results:
        if "warnings" in row:
            row["warnings"] = [text.replace(str(cache), "<corpus-cache>") for text in row["warnings"]]
    args.output.write_text(json.dumps({
        "source_revision": REVISION,
        "measurement": {"measured_at_utc": datetime.now(timezone.utc).isoformat(),
                        "platform": sys.platform,
                        "scope": "Session::open plus first CPU render batch",
                        "rss": "fresh parent getrusage(RUSAGE_CHILDREN), KiB on Linux"},
        "results": results}, indent=2) + "\n")
    return 0 if all(row.get("opened") and row.get("content_checks", True)
                    and row.get("expected_warning", True)
                    and row.get("under_5_seconds", True) for row in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
