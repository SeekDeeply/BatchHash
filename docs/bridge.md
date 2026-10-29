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

`bh_run_batch` 的响应包含：汇总计数、条目（相对路径、显示路径、大小、mtime、状态、错误类型、哈希数组）、
扫描跳过的路径、写名计划行与汇总、`batch_id`、重命名成功/失败数、是否取消、耗时，
以及 **canonical report JSON**（`report_json`），UI 可以直接拿去做导出或对比。

## Dart 侧

`BatchHashNative.tryOpen()` 的查找顺序：

1. 环境变量 `BATCHHASH_FFI_LIB`（CI 与测试用）；
2. iOS：`DynamicLibrary.process()`（静态库链接进进程）；
3. 其它平台：`libbatchhash_ffi.so` / `libbatchhash_ffi.dylib` / `batchhash_ffi.dll`。

找不到库时返回 `null`，调用方可以回退到 `FakeBatchHashService`（P0 的 UI 测试就是这么跑的）。
业务错误统一抛 `BridgeException(kind, message)`，未知算法、空算法列表、缺日志路径都是 `invalid_request`。

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
