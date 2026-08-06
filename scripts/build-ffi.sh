#!/usr/bin/env bash
# Build a universal macOS FFI library without modifying another repository.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
core_dir="$(cd "$script_dir/.." && pwd)"
output_dir="${FLINGDLNA_FFI_OUTPUT_DIR:-$core_dir/target/universal-release}"
target_dir="${CARGO_TARGET_DIR:-$core_dir/target}"
profile="${FLINGDLNA_FFI_PROFILE:-release}"
features="${FLINGDLNA_FEATURES:-}"

command -v cargo >/dev/null || {
  echo "error: cargo is required; install Rust with rustup" >&2
  exit 1
}
command -v cbindgen >/dev/null || {
  echo "error: cbindgen is required; run: cargo install cbindgen --locked" >&2
  exit 1
}
command -v lipo >/dev/null || {
  echo "error: lipo is required; install Xcode Command Line Tools" >&2
  exit 1
}

case "$profile" in
  debug) cargo_profile_args=() ;;
  release) cargo_profile_args=(--release) ;;
  *) echo "error: FLINGDLNA_FFI_PROFILE must be debug or release" >&2; exit 1 ;;
esac

feature_args=()
if [[ -n "$features" ]]; then
  feature_args=(--features "$features")
fi

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"
export FLINGDLNA_BUILD_TIME="${FLINGDLNA_BUILD_TIME:-$(date -u +"%Y-%m-%dT%H:%M:%SZ")}"

cd "$core_dir"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build -p flingdlna-ffi --target "$target" "${cargo_profile_args[@]}" "${feature_args[@]}"
done

library_name="libflingdlna_ffi.a"
mkdir -p "$output_dir/lib" "$output_dir/include"
lipo -create \
  "$target_dir/aarch64-apple-darwin/$profile/$library_name" \
  "$target_dir/x86_64-apple-darwin/$profile/$library_name" \
  -output "$output_dir/lib/$library_name"
cbindgen --config "$core_dir/cbindgen.toml" --crate flingdlna-ffi \
  --output "$output_dir/include/flingdlna.h"

lipo -info "$output_dir/lib/$library_name"
echo "FFI output: $output_dir"
