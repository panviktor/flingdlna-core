#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FLINGDLNA_FFI_PROFILE=debug "$script_dir/build-ffi.sh"
