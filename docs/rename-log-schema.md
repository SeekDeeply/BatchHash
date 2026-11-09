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
pending -> conflict (failed / not_applied / ambiguous)
~~~

- 执行成功：applied。
- 执行失败：result = failed，记录 error_kind，不进入 applied。
- 已完成计划但未执行（取消或后端提前停止）：result = not_attempted、
  error_kind = rename_not_attempted、state = conflict。绝不能保持 pending。
- 撤销成功：undone + undone_at。
- 撤销冲突：conflict，逐条报告，不强行覆盖。

## pending 状态回收（崩溃窗口）

一次重命名在两个时刻之间落盘：写日志时先插入 `pending` 行，后端确认改名成功后
再写 `applied`。进程若在这个窗口内崩溃（或日志写入失败），该行会停留在
`pending`，此时**仅凭日志无法判断文件是否已被改名**。

打开日志存储时要先执行一次回收（`reconcile_pending_operations`）：只扫描
`pending` 行，逐条比对文件系统，然后写入结论：

| 文件系统 | 结论 | 写入的 state |
|---|---|---|
| 新路径存在，原路径不存在 | 改名确实发生 | applied（可撤销） |
| 原路径存在，新路径不存在 | 改名从未发生 | conflict（result = not_applied） |
| 两者都存在，或两者都不存在 | 无法判定 | conflict（result = ambiguous） |

- 回收只处理 `pending` 行，已记录为 applied / undone / conflict 的行永不被改写，
  因此每次打开日志都可安全执行，且幂等。
- 无法判定的一律按冲突处理，绝不猜测；不会为了让撤销「看起来成功」而冒险。
- 回收未完成的行（取消导致中断）下次打开继续处理。

## 撤销规则（跨会话）

- 按 batch 逆序（最新批次优先），批次内操作逆序。
- 撤销前逐文件检测：
  - 新路径不存在（文件被移动/删除）-> conflict。
  - 原路径已存在（名称被占用）-> conflict。
  - 否则可撤销。
- 撤销使用原操作记录的新路径 -> 原路径；不猜测、不模糊匹配。
- 检测执行两次：一次在生成撤销计划时，一次在**真正改名之前**立刻重读文件系统。
  计划是快照，两次检测之间文件名可能被占用；第二次检测失败则跳过该行并报告
  冲突原因，保持 applied 状态，不移动任何文件。
- 反向改名同样使用「不覆盖」语义（Linux 上为 `renameat2(RENAME_NOREPLACE)`），
  即使第二次检测与改名之间有人抢先创建了原路径，也不会覆盖它。
- Android：使用 persisted URI permission；权限失效逐条标记 platform_access_lost。
- iOS：security-scoped bookmark；访问失效逐条报告冲突。

## 不变量

- 日志写入使用事务；崩溃后状态可恢复。
- 完整哈希与截断后缀同时保存；撤销不依赖后缀。
- 任何写操作不得覆盖已有文件（冲突默认跳过）。
- 「不覆盖」不只由计划期冲突检测保证：执行期一律走不覆盖改名
  （`RenameBackend::rename_no_replace`，Linux 上为 `renameat2 + RENAME_NOREPLACE`，
  其余平台尽力而为或依赖系统 `rename` 的失败）。计划之后才出现的同名文件
  不会被静默替换。
- 没有 `pending` 行会静默消失：撤销计划把它们作为冲突暴露出来，
  回收把能判定的一次写完，判定不了的一律记为 conflict。