#!/usr/bin/env bash
# Development data only. The Python helper verifies the publisher's checksums.
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$script_dir/fetch.py" "$@"
