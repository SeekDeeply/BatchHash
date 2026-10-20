# CI 流水线

本仓库**禁止本地构建**。GitHub Actions 是唯一的构建、静态检查与测试环境。

## CI 验证（.github/workflows/ci.yml）

触发：push 到 main、pull_request、workflow_dispatch。

| Job | 内容 | 阻断 |
|---|---|---|
| Detect components | 检测 app/ 是否存在，决定 Flutter 相关 job 是否运行 | 否 |
| Rust format | cargo fmt --all -- --check | 是 |
| Rust clippy / test | cargo clippy -D warnings + cargo test --workspace | 是 |
| Flutter analyze / test | flutter create 平台脚手架 + pub get + analyze + test | 是 |
| Build Linux (debug) | GTK 工具链 + flutter build linux --debug | 是 |
| Build Windows (debug) | flutter build windows --debug | 是 |
| Build macOS (debug) | flutter build macos --debug | 是 |
| Build iOS simulator | flutter build ios --debug --simulator --no-codesign | 是 |
| Build Android APK | flutter build apk --debug | 是 |

产物以 artifact 形式短期保留（debug 构建，不用于分发）。

## 格式化流程（.github/workflows/format.yml）

本地没有构建环境，因此格式化也交给 CI：

1. 手动 dispatch "Format (CI)" 工作流。
2. 下载 formatted-sources artifact。
3. 覆盖到工作区对应文件。
4. 提交并推送。

## Release（.github/workflows/release.yml）

触发：v* tag 或 workflow_dispatch。

- Android：appbundle + split-per-abi APK；配置 keystore secrets 时签名。
- iOS：配置证书时构建 IPA；否则 --no-codesign 验证。
- macOS / Windows / Linux：release 构建 + zip/tar.gz。
- 产物上传 GitHub Release。

签名 secrets 说明见 docs/release-secrets.md。

## 本地"构建"

~~~sh
./scripts/trigger-ci-build.sh            # 触发 CI
./scripts/trigger-ci-build.sh --release  # 触发 Release
~~~

脚本只调用 GitHub API / gh CLI，不在本地编译任何东西。
