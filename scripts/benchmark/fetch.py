#!/usr/bin/env python3
"""Fetch only the four reviewed datasets; all outputs stay in .local/benchmark."""

import argparse
import datetime
import hashlib
import json
import re
import sys
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STORE = ROOT / ".local" / "benchmark" / "sources"
CATALOG = json.loads(Path(__file__).with_name("sources.json").read_text())


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def read_url(url):
    request = urllib.request.Request(url, headers={"User-Agent": "Qiyas-benchmark/1"})
    return urllib.request.urlopen(request, timeout=60)


def metadata(url):
    with read_url(url) as response:
        return json.load(response)


def log(entry):
    with (STORE / "provenance.jsonl").open("a") as stream:
        stream.write(json.dumps({"accessed_at": now(), **entry}, sort_keys=True) + "\n")


def fetch_file(source, url, target, expected, algorithm="sha256", git_blob=False):
    """Publish a file only after its upstream digest agrees; log a SHA-256 too."""
    target.parent.mkdir(parents=True, exist_ok=True)
    partial = target.with_name(target.name + ".part")
    try:
        with read_url(url) as response, partial.open("wb") as output:
            while chunk := response.read(1024 * 1024):
                output.write(chunk)
        checksum = hashlib.new(algorithm)
        if git_blob:
            checksum.update(f"blob {partial.stat().st_size}\0".encode())
        sha256 = hashlib.sha256()
        with partial.open("rb") as stream:
            while chunk := stream.read(1024 * 1024):
                checksum.update(chunk)
                sha256.update(chunk)
        if checksum.hexdigest() != expected:
            raise ValueError(f"Checksum mismatch for {url}")
        partial.replace(target)
        log({"source": source["id"], "source_url": source["url"], "url": url,
             "path": str(target.relative_to(STORE)), "license": source["license"],
             "attribution": source["attribution"], "upstream_checksum": f"{algorithm}:{expected}",
             "checksum_kind": "git_blob" if git_blob else "file", "sha256": sha256.hexdigest(),
             "bytes": target.stat().st_size, "status": "verified"})
    finally:
        partial.unlink(missing_ok=True)


def github_blob(source, commit, entry, relative):
    repo = source["repository"]
    path = urllib.parse.quote(entry["path"], safe="/")
    url = f"https://raw.githubusercontent.com/{repo}/{commit}/{path}"
    target = STORE / source["id"] / relative
    fetch_file(source, url, target, entry["sha"], "sha1", git_blob=True)
    if target.stat().st_size < 1024:
        pointer = target.read_bytes()
        match = re.fullmatch(rb"version https://git-lfs.github.com/spec/v1\noid sha256:([0-9a-f]{64})\nsize (\d+)\n?", pointer)
        if match:
            # Keep the pinned pointer beside the material it authenticates.
            target.rename(target.with_name(target.name + ".lfs-pointer"))
            url = f"https://media.githubusercontent.com/media/{repo}/{commit}/{path}"
            fetch_file(source, url, target, match[1].decode())
            if target.stat().st_size != int(match[2]):
                raise ValueError(f"Git LFS size mismatch: {target}")


def github(source, args):
    repo = source["repository"]
    info = metadata(f"https://api.github.com/repos/{repo}")
    commit = metadata(f"https://api.github.com/repos/{repo}/commits/{info['default_branch']}")["sha"]
    tree = metadata(f"https://api.github.com/repos/{repo}/git/trees/{commit}?recursive=1")
    if tree.get("truncated"):
        raise ValueError("Publisher tree is truncated; refusing an incomplete source inventory")
    entries = [item for item in tree["tree"] if item["type"] == "blob"]
    directory = source.get("directory", "")
    count = 0
    for entry in entries:
        path = Path(entry["path"])
        is_root_notice = len(path.parts) == 1 and path.name.lower().startswith(("license", "readme", "citation"))
        if is_root_notice:
            github_blob(source, commit, entry, "notices/" + path.name)
        elif directory and entry["path"].startswith(directory + "/"):
            relative = entry["path"][len(directory) + 1:]
            # Only Clinic and Duplex model/COBie files and their attribution, never neighbouring packs.
            if path.suffix.lower() in (".ifc", ".xlsx") or path.name.lower().startswith(("readme", "license")):
                github_blob(source, commit, entry, relative)
                count += path.suffix.lower() == ".ifc"
        elif source["id"] == "resplan" and path.name.lower() in ("resplan.zip", "resplan.pkl"):
            github_blob(source, commit, entry, path.name)
            count += 1
    if source["id"] == "resplan" and count == 0:
        if not args.resplan_url or not args.resplan_sha256:
            raise ValueError("The research links the ResPlan repository, not a pinned data download. "
                             "Read its saved README; rerun with --resplan-url URL and --resplan-sha256 SHA256 "
                             "from the official release. No unverified pickle is fetched.")
        if not args.resplan_url.startswith("https://") or not re.fullmatch(r"[0-9a-fA-F]{64}", args.resplan_sha256):
            raise ValueError("ResPlan needs an HTTPS official URL and a 64-digit SHA-256")
        fetch_file(source, args.resplan_url, STORE / source["id"] / "ResPlan.zip", args.resplan_sha256.lower())
        count += 1
    if count == 0:
        raise ValueError("No source models found at the reviewed publisher path")
    log({"source": source["id"], "status": "source_complete", "git_commit": commit, "model_files": count})


def swiss(source):
    record = metadata(source["metadata_url"])
    license_id = record.get("metadata", {}).get("license", {}).get("id", "").lower()
    if license_id != "cc-by-4.0":
        raise ValueError(f"Unexpected Zenodo licence: {license_id}")
    (STORE / source["id"] / "zenodo-metadata.json").write_text(json.dumps(record, indent=2) + "\n")
    entry = next(item for item in record["files"] if item["key"] == source["filename"])
    algorithm, checksum = entry["checksum"].split(":", 1)
    if algorithm not in ("md5", "sha256"):
        raise ValueError(f"Unsupported publisher checksum: {algorithm}")
    fetch_file(source, entry["links"]["self"], STORE / source["id"] / source["filename"], checksum, algorithm)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="*", choices=[s["id"] for s in CATALOG["sources"]] + ["all"])
    parser.add_argument("--plan", action="store_true", help="Write provenance/candidate metadata only; no network")
    parser.add_argument("--resplan-url", help="Official data URL from the reviewed ResPlan repository")
    parser.add_argument("--resplan-sha256", help="Verified official release SHA-256; report contains no checksum")
    args = parser.parse_args()
    STORE.mkdir(parents=True, exist_ok=True)
    (STORE / "catalog.json").write_text(json.dumps(CATALOG, indent=2) + "\n")
    (STORE / "restricted-candidates.json").write_text(json.dumps(CATALOG["restricted_candidates"], indent=2) + "\n")
    selected = args.source or ["all"]
    failures = []
    for source in CATALOG["sources"]:
        if "all" not in selected and source["id"] not in selected:
            continue
        directory = STORE / source["id"]
        directory.mkdir(exist_ok=True)
        (directory / "LICENSE.md").write_text(
            f"# {source['name']}\n\nData licence: Creative Commons Attribution 4.0 International.\n"
            "https://creativecommons.org/licenses/by/4.0/legalcode\n\n"
            f"{source['attribution']}\n\nSource: {source['url']}\n\n"
            "Derived DXF/truth must retain attribution and describe the conversion. "
            "The original publisher notices are retained when downloaded.\n")
        if args.plan:
            log({"source": source["id"], "source_url": source["url"], "license": source["license"],
                 "status": "pending_download", "reason": "Offline preparation; no file checksums supplied by research"})
            print(f"{source['id']}: pending — {source['url']}")
            continue
        try:
            swiss(source) if source["id"] == "swiss-dwellings" else github(source, args)
        except Exception as error:
            failures.append(source["id"])
            log({"source": source["id"], "status": "download_failed", "error": str(error)})
            print(f"{source['id']}: {error}", file=sys.stderr)
    return bool(failures)


if __name__ == "__main__":
    sys.exit(main())
