# scripts/

All scripts here are **CI-only helpers**. Running build scripts locally violates the
project rule in `CONTRIBUTING.md`.

| Script | Purpose | Runs on |
|---|---|---|
| `trigger-ci-build.sh` | Local "build" entry point: dispatches GitHub Actions and prints the run URL | developer machine |
| `build-apple.sh` | Builds iOS/macOS Rust slices for the XCFramework | macOS CI runner |
| `stage-rust-libs.sh` | Stages Rust platform libraries into the Flutter app before platform builds | CI runners |
