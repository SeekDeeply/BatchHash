# 原生库打包（平台侧）

Rust 核心编译产物是 `batchhash-ffi`：

| 平台 | 产物 | 用途 |
|---|---|---|
| Linux | `libbatchhash_ffi.so` | `dart:ffi` 动态加载 |
| Windows | `batchhash_ffi.dll` | `dart:ffi` 动态加载 |
| macOS | `libbatchhash_ffi.dylib` | `dart:ffi` 动态加载 |
| Android | `libbatchhash_ffi.so`（arm64-v8a / x86_64） | 打进 APK 的 `jniLibs` |
| iOS | `libbatchhash_ffi.a`（静态库） | 链接进 Runner，`DynamicLibrary.process()` 查找 |

Dart 侧加载顺序见 [docs/bridge.md](bridge.md)：`BATCHHASH_FFI_LIB` → iOS 进程内 → 平台默认库名。
找不到库时应用自动回退到 `FakeBatchHashService`，不会崩溃。

## CI 已自动完成的打包

CI job `Package native library (Linux + Android)`（见 `.github/workflows/ci.yml`）：

1. `flutter create` 生成 android/linux 平台骨架；
2. `cargo ndk -t arm64-v8a -t x86_64 -o app/android/app/src/main/jniLibs build -p batchhash-ffi --release`；
3. `flutter build apk --debug` 后，用 `unzip -l` **断言 APK 内确实包含两个 ABI 的 .so**；
4. `flutter build linux --debug` 后，把 `libbatchhash_ffi.so` 复制进
   `build/linux/x64/debug/bundle/lib/` 并断言文件存在；
5. 上传 `batchhash-native-library` artifact（主机的 .so 与 Android jniLibs）。

也就是说「Linux 桌面包能加载到 Rust 库」「Android APK 内含 Rust 库」这两件事，
在 CI 里是**被验证过的事实**，不是推测。

## 仍需人工/后续迭代的部分

以下内容当前没有自动化，原因是要改平台工程文件（Xcode 项目、Windows CMake、签名配置），
而不是加几行 shell 就能完成，且必须在真实设备/打包链路上验证：

1. **iOS 静态库链接**：需要为 `aarch64-apple-ios` / `aarch64-apple-ios-sim` 构建静态库，
   并在 `Runner` target 里加 `-force_load`（或通过 Podfile 挂一个本地 podspec）。
   完成后 `DynamicLibrary.process()` 即可找到符号。
2. **macOS / Windows 打包**：把 `.dylib` / `.dll` 复制到
   `build/macos/Build/Products/Debug/batchhash.app/Contents/Frameworks/` 与
   `build/windows/x64/runner/Debug/`；并且发布版需要代码签名/公证（见 docs/release-secrets.md）。
3. **发布工作流**：`.github/workflows/release.yml` 目前打包 Flutter 应用；接入原生库后需要在上传前
   执行同样的复制/断言步骤。

## 本地验证（禁止）

按仓库规则，本地不执行任何构建。上面所有断言的权威来源是 GitHub Actions 的 job 日志与 artifact。
