#!/usr/bin/env bash
# Build the Rust core as an iOS/macOS XCFramework.
#
# CI-ONLY: this script is executed on a macOS runner. Never run it locally.
# It is used by the P0.5 bridge work package and by Release builds.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/target/apple"
LIB_NAME="batchhash_core"
CRATE="batchhash-core"

mkdir -p "${OUT}"

build_target() {
  local target="$1"
  echo "==> building ${target}"
  cargo build --manifest-path "${ROOT}/Cargo.toml" --release --target "${target}" -p "${CRATE}"
  local dir="${ROOT}/target/${target}/release"
  mkdir -p "${OUT}/${target}"
  cp "${dir}/lib${LIB_NAME}.a" "${OUT}/${target}/lib${LIB_NAME}.a" 2>/dev/null || true
  cp "${dir}/lib${LIB_NAME}.dylib" "${OUT}/${target}/lib${LIB_NAME}.dylib" 2>/dev/null || true
}

# Device + simulator slices (static libraries are the stable choice for iOS).
build_target aarch64-apple-ios
build_target x86_64-apple-ios
build_target aarch64-apple-ios-sim

# Optional shared macOS slice used by desktop builds in later phases.
if [ "${BUILD_MACOS_SLICE:-0}" = "1" ]; then
  build_target aarch64-apple-darwin
  build_target x86_64-apple-darwin
fi

# Headers module map placeholder: the flutter_rust_bridge codegen step (CI) writes
# the real header; the module map keeps XCFramework importable in the meantime.
cat > "${OUT}/module.modulemap" <<'MAP'
framework module BatchHashCore {
  umbrella header "BatchHashCore.h"
  export *
  module * { export * }
}
MAP

echo "XCFramework staging is ready under ${OUT}"
echo "NOTE: XCFramework assembly runs in the P0.5 bridge job once the generated"
echo "      bridge crate exists; this script stages every slice it can build."
