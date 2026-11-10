# scripts/

All scripts here are **CI-only helpers**. Running build scripts locally violates the
project rule in `CONTRIBUTING.md`.

| Script | Purpose | Runs on |
|---|---|---|
| `trigger-ci-build.sh` | Local "build" entry point: dispatches GitHub Actions and prints the run URL | developer machine |
| `build-apple.sh` | Builds iOS/macOS Rust slices for the XCFramework | macOS CI runner |
| `stage-rust-libs.sh` | Stages Rust platform libraries into the Flutter app before platform builds | CI runners |
| `prepare-app.mjs` | One CI entry point: `flutter create` for the requested platforms, then applies the app id and paints the icons | CI runners |
| `apply-app-id.mjs` | Writes `APP_ID` into the generated platform scaffolding (Android, iOS, macOS, Linux, Windows) | CI runners |
| `generate-icons.mjs` | Rasterizes `assets/icon/BatchHash-icon.svg` into every platform's icon set | CI runners |
| `lib/svg-icon.mjs` | The rasterizer: PNG encoder plus the SVG subset the icon uses | imported by `generate-icons.mjs` |
| `app-id.mjs` | Single source of truth for the application identifier and app name | imported |
| `verify-versions.mjs` | Version / tag / CHANGELOG / `Cargo.lock` consistency gate | CI runners |
| `patch-android-signing.mjs` | Adds the release signing config that reads `android/key.properties` | CI runners, only when the signing secrets exist |

## Application identity and icons

The platform directories (`app/android`, `app/ios`, …) are **not committed**; every CI
job generates them with `flutter create`. That means the application identifier and the
icon cannot live in the repository as platform files — they have to be applied right
after scaffolding. `prepare-app.mjs` is the only thing jobs should call:

```bash
cd app
node ../scripts/prepare-app.mjs android,linux
```

It runs `flutter create`, `flutter pub get`, then for each platform
`apply-app-id.mjs` and `generate-icons.mjs`.

**Identifier.** `io.github.seekdeeply.batchhash`, defined once in `app-id.mjs` and written
to the Android `applicationId`/`namespace` and Kotlin package, the iOS and macOS
`PRODUCT_BUNDLE_IDENTIFIER`, and the Linux GTK `APPLICATION_ID`. Each script **fails**
when the Flutter template shape it expects is missing, so a template change breaks CI
instead of silently shipping `com.example.batchhash`.

**Icons.** `assets/icon/BatchHash-icon.svg` is the only icon source. There is no
dependency on ImageMagick, librsvg or any native module: `lib/svg-icon.mjs` implements
the SVG subset the artwork uses (linear gradients, cubic/quadratic Béziers, polygons,
circles, rounded rects, strokes, group opacity, translate and one rounded-rect clip) and
writes PNG and ICO itself. Two variants are produced:

- **rounded, transparent corners** — Android legacy `ic_launcher`, Windows `.ico`, Linux
  hicolor PNGs;
- **opaque, full-bleed** — iOS and macOS `AppIcon` (both reject an alpha channel and
  apply their own mask), plus the Android adaptive foreground/background layers with the
  artwork inset into the launcher safe zone.

If the icon is redesigned, keep the SVG within that subset or extend the rasterizer —
an unsupported construct is skipped, so the loss is visible rather than silent.
