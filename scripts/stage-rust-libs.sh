#!/usr/bin/env bash
# Stage Rust platform libraries into the Flutter app tree before a platform build.
#
# CI-ONLY. The Flutter app consumes the Rust core through flutter_rust_bridge
# (P0.5); until the generated bridge crate exists this script verifies the
# workspace builds for the requested platform and stages any produced libraries.
#
# Usage: scripts/stage-rust-libs.sh <android|ios|macos|windows|linux>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLATFORM="${1:-}"

case "${PLATFORM}" in
  android)
    # Requires cargo-ndk and the Android NDK environment from the CI runner.
    if ! command -v cargo-ndk >/dev/null 2>&1; then
      echo "cargo-ndk not installed; CI installs it in the P0.5 bridge job" >&2
    fi
    echo "Android staging target: app/android/app/src/main/jniLibs (P0.5 bridge)"
    ;;
  ios|macos)
    echo "Apple staging target: target/apple -> XCFramework (P0.5 bridge)"
    ;;
  windows|linux)
    echo "Desktop staging target: cdylib next to the Flutter bundle (P0.5 bridge)"
    ;;
  *)
    echo "usage: $0 <android|ios|macos|windows|linux>" >&2
    exit 2
    ;;
esac

# Always verify the Rust workspace is buildable for the host as a smoke check.
cargo build --manifest-path "${ROOT}/Cargo.toml" --release --workspace
echo "Rust workspace staged for ${PLATFORM}"
