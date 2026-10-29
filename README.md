# BatchHash

跨平台批量文件哈希校验工具：**批量计算哈希 → 可选写入文件名后缀 → 反向提取校验 → 多格式统计报告 →
双列报告对比（含报告 vs 实时目录）→ 一键撤销**。

UI 采用 Flutter + Material Design 3（动态取色、自适应布局），核心引擎使用 Rust 并通过
`flutter_rust_bridge` 2.x 桥接。支持 Windows / macOS / Linux / Android / iOS。

> 规划版本：v1.5 Final。**P0 已完成**：Rust 引擎四件套 + 报告/对比/写名/撤销 + 开发者 CLI + Flutter UI 外壳，
> 全部由 CI 验证（格式、clippy、工作区测试、Flutter analyze/test、五平台 debug 构建）。当前向 **P0.5（flutter_rust_bridge 桥接）** 推进。

## 当前状态

| 能力 | 状态 | 验证方式（全部在 CI） |
|---|---|---|
| 16 算法单遍多哈希、fd 输入、Rayon 批处理、取消/进度/ETA | 已实现 | `cargo test --workspace`（含已知向量、单遍读取计数、scheduler 行为测试） |
| 报告六导四入（JSON/CSV/MD/TXT/md5sum/BSD） | 已实现 | 金标准字节比对 + 往返测试 + 导入上限/穿越/未来 schema 拒绝 |
| PathNormalizer + full outer join Diff + 流式对比 | 已实现 | 归一化用例表、8 组 diff 金标准、duplicate/ambiguous/incomplete 语义 |
| 后缀写名、纯计划、SQLite 日志、跨会话撤销 | 已实现 | 计划不写盘、冲突检测、状态机、端到端 rename→log→undo |
| 开发者 CLI（hash/report/diff/scan-diff/suffix/extract/probe） | 已实现 | 二进制端到端测试；见 [docs/cli.md](docs/cli.md) |
| Flutter UI 外壳（MD3、自适应、双列对比、硬件面板） | 已实现（假数据服务） | `flutter analyze` + widget 测试 |
| 原生桥接（JSON C ABI + dart:ffi） | 已实现 | CI job `Bridge (Rust FFI + Dart)`：真跑 Rust 库 |
| UI 接入真实桥接（RustBatchHashService，缺库回退 fake） | 已实现 | 桥接 job 里的服务级端到端测试 |

## 构建策略（重要）

**禁止任何本地构建与分发构建。**

- 本地不执行 `cargo build` / `cargo test` / `flutter build` / `flutter test`。
- 所有编译、静态检查、测试、产物构建全部由 GitHub Actions 完成（见
  [`.github/workflows/ci.yml`](.github/workflows/ci.yml)）。
- 本地"构建"脚本 [`scripts/trigger-ci-build.sh`](scripts/trigger-ci-build.sh) 只负责触发 CI。
- 本地只允许：编辑代码、文档、静态审阅；调试运行由 CI 产物承担。

## 特性

- **16 个注册算法**：MD5、SHA-1、SHA-224/256/384/512、SHA3-256/512、BLAKE2b/2s、BLAKE3、
  XXH3-64/128、XXH64、CRC32/CRC32C。统一 AlgorithmRegistry，稳定 ID、显示名、输出长度、
  加密属性、写名可用性、后缀推断规则集中定义。
- **单遍多哈希**：一次 I/O 读取同时喂入所有勾选算法；仅 BLAKE3 大文件可走 mmap/分块并行策略。
- **写名后缀**：`原名[ABCDE].扩展名`，默认 5 位大写，写名算法唯一（`suffix_algorithm`，默认 BLAKE3），
  支持碰撞估算、推荐长度、dry-run、冲突检测、SQLite 日志与跨会话撤销。
- **报告**：JSON / CSV / Markdown / TXT / md5sum / BSD 导出；JSON / CSV / md5sum / BSD 导入。
- **对比**：报告 vs 报告、报告 vs 实时目录、校验和文件互比；PathNormalizer 归一化 +
  full outer join Diff Engine，流式增量对比、`incomplete` / `unstable` 语义。
- **硬件加速可观测**：CPU 哈希加速与 Flutter GPU 渲染加速分区展示；后端状态允许 `unknown`，
  不做虚假精确；软件回退为中性语义。

## 仓库结构

```
crates/
  batchhash-types/    共享类型：AlgorithmRegistry、文件条目、Report schema v1、Diff schema v1
  batchhash-hash/     哈希引擎：单遍多哈希、fd 输入、Rayon 批处理、HardwareProbe
  batchhash-report/   PathNormalizer、Diff Engine、报告六种导出 / 四种导入
  batchhash-rename/   后缀规则、重命名计划与执行、SQLite 日志与撤销
  batchhash-core/     门面 crate（供桥接层与 CLI 使用）
  batchhash-cli/      开发者 CLI（仅用于调试，不参与分发）
app/                  Flutter UI（MD3、自适应布局、双列对比）
docs/                 规划、schema 与平台文档
scripts/              CI 构建辅助脚本（本地不执行构建）
.github/workflows/    CI 验证流水线与 Release 流水线
```

## 文档

| 文档 | 内容 |
|---|---|
| [docs/PROJECT-PLAN.md](docs/PROJECT-PLAN.md) | 项目规划 v1.5 摘要与阶段路线图 |
| [docs/internal-api-contracts.md](docs/internal-api-contracts.md) | Rust crate 间接口契约 |
| [docs/report-schema-v1.md](docs/report-schema-v1.md) | 报告 JSON schema v1 与六种格式规范 |
| [docs/diff-schema-v1.md](docs/diff-schema-v1.md) | Diff schema v1 与状态语义 |
| [docs/algorithm-registry.md](docs/algorithm-registry.md) | 16 个算法注册表 |
| [docs/path-normalization.md](docs/path-normalization.md) | 对比 key 归一化规范与全用例表 |
| [docs/hardware-probe.md](docs/hardware-probe.md) | 硬件探测字段与 reported/unknown 语义 |
| [docs/file-entry-root-spec.md](docs/file-entry-root-spec.md) | 文件条目与单根批处理规范 |
| [docs/rename-log-schema.md](docs/rename-log-schema.md) | 重命名日志 SQLite schema 与撤销状态机 |
| [docs/android-saf.md](docs/android-saf.md) | Android SAF / fd / Kotlin 重命名职责边界 |
| [docs/privacy-and-security.md](docs/privacy-and-security.md) | 隐私、截断哈希安全语义、导入校验 |
| [docs/cli.md](docs/cli.md) | 开发者 CLI 命令、选项与退出码 |
| [docs/bridge.md](docs/bridge.md) | 原生桥接 ABI 契约、Dart 绑定与 CI 校验方式 |
| [docs/ci.md](docs/ci.md) | CI 流水线结构与"禁止本地构建"的落地方式 |

## 安全语义

- MD5 / SHA-1 已被密码学攻破；XXH / CRC 为非密码学校验。它们不能用于安全校验。
- 文件名中的截断后缀只用于便利比对，**不是安全摘要**；报告同时保存完整哈希，对比始终使用完整哈希。
- 报告导入默认不可信：限制条目数与文件大小、拒绝未来 schema 版本、拒绝路径逃逸。

## 许可证

Apache-2.0，见 [LICENSE](LICENSE)。