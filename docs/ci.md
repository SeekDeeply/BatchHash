# CI 流水线

本仓库**禁止本地构建**。GitHub Actions 是唯一的构建、静态检查与测试环境。

## CI 验证（.github/workflows/ci.yml）

触发：push 到 main、pull_request、workflow_dispatch。

| Job | 内容 | 阻断 |
|---|---|---|
| Version / changelog gate | `node scripts/verify-versions.mjs`：Cargo 工作区版本、`app/pubspec.yaml` 版本、CHANGELOG 顶部标题三者必须一致 | 是 |
| Detect components | 检测 app/ 是否存在，决定 Flutter 相关 job 是否运行 | 否 |
| Rust format | `cargo fmt --all -- --check` | 是 |
| Rust clippy / test | `cargo clippy --workspace --all-targets --locked -- -D warnings` + `cargo test --workspace --locked` | 是 |
| Flutter analyze / test | flutter create 平台脚手架 + pub get + analyze + test | 是 |
| Bridge (Rust FFI + Dart) | 真构建 `batchhash-ffi` 并用真实 .so 跑 `flutter test test/bridge` | 是 |
| Package native library | Linux bundle 与 Android APK 内必须含有 `libbatchhash_ffi.so` | 是 |
| Build Linux (debug) | GTK 工具链 + `flutter build linux --debug` | 是 |
| Build Windows (debug compile check) | `flutter build windows --debug`，**不上传 artifact** | 是 |
| Build macOS (debug) | `flutter build macos --debug` | 是 |
| Build iOS simulator | `flutter build ios --debug --simulator --no-codesign` | 是 |
| Build Android APK | `flutter build apk --debug` | 是 |

产物以 artifact 形式短期保留。**Windows 是例外：Windows 的 debug job 只做编译检查，不上传任何
artifact**，原因见下节「Windows 分发为什么只有 Release 包」。

**为什么要 `--locked`**：它同时把 `Cargo.lock` 当成被验证的产物。清单改了依赖
（例如移除 `mmap2`、给 rename crate 增加 `libc`）而 lock 没跟上时，clippy/test 会
直接失败，而不是悄悄解析成一个 Release 从未测过的版本。

工具链固定：Rust `1.89.0`（`dtolnay/rust-toolchain@1.89.0`），Flutter `3.24.5`
（`subosito/flutter-action@v2` 的 `flutter-version`，**不再用 `channel: stable`**）。
固定版本的意义是：同一份提交在任何时间重跑得到同一套编译器行为。

**Rust 版本有两处声明，必须一致**：`rust-toolchain.toml` 的 `channel` 与各工作流的
`dtolnay/rust-toolchain@<version>`。`rust-toolchain.toml` 优先级更高——rustup 在仓库目录内
会强制切到它写的 channel，而**额外 target 是按 channel 安装的**。曾经这里写 `stable`、
CI 装的却是另一个版本 + Android target，结果 `package-native` 报
`` can't find crate for `core` ``（aarch64-linux-android）。现在
`scripts/verify-versions.mjs` 会比对这两处并在不一致时直接失败，所以这种漂移不会再次
悄悄发生。`Cargo.toml` 里的 `rust-version = "1.89"` 是 MSRV 下限，不参与该文本比对，但会单独校验：
channel 低于该下限时版本门禁同样失败。**为什么下限是 1.89**：锁定依赖集自己声明了它——
`uuid 1.29.0` 要求 1.89.0，`cpufeatures`/`getrandom`/`hashbrown`/`indexmap` 是 edition 2024
crate，需要 Cargo ≥ 1.85，否则连清单都解析不了（报 `feature edition2024 is required`）。

Android 打包用的 `cargo-ndk` 仍用 `cargo install cargo-ndk --locked`：工具链升到 1.89.0 后
它的 MSRV 已被满足，且这是 CI 已经跑通的路径，没必要引入未验证的下载分支。

所有 checkout 都带 `persist-credentials: false`，job 也不持有 `contents: write`：
CI 不需要写仓库。

## 格式化流程（.github/workflows/format.yml）

本地没有构建环境，因此格式化也交给 CI：

1. 手动 dispatch "Format (CI)" 工作流。
2. 下载 formatted-sources artifact。
3. 覆盖到工作区对应文件。
4. 提交并推送。

## Release（.github/workflows/release.yml）

触发：`v*` tag 或 workflow_dispatch。

- **Windows runner 固定 `windows-2022`**：`windows-latest` 已迁移到 `windows-2025-vs2026`
  （VS 18），Flutter 3.24.5 只把 VS 17 映射到正确的 CMake generator，其余版本一律发
  `Visual Studio 16 2019`，在该镜像上不存在，构建会以 "could not find any instance of
  Visual Studio" 失败。
- **权限**：工作流级只有 `contents: read`；构建 job 各自显式 `contents: read`；
  **唯一持有 `contents: write` 的是 `publish` job**，且它 `needs: [android, macos, windows, linux]`。
- **版本门禁**：`versions` job 跑 `scripts/verify-versions.mjs`；tag 推送时还会校验
  tag 与版本、CHANGELOG 已发布条目是否一致。
- **构建 job**：android / ios / ios-unsigned / macos / windows / linux，全部
  `needs: versions`（ios-unsigned 除外，它是诊断 job）。
- **产物**：每个构建 job 用 `actions/upload-artifact` 上传
  `batchhash-android-release`、`batchhash-macos-release`、`batchhash-windows-release`、
  `batchhash-linux-release`、`batchhash-ios-unsigned-diagnostic`。
- **发布**：只有 `publish` job 会在 `startsWith(github.ref, 'refs/tags/')` 时创建
  GitHub Release 并附上全部产物。**手动 workflow_dispatch 只产出可下载 artifact，
  不创建 Release**——这一点与旧版行为不同。
- **Android 签名**：见下节。
- **iOS**：`ios` job 额外受 `vars.ENABLE_IOS_RELEASE` 门控；开启后仍会在
  “Enforce the iOS release gate” 步骤失败，因为 Rust 静态库**尚未链接进 Runner target**。
  这是刻意的：宁可失败，也不产出一个会回退到演示数据的 IPA。无签名的
  `ios-unsigned` 诊断 job 受 `vars.ENABLE_IOS_UNSIGNED` 门控，不产出发布件。

签名 secrets 说明见 docs/release-secrets.md。

## Windows 分发为什么只有 Release 包

用户报过这样的启动错误：`由于找不到 MSVCP140D.dll / VCRUNTIME140D.dll /
VCRUNTIME140_1D.dll / ucrtbased.dll，无法继续执行代码`。这四个名字末尾的 `D` 表示
**Debug CRT**，只有装了 Visual Studio 的机器才有，且授权不允许随应用分发。

成因是当时 CI 的 Windows job 会把 `runner/Debug` 整个目录作为 artifact 上传，那个 zip
看起来就是一个可直接运行的 exe 包，于是被当成发布版下载运行。现在的规则：

1. **CI 的 Windows job 不再上传任何 artifact**（job 名带 `debug compile check`）。
   它只证明 Windows 端能编译链接，并断言 `batchhash.exe` 确实产出。
2. **能下载的 Windows 包只有 `batchhash-windows-release`**，来自 release.yml 的
   `flutter build windows --release`。
3. Release 包会把与当前 MSVC 工具链配套的 **x64 Release CRT**（`vcruntime140.dll`、
   `msvcp140.dll` 等）复制到 exe 同目录（app-local），这样干净的 Windows 10/11
   即使没装 VC++ Redistributable 也能启动。
4. 打包前用 `dumpbin /dependents` 扫描包内每个 exe/dll：
   - 命中任何 Debug CRT 依赖 → **失败**（不允许发布这种包）；
   - 依赖了 MSVC 运行时但包里没有对应 DLL → **失败**（不允许发布一个必然起不来的包）。
5. 打包后再用 `tar -tf` 复核 zip 内含 `batchhash.exe`、`batchhash_ffi.dll`、
   `flutter_windows.dll` 与 CRT DLL，且不含任何 Debug CRT。

**别做的两件事**：不要从第三方"dll 下载站"单独下载这些 DLL 塞进系统目录（来路不明，
且不解决根因）；也不要为了跑起来去装 Visual Studio——那是开发环境，不是运行时前提。

以上都是**构建期静态断言**：它证明包内没有 Debug CRT 依赖、且该带的运行时都带齐了，
**不等于**已经在真实 Windows 上跑过。首次发布前仍需人工在干净机器上启动一次（见
[docs/release-checklist.md](release-checklist.md)）。

**一个已经踩过的坑**：`VC\Redist\MSVC\<ver>\` 下同时存在桌面版 `x64\Microsoft.VC142.CRT`
和 UWP 版 `onecore\x64\Microsoft.VC142.CRT`，两者都匹配 `x64` 路径模式。OneCore 是
UWP/minimal API set 变体，打进桌面应用是错的运行时。选目录时显式排除 `onecore`，
并按解析后的版本号排序（按字符串排会把 `14.9` 排在 `14.44` 之上，反而选到更旧的 CRT）。

## Android 签名（Release）

仅在 secrets 存在时生效，缺失时构建照常成功但**仍是 debug 签名**（Flutter 默认）：

1. `Configure Android release signing (when secrets are present)`（id `signing`）
   把 keystore 写到 `app/android/upload-keystore.jks`、把 `key.properties` 写到
   `app/android/key.properties`。`app/android` 是 Gradle 的 `rootProject` 目录，
   所以模板里的 `rootProject.file("key.properties")` 正好指向它。
2. 同一 job 调用 `node scripts/patch-android-signing.mjs app/android` 给生成的
   `app/build.gradle(.kts)` 注入 `signingConfigs { release { ... } }` 并把 debug
   签名引用替换为 release。脚本优先改 `.kts`，找不到就改 `.gradle`；**模板形态
   无法识别、或模板已自带 `signingConfigs` 时直接失败**，不猜。
3. `Assert release APKs are not debug-signed`（仅当 `steps.signing.outputs.configured == 'true'`）
   用 `apksigner verify --print-certs` 检查每个 split APK，证书 DN 里若还有
   `CN=Android Debug` 就报错。

**未验证**：apksigner 在 runner 上的可用路径、脚本对真实 Flutter 模板的注入结果、
以及最终签名产物，都需要一次带 secrets 的真实运行才能确认；本仓库内没有任何
证据表明它们已跑通。

## 本地"构建"
~~~sh
./scripts/trigger-ci-build.sh            # 触发 CI
./scripts/trigger-ci-build.sh --release  # 触发 Release
~~~

脚本只调用 GitHub API / gh 命令，不在本地编译任何东西。
