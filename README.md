# FlingDLNA Core

Rust core for FlingDLNA: a DLNA controller and media server, a command-line
client, and C-compatible bindings for the macOS application.

The companion app lives in [`flingdlna-macos`](../flingdlna-macos). It pins this
repository as a Git submodule and builds the FFI library locally; this
repository intentionally contains no prebuilt libraries.

## Features

- DLNA renderer discovery, playback control, queues, subtitles, and Wake-on-LAN
- DLNA media server and file watcher
- Chromecast discovery and playback
- CLI, TUI, and optional Unix-socket daemon
- C FFI for macOS clients

## Prerequisites

Install the pinned Rust toolchain with [rustup](https://rustup.rs/). The
repository's `rust-toolchain.toml` installs `rustfmt`, Clippy, and the macOS
Apple Silicon and Intel targets automatically.

`ffprobe` is optional and is used only for duration probing of AVI, WMV, and
FLV files.

## Build and verify

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
./target/release/flingdlna --help
```

Build a universal macOS FFI library and C header outside the source tree:

```bash
cargo install cbindgen --locked
FLINGDLNA_FFI_OUTPUT_DIR=/tmp/flingdlna-ffi ./scripts/build-ffi.sh
```

The output directory contains `lib/libflingdlna_ffi.a` and
`include/flingdlna.h`. The macOS app performs this step automatically in
DerivedData.

## Security and licensing

- Never commit credentials, certificate material, provisioning profiles, local
  databases, or logs. Run `./scripts/audit-open-source.sh` before every release.
- User-controlled integrations belong in the client application's Keychain or
  local configuration, never in this repository.
- This project's source is [MIT licensed](LICENSE). Dependency licensing is
  checked with `cargo deny`; the tracked
  [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and
  [SBOM.json](SBOM.json) are generated from the locked graph.

## Release checklist

1. Run the quality commands above plus `cargo deny check licenses bans sources`.
2. Regenerate the third-party notice and CycloneDX SBOM with
   `./scripts/generate-third-party-materials.sh`.
3. Run `./scripts/audit-open-source.sh` and inspect the complete output.
4. Tag the core release before advancing the macOS submodule pointer.
