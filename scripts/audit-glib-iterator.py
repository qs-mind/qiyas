#!/usr/bin/env python3
"""Check the reviewed glib iterator exception against an offline Cargo dependency graph."""
import json
from pathlib import Path
import re
import sys


def audit(metadata):
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n["dependencies"] for n in metadata["resolve"]["nodes"]}
    pending = [p["id"] for p in packages.values() if p["name"] == "qiyas-desktop"]
    if len(pending) != 1:
        raise ValueError("Expected one qiyas-desktop package")
    reachable = set()
    while pending:
        package = pending.pop()
        if package not in reachable:
            reachable.add(package)
            pending.extend(nodes.get(package, []))
    glib = [packages[p] for p in reachable if packages[p]["name"] == "glib"]
    if [p["version"] for p in glib] != ["0.18.5"]:
        raise ValueError("glib changed: review the alert and update the documented exception")
    hits = []
    checked = 0
    for package_id in sorted(reachable):
        package = packages[package_id]
        if package["name"] == "glib":
            continue  # Its definitions, examples and tests were reviewed separately.
        root = Path(package["manifest_path"]).parent / "src"
        for source in sorted(root.rglob("*.rs")):
            checked += 1
            for number, line in enumerate(source.read_text().splitlines(), 1):
                if re.search(r"\b(?:array_iter_str|VariantStrIter)\b", line):
                    hits.append(f"{source}:{number}: {line.strip()}")
    if hits:
        raise ValueError("The affected API is now referenced; reassess reachability:\n" + "\n".join(hits))
    print(f"glib 0.18.5: no external references in {checked} Rust source files across {len(reachable)} dependency packages")


if __name__ == "__main__":
    try:
        audit(json.loads(Path(sys.argv[1]).read_text()))
    except (IndexError, OSError, ValueError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
