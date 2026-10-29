# BatchHash developer CLI

The CLI exists for CI smoke coverage and for debugging through CI-run jobs. It is
`never` a distribution artifact: the release workflow packages only the Flutter
apps and the Rust static libraries.

本地不执行构建（见 [README 的构建策略](../README.md)）。CLI 的所有验证都在 CI 内完成：
`cargo test -p batchhash-cli` 会真实运行二进制并断言其输出。

## 用法

```
batchhash <command> [options]
```

所有命令只接受显式声明的选项；未知选项会直接报错退出，避免拼写错误被静默忽略。

### hash

对文件或目录计算哈希并输出报告。

```sh
batchhash hash <path...> \
  --algo sha256,md5,blake3 \
  --format json|csv|markdown|txt|md5sum|bsd \
  --out report.json \
  --root <root-label> \
  --follow-links
```

- 目录会递归扫描；符号链接默认跳过，`--follow-links` 开启并做循环检测。
- 单个不可读目录不会中断整批任务，而是以 `failed` 条目出现在报告里。
- 默认算法为 `sha256,md5,blake3`；报告头记录 `hardware` 与 `algorithms`。
- 进度条只在 stderr 是终端时输出，`--quiet` 关闭。

### report

检测 → 导入 → 再导出，用于格式转换与规范化。

```sh
batchhash report report.json --to csv --out report.csv
batchhash report checksums.md5 --format md5sum --to json
```

### diff

比较两份报告（full outer join）。

```sh
batchhash diff left.json right.json --to markdown --out diff.md --case-sensitive
```

### scan-diff

报告 vs 实时目录：先按报告声明的算法重新计算目录哈希，再做比较。

```sh
batchhash scan-diff report.json /data/media --to json --out diff.json
```

扫描被中断时不会伪造「删除」：`finish(scan_completed=false)` 会把 diff 标记为
`incomplete`。

### suffix

文件名后缀写名的 dry-run、执行与撤销。

```sh
# 只出计划，不写盘
batchhash suffix plan /data/media --algo blake3 --length 5 --case upper

# 执行并写入 SQLite 日志（默认 <dir>/.batchhash-rename.db）
batchhash suffix apply /data/media --algo blake3 --length 8 --db rename.db

# 撤销最近一批（或指定 batch id）
batchhash suffix undo --db rename.db --apply
batchhash suffix undo --db rename.db --batch <batch-id>
```

- 计划阶段是纯函数：只读取文件系统，不改动任何文件。
- 执行阶段只调用 `rename(2)`，失败时**不会**退化为复制+删除。
- 日志状态机：`pending → applied → undone | conflict`；撤销时若目标名被占用或
  原文件已消失，该行标记为 `conflict` 而不是覆盖。
- `--prefix` 写入 `blake3:ABCDE` 形式（自描述 token）。

### extract

反向提取：只解析文件名后缀，不做任何哈希计算。

```sh
batchhash extract /data/media --out suffixes.json
```

输出 `parsed` / `not_suffixed` / `unparseable` 三类，`hex` 统一小写；
长度可唯一对应算法时给出 `algorithm`，否则 `ambiguous = true`。

解析时接受最长可达已注册摘要长度的 hex token（例如别人写入的 40 位 SHA-1），
而**写入**仍限制在 4..=16 位。

### probe

打印硬件可观测信息（JSON）。`blake3_backend_reported` / `sha256_backend_reported`
在库不暴露运行期后端时如实返回 `unknown`。

### algorithms / version / help

```sh
batchhash algorithms   # 16 个注册算法表
batchhash version
batchhash help
```

## 退出码

| 码 | 含义 |
|---:|---|
| 0 | 成功 |
| 1 | 运行失败（I/O、校验、导入、重命名等） |
| 2 | 用法错误（参数解析失败时由 process::exit 返回） |

## 与 UI 的关系

CLI 与 Flutter UI 共享同一批引擎 crate，CLI 只是薄适配层：报告导出、diff 语义、
后缀计划与 SQLite 日志的行为完全一致，因此 CLI 的端到端测试同时覆盖了 UI 将要调用的
核心路径。
