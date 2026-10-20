# BatchHash 项目规划（v1.5 Final 摘要）

> 完整规划由产品定稿于 2026-10-20，本文件是仓库内的执行摘要。v1.5 固化的 12 项决策是 P0 及
> 后续阶段不可回退前提。

## 0. 固化决策

1. **AlgorithmRegistry**：16 个算法全部注册化，统一稳定 ID、显示名、输出长度、是否加密、
   是否可用于写名、后缀推断规则。
2. **写名算法唯一**：写文件名后缀必须指定 **suffix_algorithm**，默认 BLAKE3；非加密算法写名需警告。
3. **碰撞预检**：写名预检包含后缀碰撞估算与推荐长度；默认 5 位，大批量必须提示。
4. **单遍多哈希与 BLAKE3 并行边界**：多算法勾选时单遍流式；仅 BLAKE3 大文件允许 mmap/分块并行。
5. **Android SAF 重命名**：由 Kotlin 的 DocumentsContract.renameDocument 执行；Rust 只产出计划、
   冲突检测与日志模型。
6. **Android fd 语义**：fd 传入 Rust 后必须 dup + seek(offset) + 限定长度读取，不关闭原 fd，
   不假设 offset 为 0。
7. **单根批处理**：每份报告必须有明确 root；跨目录多选必须选择共同根，根外文件标记 out_of_root。
8. **schema 版本化**：报告 schema v1 与 Diff schema v1 包含错误状态、原因、suffix_algorithm、
   root_kind 等字段。
9. **硬件可观测性分级**：记录检测特性、策略、库报告后端，允许 unknown，不虚假精确。
10. **CI 强化**：Rust 平台库 staging、codegen 一致性检查、fmt/clippy、性能基线策略。
11. **持久化撤销**：重命名日志 SQLite；iOS 跨会话撤销使用 security-scoped bookmark；
    Android 使用 persisted URI permission。
12. **构建边界**：允许本地调试运行（历史规划），本仓库执行用户最终指令：**一切构建/测试交给
    GitHub CI，本地不构建**。

## 1. 核心链路

批量计算哈希 → 可选写入文件名后缀 → 反向提取校验 → 多格式统计报告 →
双列报告对比（含报告 vs 实时目录）→ 一键撤销。

## 2. 架构

- **Flutter（全平台 UI）**：Material 3、dynamic_color、自适应布局。
- **Rust 核心库**：Hash Engine、Batch Scheduler、Suffix、Rename Planner、Verify、Checksum、
  Report、Diff Engine、PathNormalizer、HardwareProbe、Log Store。
- **桥接**：flutter_rust_bridge 2.x（P0.5 起）。
- **平台原生**：Android Kotlin SAF/renameDocument/权限；iOS Swift bookmark；
  Flutter isHardwareAccelerated / GPU 后端。

架构前提：Tauri 前端是系统 WebView，Flutter 不能作为 Tauri 前端，因此 **不使用 Tauri**。

## 3. 文件访问抽象

- 桌面 / iOS：路径输入，Rust 打开文件。
- Android：SAF 获取 ParcelFileDescriptor，传 fd + offset + length；Rust dup/seek/限定长度/不关闭原 fd。
- 统一文件条目：root_id、root_kind、relative_path、display_path、platform_meta、size、mtime、
  hashes、status、error_kind、error_message。
- 符号链接默认不跟随；实时扫描遇到符号链接跳过并标记。

## 4. 报告与对比

| 格式 | 导出 | 导入 | 对比输入 |
|---|---|---|---|
| JSON | 是 | 是 | 是 |
| CSV | 是 | 是 | 是 |
| Markdown | 是 | 否 | 否 |
| TXT | 是 | 否 | 否 |
| md5sum | 是 | 是 | 是（单算法） |
| BSD | 是 | 是 | 是（单算法） |

- 报告：单根、schema v1、完整哈希 + 截断后缀、hardware 元数据、summary 统计。
- 对比 key：相对 root 路径 + 归一化（点段、分隔符、NFC、大小写）；降级模式仅按文件名。
- Diff 状态：unchanged / changed / added / removed / unknown / ambiguous / duplicate_key /
  out_of_root / corrupt / unstable。算法取交集；交集为空降级为清单对比。
- 实时对比：流式增量填充；取消后不得批量标 removed，摘要标 incomplete；
  哈希前后两次 (size, mtime) 不一致标记 unstable。

## 5. 性能策略

- 单遍读取、多哈希并行喂入（一次 I/O）。
- Rayon 跨文件并行；固定缓冲每 worker 4–8 MB。
- BLAKE3：多算法时关闭内部分块并行；仅 BLAKE3 且大文件可 mmap/分块并行（默认关闭）。
- 进度 / ETA / 取消：全局聚合，chunk 边界检查取消令牌。
- 不做哈希缓存，每次全量计算。

## 6. 路线图

| 阶段 | 目标 | 关键交付 |
|---|---|---|
| **P0** | 核心引擎 + CI 骨架 | AlgorithmRegistry；单遍多哈希 + BLAKE3 策略；fd 双入口；Rayon 进度/取消；HardwareProbe；PathNormalizer；Report/Diff schema v1；SQLite schema 草案；Rust staging 脚本；Flutter 空壳；CI 矩阵 |
| **P0.5** | 桥接与平台基线 | Android fd 桥接、Kotlin SAF 选目录、iOS bookmark 最小样例；codegen 一致性；fmt/clippy 阻断 |
| **P1** | 桌面 MVP | 选文件/目录、root 选择、算法多选、结果着色、dry-run、硬件面板 |
| **P2** | 写名闭环 | suffix_algorithm、碰撞预检、冲突检测、SQLite 日志、跨会话撤销、六格式导出四格式导入 |
| **P3** | 校验 + 对比 | 文件名反向校验、校验和 I/O、Diff Engine 完整规范、双列对比 UI、Patrol |
| **P4** | Android | SAF、持久化权限、fd dup/offset、Kotlin renameDocument 降级、前台服务 |
| **P5** | iOS + 实时对比 + 打磨 | 签名/unsigned IPA、流式实时对比、unstable/incomplete、动态取色、GB/TB 压测 |

## 7. 关键风险与应对

| 风险 | 应对 |
|---|---|
| 5 位后缀大批量碰撞 | 碰撞预检 + 推荐长度 + 冲突默认跳过 |
| BLAKE3 内并行与单遍冲突 | 策略表：多算法关闭内并行 |
| Android SAF 重命名运行时失败 | Kotlin 执行 + 能力预判 + 逐条降级，不复制删除 |
| fd 所有权误关闭 / offset 错误 | dup/borrowed + seek(offset) + 限定长度 + 单测 |
| 报告 root 跨目录不成立 | 单根约束 + 共同根选择 + out_of_root |
| 路径归一化跨平台不一致 | PathNormalizer 独立 + 全用例 CI + golden |
| 硬件后端实际值不可得 | reported/unknown 语义，不断言特定后端 |
| 取消实时对比产生虚假 removed | 取消后不批量标 removed，标 incomplete |
| 双列同步滚动性能 | 单滚动容器 / 固定行高 / 虚拟列表 |
| 截断后缀被当成安全摘要 | 报告同时存完整哈希；对比只用完整哈希；UI 明示 |

## 8. 验收要点

- 16 算法全部可勾选；单遍流式路径与逐一算法结果一致；输出小写十六进制。
- 后缀格式 原名[ABCDE].扩展名；默认 5 位大写；suffix_algorithm 必选并记录；冲突默认跳过。
- dry-run、冲突检测、执行、SQLite 日志、跨会话撤销全链路可用。
- 六种导出、四种导入；JSON/CSV round-trip 逐字节一致。
- 报告 vs 报告 / 报告 vs 实时目录 / 校验和互比；PathNormalizer 全用例；Diff schema 导出。
- CPU 哈希加速与 GPU 渲染加速分区展示；reported/unknown；强制软件与自动加速结果逐字节一致。
- Android SAF 选目录 / fd 哈希 / Kotlin 重命名降级可用，原 fd 不被关闭。
- iOS 跨会话撤销使用 bookmark，访问失效报冲突。
- CI：push/PR 全绿；tag 产出可安装产物；本地零分发构建。
