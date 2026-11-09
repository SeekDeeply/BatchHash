# 原生桥接（Rust ↔ Dart）

`crates/batchhash-ffi` 是 Rust 核心暴露给非 Rust 代码的唯一出口，Dart 侧通过 `dart:ffi` 调用。
两层结构刻意分开：

| 层 | 位置 | 职责 |
|---|---|---|
| 纯 Rust API | `crates/batchhash-ffi/src/api.rs` | 请求/响应结构体（全部是标量、String、Vec），有单元测试，不涉及指针 |
| C ABI | `crates/batchhash-ffi/src/c_api.rs` | JSON 进 / JSON 出，捕获 unwind，管理内存 |
| Dart 绑定 | `app/lib/src/bridge/native_bridge.dart` | 加载动态库、解析信封、抛出类型化异常 |

## 为什么是 JSON C ABI

规划里写的是 `flutter_rust_bridge` 2.x。实际实现改成了「JSON C ABI + `dart:ffi`」，原因只有一条：
**可验证**。FRB 需要 `flutter_rust_bridge_codegen` 在构建时生成 Rust 与 Dart 胶水代码，
而本仓库禁止本地构建，生成物只能靠 CI 往返，出问题时很难区分是「绑定生成失败」还是「业务逻辑错」。
JSON C ABI 的每一层都能被 CI 直接测试：

- `cargo test -p batchhash-ffi` 覆盖纯 Rust API 与 C ABI 信封；
- CI 的 `Bridge (Rust FFI + Dart)` job 先 `cargo build -p batchhash-ffi --release` 产出 `libbatchhash_ffi.so`，
  再用 `flutter test test/bridge` 真的通过 `dart:ffi` 调用它并断言「哈希 → 写名 → 撤销」全链路。

`api.rs` 里的类型仍然是普通可序列化结构体，如果以后要接 FRB，可以直接把它们作为 FRB 的 `rust_input`，
不需要改动业务代码。

## ABI 契约

- 版本：`CONTRACT_VERSION = 1`（请求/响应结构），`C_ABI_VERSION = 1`（信封形状）。
- 所有函数返回 **堆分配的 NUL 结尾 UTF-8 JSON**，调用方必须用 `bh_free` 释放。
- 信封：

```json
{ "ok": true,  "api_version": 1, "data": { } }
{ "ok": false, "api_version": 1, "kind": "invalid_request", "error": "..." }
```

- 任何 Rust panic 都被 `catch_unwind` 转成 `kind = "internal"` 的信封，绝不跨越 FFI 边界。
- 请求指针为 `NULL` 时：`bh_probe` 视为空请求，其余报 `invalid_request`。

### 导出函数

| 函数 | 请求 | 说明 |
|---|---|---|
| `bh_api_version` | 无 | 契约版本、C ABI 版本、工具名与版本 |
| `bh_algorithms` | 无 | 16 个注册算法（id / 显示名 / hex 长度 / 加密属性 / 可否写名） |
| `bh_probe` | `{"forced_soft": bool}` | 硬件可观测信息，后端未知时如实返回 `unknown` |
| `bh_scan` | `{"root": "...", "follow_links": bool}` | 只扫描不哈希 |
| `bh_run_batch` | `BatchRequestDto` | 扫描 → 哈希 → 可选写名 → 报告 → SQLite 日志 |
| `bh_undo` | `{"db": "...", "batch_id"?: "...", "apply": bool}` | 撤销计划/执行 |
| `bh_export_report` | `{"report_json": "...", "format": "csv"}` | 报告格式转换 |
| `bh_job_start` | `{"action": "batch"\|"hash_entries"\|"diff_directory", "payload": {...}}` | 立刻返回 `job_id`，工作在原生线程执行 |
| `bh_job_poll` | `{"job_id": "..."}` | 当前状态、最新进度样本、完成后的 `result_json` |
| `bh_job_cancel` | `{"job_id": "..."}` | 请求停止；任务仍通过 poll 报告终态 |
| `bh_job_release` | `{"job_id": "..."}` | 丢弃任务与结果，释放其占用 |

### 异步任务（长任务必须走这里）

单次同步调用会一直阻塞到结束。哈希一个数 GB 的文件、或把一个报告与一个大目录对比，
足以让 Flutter 唯一的 isolate 停住，界面看起来就是「卡死」。因此这两类工作通过任务协议执行：

1. `bh_job_start` 只做校验与排队，立即返回 `job_id`；校验失败（例如未知算法）在 start 阶段
   就以 `invalid_request` 报错，而不是异步失败。
2. `bh_job_poll` 返回 `state`（`running` / `completed` / `failed` / `cancelled`）、
   `progress` 与终态数据。`progress` 含 `phase`、`completed`/`total`、
   `bytes_done`/`bytes_total`、`percent`、`eta_seconds`、`current_path`、`elapsed_ms`。
3. `bh_job_cancel` 请求停止；**取消优先于成功**——已取消的任务不会返回 `result_json`。
4. `bh_job_release` 释放任务；释放后再 poll 该 id 会以 `invalid_request` 失败。

`bh_api_version` 的 `features` 数组声明这些能力（当前为 `async_jobs`、`progress`）。
信封结构没有变化，所以 C ABI 版本没有随之递增；Dart 侧按能力探测，而不是靠版本号猜。
旧的原生库没有这些符号时，服务层回退到同步调用，功能可用但只有一次性进度。

`bh_run_batch` 的响应包含：汇总计数、条目（相对路径、显示路径、大小、mtime、状态、错误类型、哈希数组）、
扫描跳过的路径、写名计划行与汇总、`batch_id`、重命名成功/失败数、是否取消、耗时，
以及 **canonical report JSON**（`report_json`），UI 可以直接拿去做导出或对比。

`BatchRequestDto` 的两个可选字段决定「这次到底处理哪些文件」，UI 必须显式传：

- `recursive`（默认 `true`）：是否进入子目录。用户选「只此文件夹」时传 `false`，
  否则批次会静默扩大到整棵树。
- `restrict_paths`（默认空数组 = 不限制）：根相对路径列表。UI 预览了写名计划后
  执行时传入计划里 `ready` 行的原路径，Rust 只处理这些条目，因此执行阶段不可能碰到
  预览里没出现的文件。路径必须是正斜杠根相对形式（绝对路径、`..` 逃逸、含 NUL 都会
  以 `invalid_request` 拒绝）；执行前已消失的路径只是不出现在结果里，不会被当成失败。

错误的 `kind` 与 Rust 错误族一一对应：`invalid_request`（请求自相矛盾，重试无意义）、
`scan`、`suffix`、`log`、`report`、`io`、`too_large`、`invalid_report`、`export`、`unsupported`。
旧版本里的通用 `pipeline` 已不再出现，Dart 侧不要再按它分支。

## Dart 侧

`BatchHashNative.tryOpen()` 的查找顺序：

1. 环境变量 `BATCHHASH_FFI_LIB`（CI 与测试用）；
2. iOS：`DynamicLibrary.process()`（静态库链接进进程）；
3. 其它平台：`libbatchhash_ffi.so` / `libbatchhash_ffi.dylib` / `batchhash_ffi.dll`。

找不到库时返回 `null`，调用方可以回退到 `FakeBatchHashService`（P0 的 UI 测试就是这么跑的）。
业务错误统一抛 `BridgeException(kind, message)`，未知算法、空算法列表、缺日志路径都是 `invalid_request`。

## UI 服务层

`app/lib/src/services/rust_batch_hash_service.dart` 实现 `BatchHashService`，
把桥接 DTO 映射到 UI 模型（`BatchEntry` / `ReportModel` / `RenamePlanModel` /
`DiffRowModel` / `HistoryBatchModel`）：

| 服务方法 | Rust 侧 |
|---|---|
| `probeHardware` | `bh_probe` |
| `scanDirectory` | `bh_scan` |
| `computeHashes` | `bh_job_start` + `bh_job_poll`（原生线程执行，流式阶段/字节进度） |
| `planRenames` | `bh_plan_renames`（纯计划，不落盘，复用已算好的哈希） |
| `executeRenames` | `bh_run_batch` + `apply_renames`（写 SQLite 日志） |
| `exportReport` | `bh_build_report` + `bh_export_report`，Dart 负责落盘 |
| `importReport` | `bh_import_report` |
| `diffReports` / `diffReportAgainstDirectory` | `bh_diff_reports` / `bh_diff_directory`（目录对比走任务协议，范围见下） |

### 目录对比的范围与上限

`DiffDirectoryRequest` 有三个决定「这次到底读多少」的字段，都不该由引擎替调用方猜：

- `recursive`（缺省 `false`）：是否进入子目录。报告里的 root 往往只是所选文件的公共父目录，
  默认递归会把整棵树算进去——这正是「选了一个文件却扫描整个目录」的原因。
- `restrict_paths`（缺省空）：只哈希这些根相对路径。缺省空表示「扫描到的都算」，
  所以只关心报告里那些文件的调用方**必须**显式传入，否则范围退回整个扫描结果。
- `max_entries`（缺省 50000）：条目上限，`0` 表示不限制。超限返回 `too_large`。

另有字节上限 `DIFF_SCAN_MAX_BYTES`（8 GiB）。哈希是流式的，内存不会涨，
但无上限的对比会持续压满磁盘与 CPU；上限让它在变成系统问题之前就失败。
| `cancelCurrentOperation` | `bh_job_cancel`（真正停止原生工作，而非只断开 Dart 订阅） |
| `loadHistory` / `undoBatch` | `bh_list_batches` / `bh_undo` |

`BatchHashApp` 的注入顺序是：显式传入的 service → `RustBatchHashService.tryCreate()` →
`FakeBatchHashService`。因此 widget 测试（没有原生库）仍然跑 fake，而打包后的应用自动走 Rust。

已知限制（诚实记录）：

- 旧版原生库（没有 `bh_job_*` 符号）仍会被加载，此时哈希走同步回退：结果正确，但没有中间进度，
  也无法中途取消。随包分发的库总是新版，回退只为开发期的库版本滞后兜底。
- `executeRenames` 复用 `run_batch` 的「扫描 → 计划 → 执行」路径，因此会重新读一遍文件；
  它通过 `restrict_paths` 把范围锁在预览计划里 `ready` 的行，所以「重新读」不会扩大范围，
  但执行与预览之间新增或消失的文件仍会体现为结果差异（消失的只是不出现，不会报错）。
- `probeRenderingHardware` 仍走平台层（Flutter 侧），不是 Rust；这是设计边界，不是遗漏。

## 校验方式

```sh
# 本地禁止执行；以下命令只用于说明 CI 做了什么
cargo test -p batchhash-ffi
cargo build -p batchhash-ffi --release
BATCHHASH_FFI_LIB=$PWD/target/release/libbatchhash_ffi.so \
  flutter test app/test/bridge --reporter expanded
```

CI 中该 job 名为 `Bridge (Rust FFI + Dart)`，覆盖：契约版本、算法表、扫描、哈希 + 报告 JSON、
写名 dry-run、写名执行 + 撤销、错误信封、硬件探测。