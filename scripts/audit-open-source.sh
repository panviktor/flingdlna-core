#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail=0
patterns='AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9]{16,}|AIza[0-9A-Za-z_-]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}|-----BEGIN (RSA|EC|OPENSSH|PRIVATE) KEY-----|[0-9]{8,10}:[A-Za-z0-9_-]{30,}'
forbidden_names='(^|/)(\.env(\..*)?|.*\.(p12|p8|mobileprovision|cer|pem|key|der)|GoogleService-Info\.plist|AuthKey_.*\.p8|.*\.xcarchive|.*\.dSYM)(/|$)'

if git grep -I -n -E "$patterns" -- ':!scripts/audit-open-source.sh'; then
  echo "error: known secret pattern found" >&2
  fail=1
fi

if git ls-files | grep -E "$forbidden_names"; then
  echo "error: prohibited credential or build artifact is tracked" >&2
  fail=1
fi

if git grep -I -n -E '^[[:space:]]*DEVELOPMENT_TEAM[[:space:]]*=[[:space:]]*[A-Z0-9]{10};|192\.168\.1\.148' -- ':!scripts/audit-open-source.sh'; then
  echo "error: personal signing or local-network metadata is tracked" >&2
  fail=1
fi

exit "$fail"
