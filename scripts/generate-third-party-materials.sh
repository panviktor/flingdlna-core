#!/usr/bin/env bash

set -euo pipefail

command -v cargo >/dev/null || {
  echo "error: cargo is required" >&2
  exit 1
}

command -v cargo-about >/dev/null || {
  echo "error: cargo-about is required; run: cargo install cargo-about --locked --features cli" >&2
  exit 1
}
command -v cargo-cyclonedx >/dev/null || {
  echo "error: cargo-cyclonedx is required; run: cargo install cargo-cyclonedx --locked" >&2
  exit 1
}

export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"

cargo about generate --frozen --workspace --output-file THIRD_PARTY_NOTICES.md about.hbs
# cargo-about preserves the template's final separator; keep the tracked Markdown
# clean and stable for `git diff --check`.
perl -0pi -e 's/\n{2,}\z/\n/' THIRD_PARTY_NOTICES.md
cargo cyclonedx --all --target all --format json --spec-version 1.5 \
  --override-filename SBOM
echo "Review THIRD_PARTY_NOTICES.md and SBOM.json before publishing a binary release."
