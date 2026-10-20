# Report schema v1

报告是 BatchHash 的机器可读主产物。schema_version = 1；导入端拒绝未来版本。

## JSON 顶层字段

| 字段 | 类型 | 说明 |
|---|---|---|
| schema_version | integer | 固定 1 |
| tool | string | 固定 BatchHash |
| tool_version | string | 生成工具版本 |
| generated_at | string | UTC ISO8601 |
| root | string | 批处理根（文件系统绝对路径或 SAF tree 展示路径） |
| root_kind | fs / saf | 根类型 |
| case_sensitive | boolean | 生成时平台建议值；对比以用户选择为准 |
| algorithms | string[] | 算法稳定 ID，按 AlgorithmRegistry 规范顺序 |
| suffix_config | object | length / case / enabled / algorithm |
| hardware | object | 硬件可观测性元数据（不参与差异判定） |
| summary | object | total / ok / failed / skipped / total_bytes / truncated |
| entries | object[] | 条目列表 |

## Entry 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| path | string | 相对 root，统一 / 分隔 |
| size | integer | 字节数 |
| mtime | string/null | UTC ISO8601 |
| hashes | object | 算法稳定 ID -> 完整小写十六进制哈希 |
| status | ok / failed / skipped | 处理状态 |
| error_kind | string/null | 失败/跳过原因枚举 |
| error_message | string/null | 可读错误 |
| suffix_applied | string/null | 写入的截断后缀（不是安全摘要） |
| suffix_algorithm | string/null | 产生后缀的算法 |
| renamed_to | string/null | 重命名后的相对路径 |

规则：

- 同时保存完整哈希与截断后缀；对比永远使用完整哈希。
- path 不得为绝对路径、不得含反斜杠、不得逃逸 root（导入端强制校验）。
- 大报告流式序列化；导入限制 MAX_IMPORT_ENTRIES 与 MAX_IMPORT_BYTES。
- 硬件字段 round-trip 保持一致，不参与差异判定。

## 示例

~~~json
{
  "schema_version": 1,
  "tool": "BatchHash",
  "tool_version": "0.1.0",
  "generated_at": "2026-10-20T12:00:00Z",
  "root": "/data/media",
  "root_kind": "fs",
  "case_sensitive": false,
  "algorithms": ["sha256", "blake3"],
  "suffix_config": { "length": 5, "case": "upper", "enabled": true, "algorithm": "blake3" },
  "hardware": {
    "arch": "x86_64",
    "detected_features": ["SSE4.1", "AVX2", "SHA-NI"],
    "backend_policy": "auto",
    "blake3_backend_reported": "avx2",
    "sha256_backend_reported": "sha-ni",
    "source": "cpufeatures",
    "multithread": 8
  },
  "summary": { "total": 1, "ok": 1, "failed": 0, "skipped": 0, "total_bytes": 1048576, "truncated": false },
  "entries": [
    {
      "path": "sub/a.mkv",
      "size": 1048576,
      "mtime": "2026-10-01T10:00:00Z",
      "hashes": { "sha256": "…64 hex…", "blake3": "…64 hex…" },
      "status": "ok",
      "error_kind": null,
      "error_message": null,
      "suffix_applied": "A1B2C",
      "suffix_algorithm": "blake3",
      "renamed_to": "sub/a[A1B2C].mkv"
    }
  ]
}
~~~

## CSV

固定列，首行 header：

~~~text
path,size,mtime,status,error_kind,suffix_applied,suffix_algorithm,renamed_to,<算法列按 registry 顺序>
~~~

- 算法列动态追加但按 AlgorithmRegistry 规范顺序。
- RFC 4180 转义；默认 UTF-8 无 BOM、LF 换行；导出对话框可选 Excel 兼容 BOM。
- 可选 CSV 公式注入防护（以 = + - @ 开头的单元格前置单引号）。
- round-trip 必须逐字节一致。

## Markdown

- 头部元数据表 + Summary 表 + Entries 表。
- 条目 > 500 默认截断为前 500 行，写 truncated: true，并提示完整明细见 JSON/CSV。
- 仅导出，不作为导入/对比输入。

## TXT

- 纯文本、等宽友好、可打印、可粘贴；字段左对齐、冒号对齐。
- 头部：Tool Version / Generated / Root / Root Kind / Algorithms / Suffix / Hardware。
- Summary 段 + Entries 段；条目 > 500 截断并写 Truncated: true。
- UTF-8 无 BOM、LF；仅导出。

## md5sum / BSD

- md5sum：小写十六进制 + 两个空格 + 路径，兼容 coreutils；单算法报告。
- BSD：ALGO (path) = hex，兼容 BSD/macOS 工具；单算法报告。
- 导入时按摘要长度推断单一算法；长度歧义必须报错并提供候选。
- 校验和文件没有 root 概念：导入时 root = ""、root_kind = fs、case_sensitive = false。

## 导入校验

1. 拒绝 schema_version > 1。
2. 拒绝路径逃逸、绝对路径、反斜杠、NUL。
3. 限制条目数（2,000,000）与字节数（512 MiB）。
4. 拒绝未声明的算法键 / 列、长度不匹配的哈希。
5. 拒绝无哈希的 ok 条目。

## Round-trip 契约

JSON 与 CSV：export -> import -> export 必须逐字节一致（算法列序、换行、BOM 策略固定）。
Markdown / TXT 只验证导出不崩溃（不作为导入输入）。
