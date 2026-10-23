#!/usr/bin/env bash
# Copies the licensed Thmanyah fonts into the git-ignored src/fonts/thmanyah/.
# Never commit these files.
set -euo pipefail
cd "$(dirname "$0")/.."

# On GitHub (release builds) the fonts come from the sealed copy in the repo, unlocked with the
# THMANYAH_FONTS_KEY secret; see scripts/fonts-seal.mjs.
if [[ -n "${THMANYAH_FONTS_KEY:-}" ]]; then
  exec node scripts/fonts-seal.mjs unseal
fi

src="$HOME/Downloads/Thmanyah-Font-Family/thmanyah typeface"
dest="src/fonts/thmanyah"
mkdir -p "$dest"
find "$src" -type f \( -iname '*.ttf' -o -iname '*.otf' \) -exec cp {} "$dest"/ \;
echo "Copied $(find "$dest" -type f \( -iname '*.ttf' -o -iname '*.otf' \) | wc -l) font files to $dest"
