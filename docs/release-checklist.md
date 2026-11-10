# 发布检查清单

打 `v*` tag 之前逐项确认。每一项都必须有 CI 证据，不接受"应该没问题"。

分两类：**[CI 自动断言]** 由工作流自己跑，人只需看 job 是否绿；**[人工检查]** CI 无法证明，
必须有人真的看一遍。不要把人工项写成 CI 项——那是谎报。

## 1. 代码与测试 [CI 自动断言]

- [ ] `CI`（push 触发）全绿，含 `Version / changelog gate`、Rust format、
      Rust clippy + test（`--locked`）、Flutter analyze/test、`Bridge (Rust FFI + Dart)`、
      `Package native library`、五平台 debug 构建。
- [ ] `Format (CI)` 已跑过并把结果回灌仓库（否则 `cargo fmt --check` 会失败）。
- [ ] 版本门禁：`Cargo.toml` 工作区版本、`app/pubspec.yaml` 版本同为 `1.0.0`，
      `Cargo.lock` 内各 `batchhash-*` crate 版本同为 `1.0.0`（CI 用 `--locked` 构建），
      CHANGELOG 顶部为 `[未发布]` 或与版本一致的已发布标题。

## 2. release 产物 [CI 自动断言]

- [ ] `Release`（手动 dispatch 即可）全绿，五个构建 job 都产出 artifact：
      `batchhash-android-release`、`batchhash-macos-release`、`batchhash-windows-release`、
      `batchhash-linux-release`、（若开启）`batchhash-ios-unsigned-diagnostic`。
- [ ] Android 产物内三个 ABI 的 `.so` 都在（工作流已用 `unzip -l` 断言）：
      `lib/arm64-v8a/`、`lib/armeabi-v7a/`、`lib/x86_64/libbatchhash_ffi.so`。
- [ ] macOS `BatchHash.app/Contents/Frameworks/libbatchhash_ffi.dylib` 存在（release.yml 已断言）。
- [ ] Windows `batchhash_ffi.dll` 与 `batchhash.exe` 同目录（release.yml 已断言）。
- [ ] Windows 包内**没有 Debug CRT 依赖**，且 x64 Release CRT（`vcruntime140.dll` 等）
      已 app-local 打进包（release.yml 用 `dumpbin /dependents` 断言；命中
      `MSVCP140D.dll`/`VCRUNTIME140D.dll`/`ucrtbased.dll` 即失败）。
- [ ] **CI 的 Windows job 没有 artifact**：它只是 debug 编译检查。若有人在 Actions 里
      找到一个可运行的 Windows exe 包，那说明有步骤被误加回去了——debug 包在干净机器上
      必然报「找不到 MSVCP140D.dll」。
- [ ] Linux `lib/libbatchhash_ffi.so` 在 tar.gz 内（release.yml 已断言）。
- [ ] `publish` job 只在 tag 推送时运行；手动 dispatch **不创建 Release**，这是预期行为。

## 3. 签名 [部分 CI，部分人工]

- [ ] Android：四个 secrets 已配置（`ANDROID_KEYSTORE_BASE64` 是唯一开关）。
      配置后 CI 会用 `apksigner verify --print-certs` 断言 split APK **不是** debug 签名。
      **未配置时产物仍是 debug 签名，不能上架**——这一点 CI 不会替你拦（它跳过断言）。
- [ ] Apple：`APPLE_CERTIFICATE_BASE64` / `APPLE_CERTIFICATE_PASSWORD` /
      `APPLE_PROVISIONING_PROFILE_BASE64` 已配置。
- [ ] macOS 公证：**未接入**，产物只有 ad-hoc 签名，Gatekeeper 会提示。
- [ ] Windows 代码签名：**未接入**。

## 4. iOS 发布开关 [人工]

- [ ] 默认保持 `ENABLE_IOS_RELEASE` 未设置（`ios` job 不运行）。
- [ ] **不要**在静态库链接完成前把它设成 `true`：即使设了，job 也会在
      “Enforce the iOS release gate” 步骤失败，这是刻意门禁。
- [ ] 需要验证 Flutter 侧编译时用 `ENABLE_IOS_UNSIGNED=true` 跑诊断 job，产物不对外。

## 5. 打 tag 与验证 [人工]

- [ ] 确认 tag 提交就是上面 `Release` 验证过的提交。
- [ ] `git tag v1.0.0 && git push origin v1.0.0`（本地只打 tag，不做构建）。
- [ ] tag 触发的 `versions` job 会额外校验 tag 与版本、CHANGELOG 已发布条目；任一不符即失败。
- [ ] Release 工作流完成后（由 `publish` job 创建），逐个下载产物并**再次**校验
      `.so`/`.dylib`/`.dll` 是否存在——发布产物与之前的 dispatch 是两次独立构建。
- [ ] 至少在一台真实机器上**安装并启动**一次桌面包，确认界面没有持续显示「演示数据」横幅
      （横幅存在即说明原生库没被加载）。这一项 CI 完全无法证明。
- [ ] Windows 必须在一台**没装 Visual Studio、也没装 VC++ Redistributable** 的干净
      Windows 10/11 上启动 Release zip 验证：能启动、且不弹「找不到 MSVCP140D.dll /
      VCRUNTIME140D.dll / ucrtbased.dll」。这类报错说明拿到的是 debug 包或 CRT 没打包，
      两者都是发布阻断项，不能靠让用户装 Visual Studio 绕过。

## 6. 未达标就不要发

以下任一情况成立时，不具备"正式版"资格：

- release 产物里没有原生库（应用会静默使用 fake 数据，这是最危险的失败模式）；
- iOS 未完成静态库链接而对外发布 iOS 版本；
- 仅验证过 debug 构建（Windows 尤其：debug 包必定依赖 Debug CRT，在普通机器上必然起不来）；
- 没有任何签名，或 Android 产物仍是 debug 签名（商店无法接收，用户也会看到安全警告）；
- macOS 未公证却宣称"可直接分发"。
