# 原生库打包（平台侧）

Rust 核心编译产物是 `batchhash-ffi`：

| 平台 | 产物 | 用途 |
|---|---|---|
| Linux | `libbatchhash_ffi.so` | `dart:ffi` 动态加载 |
| Windows | `batchhash_ffi.dll` | `dart:ffi` 动态加载 |
| macOS | `libbatchhash_ffi.dylib` | `dart:ffi` 动态加载 |
| Android | `libbatchhash_ffi.so`（arm64-v8a / armeabi-v7a / x86_64） | 打进 APK 的 `jniLibs` |
| iOS | `libbatchhash_ffi.a`（静态库） | **尚未**链接进 Runner，见下 |

Dart 侧加载顺序见 [docs/bridge.md](bridge.md)：`BATCHHASH_FFI_LIB` → iOS 进程内 → 平台默认库名。
找不到库时应用自动回退到 `FakeBatchHashService`，不会崩溃，并且界面顶部常驻「演示数据」横幅。

## CI 断言的打包事实（debug）

CI job `Package native library (Linux + Android)`（`.github/workflows/ci.yml`）：

1. `flutter create` 生成 android/linux 平台骨架；
2. `cargo ndk -t arm64-v8a -t x86_64 -o app/android/app/src/main/jniLibs build -p batchhash-ffi --release --locked`；
3. `flutter build apk --debug` 后用 `unzip -l` **断言 APK 内含两个 ABI 的 .so**；
4. `flutter build linux --debug` 后把 `libbatchhash_ffi.so` 复制进
   `build/linux/x64/debug/bundle/lib/` 并断言文件存在；
5. 上传 `batchhash-native-library` artifact。

也就是说「Linux 桌面包内含 Rust 库」「Android APK 内含 Rust 库」在 CI 里是**被断言的事实**。

## Release 断言的打包事实（release）

`.github/workflows/release.yml` 的构建 job 各自做同样的复制 + 断言：

| Job | 复制目标 | 断言 |
|---|---|---|
| android | `cargo ndk` 三个 ABI → `app/android/app/src/main/jniLibs` | 三个 split APK 与 AAB 内各自有对应 ABI 的 .so |
| macos | `target/release/libbatchhash_ffi.dylib` → `BatchHash.app/Contents/Frameworks/` | 文件存在；随后做 **ad-hoc** 签名（`codesign --sign -`）；打包后再用 `unzip -l` 复核 |
| windows | `target/release/batchhash_ffi.dll` → `Release/`，并把 x64 **Release CRT** 复制到同目录 | 文件存在；`dumpbin /dependents` 扫描确认无 Debug CRT 依赖、且所有 MSVC 运行时依赖都已打进包里；zip 内含 exe/dll/CRT 且不含 Debug CRT |
| linux | `target/release/libbatchhash_ffi.so` → `bundle/lib/` | 文件存在；打包后用 `tar -tzf` 复核 |
| ios | 静态库 `lipo` 成 fat .a | 仅产出库文件，**不构建 IPA**（见下） |

**未验证的部分**：以上都是**构建期**断言（文件在产物里）。仓库内没有任何一次在真实
设备或安装后的应用上做过**运行时加载**测试，因此「`dart:ffi` 真能加载到这些库」只有
Linux/Android 的 debug 链路在 CI 里有间接证据（Bridge job 用真实 .so 跑 Dart 测试），
桌面 release 包的运行时加载从未被执行过。

Windows 的 PE 依赖扫描同理：它证明**包内没有 Debug CRT 依赖、且该带的运行时都带齐了**，
不证明它已经在某台干净的 Windows 上启动成功。这两件事不要混为一谈。

**Windows 为什么不提供 debug 包下载**：debug 链的是 Debug CRT（`MSVCP140D.dll`、
`VCRUNTIME140D.dll`、`VCRUNTIME140_1D.dll`、`ucrtbased.dll`），只在装了 Visual Studio
的机器上存在，且不允许分发。曾经把 CI 的 `runner/Debug` 当作 artifact 上传，用户在普通
机器上双击就得到「找不到 MSVCP140D.dll」的对话框。现在 CI 的 Windows job 只做编译检查、
不上传 artifact，可下载的 Windows 包只有 release.yml 产出的 Release zip。详见
[docs/ci.md](ci.md) 的「Windows 分发为什么只有 Release 包」。

## 尚未完成的部分

1. **iOS 静态库链接**（唯一真正的缺口）：需要把 `libbatchhash_ffi.a` 通过
   `-force_load` 或本地 podspec 挂进 `Runner` target，之后
   `DynamicLibrary.process()` 才能找到符号。在完成之前：
   - `ios` release job 会在 “Enforce the iOS release gate” 步骤**显式失败**；
   - iOS 端运行时会回退到 fake 服务；
   - 因此不应对外宣称 iOS 版可用。
2. **macOS 公证**：目前只有 ad-hoc 签名，未公证，Gatekeeper 会提示（见 docs/release-secrets.md）。
3. **Windows 代码签名**：未接入。

## 本地验证（禁止）

按仓库规则，本地不执行任何构建。上面所有断言的权威来源是 GitHub Actions 的 job 日志与 artifact。
