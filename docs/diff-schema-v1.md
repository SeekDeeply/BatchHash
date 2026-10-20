# Diff schema v1

对比结果导出文档。schema_version = 1；导入端拒绝未来版本。

## 顶层字段

| 字段 | 类型 | 说明 |
|---|---|---|
| schema_version | integer | 固定 1 |
| tool / tool_version | string | 生成工具 |
| generated_at | string | UTC ISO8601 |
| strategy | object | 对比策略（必须随结果保存，否则不可复现） |
| summary | object | 各状态计数 + incomplete |
| rows | object[] | 对齐后的行 |

## strategy

| 字段 | 取值 | 说明 |
|---|---|---|
| mode | root_relative / basename / absolute | 对比 key 策略 |
| case_sensitive | boolean | 用户选择的大小写策略 |
| root_alignment | same_root / user_common_root / basename_only / absolute | root 对齐方式 |
| algorithms | string[] | 两侧声明的算法（规范顺序） |
| algorithms_compared | string[] | 实际参与比较的交集 |
| left / right | object | kind（report / realtime_directory / checksum_file）、label、generated_at、scan_time |

## row

| 字段 | 说明 |
|---|---|
| key | 归一化对比 key（basename 模式下为文件名） |
| status | 见下表 |
| reason | 可读原因（策略/错误/不匹配摘要） |
| algorithms_compared | 本行参与比较的算法 |
| changed_algorithms | 实际不一致的算法 |
| left_status / right_status | 两侧条目状态 |
| left / right | 条目快照（path、size、mtime、status、hashes、error） |

## 状态

| 状态 | 条件 |
|---|---|
| unchanged | 两侧都有，共同算法哈希全部一致 |
| changed | 两侧都有，至少一个共同算法哈希不同 |
| added | 仅右侧有 |
| removed | 仅左侧有；取消导致未扫描时不得批量标记 |
| unknown | 一侧或两侧 status 非 ok |
| ambiguous | 仅按文件名模式下 basename 冲突 |
| duplicate_key | 单侧内部归一化 key 冲突（整组单列，不自动配对） |
| out_of_root | 指定共同根后路径不在根下 |
| corrupt | 大小不同但哈希相同（数据异常，高亮） |
| unstable | 实时扫描期间文件被修改 |

规则：

- 算法交集作为比较基准；交集为空降级为仅比较清单（存在/缺失/大小变化）并明确提示。
- hardware 字段不参与差异判定。
- 排序：归一化 key 的 Unicode code point 字典序，tie-breaker 为原始路径，保证 golden 逐字节可复现。
- 实时对比：每完成一个文件哈希立即产出行；扫描正常结束后才批量标记 removed；
  取消/部分扫描时 summary.incomplete = true，且不得产生虚假 removed。

## 示例

~~~json
{
  "schema_version": 1,
  "tool": "BatchHash",
  "tool_version": "0.1.0",
  "generated_at": "2026-10-20T12:00:00Z",
  "strategy": {
    "mode": "root_relative",
    "case_sensitive": false,
    "root_alignment": "same_root",
    "algorithms": ["sha256", "blake3"],
    "algorithms_compared": ["sha256", "blake3"],
    "left": { "kind": "report", "label": "old.json", "generated_at": "2026-10-19T08:00:00Z", "scan_time": null },
    "right": { "kind": "realtime_directory", "label": "/data/media", "generated_at": null, "scan_time": "2026-10-20T12:00:00Z" }
  },
  "summary": {
    "total": 4, "unchanged": 1, "changed": 1, "added": 1, "removed": 1,
    "unknown": 0, "ambiguous": 0, "duplicate_key": 0, "out_of_root": 0,
    "corrupt": 0, "unstable": 0, "incomplete": false
  },
  "rows": [
    { "key": "a.mkv", "status": "unchanged", "reason": "all common algorithms match", "algorithms_compared": ["sha256", "blake3"], "changed_algorithms": [], "left_status": "ok", "right_status": "ok", "left": null, "right": null }
  ]
}
~~~

## 导出格式

支持 JSON / CSV / Markdown / TXT；md5sum / BSD 对 diff 无意义，导出时报错。
