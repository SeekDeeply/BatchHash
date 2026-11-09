# Release 签名 secrets 配置

Release 工作流（`.github/workflows/release.yml`）在推送 `v*` tag 或手动触发时运行。
未配置 secrets 时仍会产出**开发签名**产物（Android 是 debug 签名），用于验证构建链路；
正式分发必须配置签名。

## 仓库变量（vars，而非 secrets）

| Variable | 作用 |
|---|---|
| `ENABLE_IOS_RELEASE` | 设为 `true` 才运行 `ios` job。**当前即使设为 true 也会失败**，见下文 iOS 说明。 |
| `ENABLE_IOS_UNSIGNED` | 设为 `true` 才运行 `ios-unsigned` 诊断 job（无签名、不产生发布件）。 |

## Android

| Secret | 说明 |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | `upload-keystore.jks` 的 base64（**唯一开关**：缺它就完全不签名） |
| `ANDROID_STORE_PASSWORD` | keystore 密码 |
| `ANDROID_KEY_PASSWORD` | key 密码 |
| `ANDROID_KEY_ALIAS` | key alias |

生成 base64：

~~~sh
base64 -w0 upload-keystore.jks
~~~

工作流把这些值落盘到 `app/android/upload-keystore.jks` 与 `app/android/key.properties`，
再用 `scripts/patch-android-signing.mjs` 注入 `signingConfigs`。只有
`ANDROID_KEYSTORE_BASE64` 存在时才会执行这套步骤，并且随后会用 `apksigner verify
--print-certs` 断言产物**不是** debug 签名（`CN=Android Debug` 一律判失败）。

缺 secrets 时：Release 构建照旧成功，APK/AAB 是 debug 签名，`apksigner` 断言步骤
被跳过。**不要把这种产物对外分发。**

## iOS

| Secret | 说明 |
|---|---|
| `APPLE_CERTIFICATE_BASE64` | 导出 .p12 的 base64 |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 密码 |
| `APPLE_PROVISIONING_PROFILE_BASE64` | mobileprovision 的 base64 |

**当前 iOS 无法产出可用的发布件**：Rust 静态库已经能编出来，但**尚未链接进 Runner
target**，所以即使签名齐全，`ios` job 也会在 “Enforce the iOS release gate” 步骤
显式失败。这是刻意设计的门禁，避免发布一个运行时静默回退到演示数据的 IPA。

想验证 Flutter 侧能否编译，用 `ENABLE_IOS_UNSIGNED=true` 跑 `ios-unsigned` 诊断 job。

## macOS

正式分发需要 Developer ID 证书 + 公证（notarization）；未配置时产物是 **ad-hoc
签名**（`codesign --sign -`），未公证，Gatekeeper 会提示，仅用于内部验证。

## Windows

**未接入代码签名**，产物无签名，SmartScreen 会提示。

## 安全

- secrets 只存在于 GitHub Actions secrets，不写入仓库、不写入日志。
- 工作流级权限只有 `contents: read`；唯一持有 `contents: write` 的是 `publish`
  job，它只做“下载 artifact + 创建 Release”，不执行任何构建。
- 所有 checkout 带 `persist-credentials: false`，构建步骤拿不到可用于推送的凭据。
- 任何构建产物不得提交到仓库。
- 本地无法构建，因此签名链路只能通过 Release 工作流验证；**仓库内没有任何一次
  带 secrets 的成功运行记录**，四类签名（Android/iOS/macOS/Windows）均未在此证明。
