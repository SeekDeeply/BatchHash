# 发布检查清单

打 `v*` tag 之前逐项确认。每一项都必须有 CI 证据，不接受"应该没问题"。

## 1. 代码与测试

- [ ] `CI`（push 触发）11 个 job 全绿：Rust format、clippy + 全部测试、Flutter analyze/test、
      Bridge (Rust FFI + Dart)、Package native library、五平台 debug 构建。
- [ ] `Format (CI)` 已跑过并把结果回灌仓库（否则 `cargo fmt --check` 会失败）。
- [ ] `CHANGELOG.md` 已更新到本次版本。
- [ ] `Cargo.toml` / `app/pubspec.yaml` 的版本号与 tag 一致。

## 2. release 产物（关键）

- [ ] 手动 dispatch `Release check`，Linux 与 Android 两个 job 全绿：
      这一步真的构建 release 产物，并断言
      `libbatchhash_ffi.so` 出现在 Linux bundle 与 APK/AAB 里。
- [ ] Android：`unzip -l app-release.apk | grep lib/arm64-v8a/libbatchhash_ffi.so` 有输出。
- [ ] macOS：`BatchHash.app/Contents/Frameworks/libbatchhash_ffi.dylib` 存在（release.yml 已断言）。
- [ ] Windows：`batchhash_ffi.dll` 与 `batchhash.exe` 同目录（release.yml 已断言）。
- [ ] iOS：`libbatchhash_ffi-fat.a` 已产出并随发布附带；**链接进 Runner 仍未完成**，
      未完成前 iOS 端只能用 fake 服务，不应对外宣称 iOS 版可用。

## 3. 签名与凭据

- [ ] Android：`ANDROID_KEYSTORE_BASE64` / `ANDROID_STORE_PASSWORD` / `ANDROID_KEY_PASSWORD` /
      `ANDROID_KEY_ALIAS` 已配置；未配置时产物是 debug 签名，不能上架。
- [ ] Apple：`APPLE_CERTIFICATE_BASE64` / `APPLE_CERTIFICATE_PASSWORD` /
      `APPLE_PROVISIONING_PROFILE_BASE64` 已配置；macOS 还需公证（notarization）。
- [ ] Windows：代码签名证书（release.yml 目前未接入）。

## 4. 打 tag 与验证

- [ ] 确认 tag 提交就是上面 release-check 验证过的提交。
- [ ] `git tag v0.1.0 && git push origin v0.1.0`（本地只打 tag，不做构建）。
- [ ] Release 工作流完成后，逐个下载产物并**再次**校验 .so/.dylib/.dll 是否存在
      （`unzip -l` / `tar -tzf`）——发布产物与 release-check 是两次独立构建。

## 5. 未达标就不要发

以下任一情况成立时，不具备"正式版"资格：

- release 产物里没有原生库（应用会静默使用 fake 数据，这是最危险的失败模式）；
- iOS 未完成静态库链接而对外发布 iOS 版本；
- 仅验证过 debug 构建；
- 没有任何签名（商店无法接收，用户也会看到安全警告）。
