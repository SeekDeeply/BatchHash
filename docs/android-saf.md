# Android SAF 集成

## 职责边界

| 层 | 职责 |
|---|---|
| Rust Rename Planner | dry-run 计划、冲突检测、日志模型、撤销计划 |
| Kotlin SAF Backend | DocumentsContract.renameDocument 执行、逐文件结果回传 |
| Flutter | 持久化 SAF 权限、调用原生重命名、展示逐条结果 |

## 选择目录

- ACTION_OPEN_DOCUMENT_TREE 获取 tree URI。
- 必须调用 takePersistableUriPermission 保存跨会话权限。
- root_kind = saf；报告 path 存相对 tree 的逻辑路径。

## fd 哈希输入

- SAF 获取 ParcelFileDescriptor。
- 传给 Rust 的抽象：fd + offset + length。
- Rust 必须：
  1. dup() fd（或用 borrowed fd 语义）；
  2. seek(offset)；
  3. 仅读取限定长度；
  4. 不关闭 Flutter 侧原 fd；
  5. 不假设起始 offset 为 0。
- 单测覆盖：offset 非 0、限定长度、原 fd 调用后仍可用。

## 重命名

- 执行前逐文件检查 DocumentsContract.Document.FLAG_SUPPORTS_RENAME，但该标志仅作预判。
- 实际执行由 Kotlin 调用 renameDocument；必须处理运行时失败（SecurityException、IllegalArgumentException 等）。
- 不支持或执行失败 -> **仅校验、不写名，结果明确标记原因**。
- **不做"复制再删"替代方案。**
- 结果逐文件回传（ok / error_kind / message），写入 Rust 日志模型。

## 持久化

- Save persisted URI permission 供跨会话撤销。
- 撤销前检测权限是否仍有效；失效标记 platform_access_lost，不静默失败。

## 平台限制

- 不同 DocumentsProvider 对 rename 支持差异大，必须以运行时结果为准。
- SAF 下路径大小写依分区，报告记录 root_kind 与逻辑路径。