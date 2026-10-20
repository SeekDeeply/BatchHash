# 重命名日志 schema 与撤销状态机

重命名默认不执行；仅在用户勾选后：dry-run -> 冲突检测 -> 执行 -> 持久化日志 -> 跨会话撤销。
日志使用 SQLite（app 私有目录）。

## 表

### batch

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | batch_id |
| created_at | TEXT | UTC ISO8601 |
| root | TEXT | 批处理根 |
| root_kind | TEXT | fs / saf |
| algorithms | TEXT | JSON 数组（写名算法等） |
| suffix_length | INTEGER | 4..16 |
| suffix_case | TEXT | upper / lower |
| suffix_algorithm | TEXT | 稳定算法 ID |
| status | TEXT | pending / applied / undone / partial / cancelled |

### rename_operation

| 列 | 类型 | 说明 |
|---|---|---|
| id | INTEGER PK AUTOINCREMENT | 操作 ID |
| batch_id | TEXT FK -> batch(id) | 所属批次 |
| original_path | TEXT | 原路径/URI |
| new_path | TEXT | 新路径/URI |
| algorithm | TEXT | 写名算法 |
| full_hash | TEXT | 完整哈希（绝不只存截断后缀） |
| suffix | TEXT | 截断后缀 |
| op_type | TEXT | rename / android_rename |
| result | TEXT | ok / failed / skipped |
| error_kind | TEXT | 失败原因枚举 |
| created_at | TEXT | UTC ISO8601 |
| undone_at | TEXT | 撤销时间 |
| state | TEXT | pending / applied / undone / conflict |

### schema_migration

| 列 | 类型 | 说明 |
|---|---|---|
| version | INTEGER PK | 迁移版本 |
| applied_at | TEXT | 应用时间 |

## 状态机

~~~text
pending -> applied -> undone
                    \-> conflict
pending -> failed (result)
~~~

- 执行成功：applied。
- 执行失败：result = failed，记录 error_kind，不进入 applied。
- 撤销成功：undone + undone_at。
- 撤销冲突：conflict，逐条报告，不强行覆盖。

## 撤销规则（跨会话）

- 按 batch 逆序（最新批次优先），批次内操作逆序。
- 撤销前逐文件检测：
  - 新路径不存在（文件被移动/删除）-> conflict。
  - 原路径已存在（名称被占用）-> conflict。
  - 否则可撤销。
- 撤销使用原操作记录的新路径 -> 原路径；不猜测、不模糊匹配。
- Android：使用 persisted URI permission；权限失效逐条标记 platform_access_lost。
- iOS：security-scoped bookmark；访问失效逐条报告冲突。

## 不变量

- 日志写入使用事务；崩溃后状态可恢复。
- 完整哈希与截断后缀同时保存；撤销不依赖后缀。
- 任何写操作不得覆盖已有文件（冲突默认跳过）。