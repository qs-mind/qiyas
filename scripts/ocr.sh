#!/usr/bin/env bash
# Downloads what Qiyas needs to read text in scanned drawings and documents (Arabic and English):
# the ONNX Runtime shared library (MIT, microsoft/onnxruntime) and the PP-OCRv5 text detection
# and Arabic recognition models with their dictionary (Apache-2.0, PaddleOCR, as published by
# GreatV/oar-ocr). They go into crates/qiyas-desktop/resources/ocr/ (git-ignored), the licences
# into LICENSES/. Without them reading a scan says "Text reading is not installed".
# Run it once per machine, and before bundling a release.
#
# Every download must match its SHA-256 in scripts/ocr.sha256, or nothing is installed.
set -euo pipefail
cd "$(dirname "$0")/.."

ORT=1.31.1
MODELS=v0.3.0

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) asset=onnxruntime-linux-x64-$ORT.tgz libs="libonnxruntime.so.$ORT" link=libonnxruntime.so ;;
  Darwin-arm64) asset=onnxruntime-osx-arm64-$ORT.tgz libs="libonnxruntime.$ORT.dylib" link=libonnxruntime.dylib ;;
  MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64)
    asset=onnxruntime-win-x64-$ORT.zip libs="onnxruntime.dll onnxruntime_providers_shared.dll" link= ;;
  *)
    echo "No ONNX Runtime build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

sum() {
  if command -v sha256sum > /dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
fetch() { # release asset url destination
  local expected actual
  expected="$(awk -v r="$1" -v a="$2" '$1 == r && $2 == a { print $3 }' scripts/ocr.sha256)"
  if [[ -z "$expected" ]]; then
    echo "No SHA-256 for $2 of $1 in scripts/ocr.sha256: refusing to install it." >&2
    exit 1
  fi
  echo "Downloading $3"
  curl -fsSL "$3" -o "$4"
  actual="$(sum "$4")"
  if [[ "$actual" != "$expected" ]]; then
    echo "SHA-256 MISMATCH for $2: expected $expected, got $actual. Nothing was installed." >&2
    exit 1
  fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fetch "onnxruntime-$ORT" "$asset" "https://github.com/microsoft/onnxruntime/releases/download/v$ORT/$asset" "$work/$asset"
for m in pp-ocrv5_mobile_det.onnx arabic_pp-ocrv5_mobile_rec.onnx ppocrv5_arabic_dict.txt; do
  fetch "models-$MODELS" "$m" "https://github.com/GreatV/oar-ocr/releases/download/$MODELS/$m" "$work/$m"
done

case "$asset" in
  *.zip) unzip -q "$work/$asset" -d "$work" ;;
  *) tar -xzf "$work/$asset" -C "$work" ;;
esac
src="$work/${asset%.*}"
src="${src%.tar}"

dest=crates/qiyas-desktop/resources/ocr
rm -rf "$dest"
mkdir -p "$dest" LICENSES
for l in $libs; do cp "$src/lib/$l" "$dest/"; done
# The library is loaded by this plain name (ocr::library_name in qiyas-ocr).
if [[ -n "$link" ]]; then mv "$dest/${libs%% *}" "$dest/$link"; fi
cp "$work"/pp-ocrv5_mobile_det.onnx "$work"/arabic_pp-ocrv5_mobile_rec.onnx "$work"/ppocrv5_arabic_dict.txt "$dest/"
cp "$src/LICENSE" LICENSES/onnxruntime-MIT.txt
cp "$src/ThirdPartyNotices.txt" "$dest/onnxruntime-ThirdPartyNotices.txt"
echo "onnxruntime $ORT, models $MODELS" > "$dest/VERSION"
echo "$dest (ONNX Runtime $ORT, PP-OCRv5 models, SHA-256 checked)"
