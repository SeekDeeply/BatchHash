# Release 签名 secrets 配置

Release 工作流（.github/workflows/release.yml）在推送 v* tag 或手动触发时运行。
未配置 secrets 时仍会产出**未签名/开发签名**产物，用于验证构建链路；正式分发必须配置签名。

## Android

| Secret | 说明 |
|---|---|
| ANDROID_KEYSTORE_BASE64 | upload-keystore.jks 的 base64 |
| ANDROID_STORE_PASSWORD | keystore 密码 |
| ANDROID_KEY_PASSWORD | key 密码 |
| ANDROID_KEY_ALIAS | key alias |

生成 base64：

~~~sh
base64 -w0 upload-keystore.jks
~~~

## iOS

| Secret | 说明 |
|---|---|
| APPLE_CERTIFICATE_BASE64 | 导出 .p12 的 base64 |
| APPLE_CERTIFICATE_PASSWORD | .p12 密码 |
| APPLE_PROVISIONING_PROFILE_BASE64 | mobileprovision 的 base64 |

未配置时回退到 --no-codesign 构建（仅模拟器/侧载验证，不是正式分发通道）。

## macOS

正式分发需要 Developer ID 证书 + 公证（notarization）；未配置时产物为 ad-hoc 签名，
Gatekeeper 会提示，仅用于内部验证。

## 安全

- secrets 只存在于 GitHub Actions secrets，不写入仓库、不写入日志。
- 任何构建产物不得提交到仓库。
- 本地无法构建，因此签名链路只能通过 Release 工作流验证。