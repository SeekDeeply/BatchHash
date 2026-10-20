# 文件条目、root 与稳定标识

## 统一文件条目

| 字段 | 说明 |
|---|---|
| root_id | 本次批处理根标识 |
| root_kind | fs / saf |
| relative_path | 相对 root 的 / 分隔路径（报告存储字段） |
| display_path | UI 展示路径 |
| platform_meta | Android document URI、iOS bookmark 等审计信息，不参与默认对比 key |
| size | 字节数 |
| mtime | UTC ISO8601 |
| hashes | 算法稳定 ID -> 完整十六进制哈希 |
| status | ok / failed / skipped |
| error_kind | 失败/跳过原因枚举 |
| error_message | 可读错误信息 |

## 单根规则

- 单文件：root = 父目录。
- 单目录：root = 所选目录。
- 多选同根：root = 共同父目录。
- 多选跨目录：强制用户选择共同根；不在根下文件标记 out_of_root。
- Android：root = SAF tree；报告 path 存相对 tree 的逻辑路径。
- 符号链接默认不跟随；实时扫描遇到符号链接跳过并标记。

## 错误枚举

permission_denied / not_found / is_directory / io_error / unsupported / cancelled / unstable /
out_of_root / suffix_exists / invalid_name / rename_unsupported / rename_failed /
platform_access_lost / hash_mismatch / parse_error / other。

## 平台输入

- 桌面 / iOS：路径输入，Rust 打开文件。
- Android：SAF 获取 ParcelFileDescriptor，传 fd + offset + length。
- Rust 侧必须：dup() 或 borrowed fd 语义；seek(offset)；仅读取限定长度；不关闭 Flutter 侧原 fd；
  不假设起始 offset 为 0。

## 报告路径校验（导入端）

- 非空；无 NUL；无反斜杠；非绝对路径；无盘符前缀。
- ../ 段不得使深度为负（a/../b 允许，../b 拒绝）。
- 同一报告内相对路径必须唯一（重复由 Diff 标 duplicate_key）。