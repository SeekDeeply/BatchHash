# Contributing to BatchHash

## 铁律：本地不构建

本仓库禁止在本地执行任何构建、测试或产物生成命令，包括但不限于：

- `cargo build` / `cargo test` / `cargo run`
- `flutter build` / `flutter test` / `flutter run`
- 除 `flutter_rust_bridge_codegen generate` 之外的代码生成

所有校验由 GitHub Actions 完成：

- push / PR：`.github/workflows/ci.yml`（Rust fmt/clippy/test、Flutter analyze/test、五端 debug 构建）
- tag `v*`：`.github/workflows/release.yml`（签名 / 可安装产物 → GitHub Release）

本地需要"构建"时，使用：

```sh
./scripts/trigger-ci-build.sh            # 触发 CI 验证
./scripts/trigger-ci-build.sh --release  # 触发 Release 流水线
```

本地唯一的允许行为：编辑源码/文档、静态审阅、查看 CI 日志与产物。

## 工作流

1. 从 `main` 创建分支：`feat/<scope>`、`fix/<scope>`、`docs/<scope>`。
2. 提交遵循 Conventional Commits：`feat(hash): ...`、`fix(report): ...`。
3. 推送分支并创建 PR；等待 CI 全绿后合并。
4. 不要在 PR 中提交构建产物、`target/`、`app/build/`、本地日志或密钥。

## 代码规范

- Rust：`cargo fmt` 语义（CI 校验）、`clippy -D warnings`、公共项必须有 rustdoc、
  库代码禁止 `unwrap()`（测试除外）、错误用 `thiserror` 建模。
- 哈希十六进制统一小写；算法列表与 CSV 列序使用 AlgorithmRegistry 规范顺序（schema v1 契约）。
- Flutter：Material 3、`flutter_lints` 零告警、可访问性（状态不能只靠颜色）。
- 任何 schema 变更必须同步文档并递增 `schema_version`。

## 提交前自检（只读）

- 阅读 `docs/internal-api-contracts.md`，确认公共 API 未破坏契约。
- 确认未修改无关 crate、未引入密钥、未提交生成物。
- 推送后观察 CI；修复必须通过 CI 验证，不能以"本地通过"为依据。
