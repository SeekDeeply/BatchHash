# PathNormalizer 规范

对比准确性取决于"两条记录是否被认为是同一个文件"。归一化必须确定性、跨平台一致、可测试。
PathNormalizer 独立于 Diff Engine，单独单测 + golden test。

## 归一化流水线

| 步骤 | 处理 | 说明 |
|---|---|---|
| 1 | 取相对 root 的路径 | 报告已存相对路径；实时目录先剥离 root |
| 2 | 解析 .. 与 . 段 | 消除 sub/../a；越界到 root 外标 out_of_root |
| 3 | 分隔符统一为 / | Windows 反斜杠 -> / |
| 4 | 去除首尾 / 与重复 / | //sub///a.mkv -> sub/a.mkv |
| 5 | Unicode NFC 规范化 | macOS NFD 统一到 NFC |
| 6 | 大小写处理 | 按用户策略统一 |
| 7 | to_lowercase 后再次 NFC | 保证大小写转换后稳定 |
| 8 | （可选）仅 basename | 降级模式 |
| 9 | 生成最终 key | 字符串 |

**不做**：符号链接解析、忽略扩展名、模糊匹配、NFKC 全角半角归一化。

## 大小写敏感策略

| 平台 | 文件系统默认 | BatchHash 默认 |
|---|---|---|
| Windows | 不敏感 | 不敏感 |
| macOS | 不敏感 | 不敏感 |
| Linux | 敏感 | 敏感 |
| Android | 依分区 | 不敏感 |
| iOS | 不敏感 | 不敏感 |

- 对比对话框提供开关；默认取两侧报告 case_sensitive 的逻辑或。
- 不敏感模式统一使用 Unicode-aware to_lowercase()，之后再次 NFC。
- 同一报告内两个原始路径映射到同一 key -> duplicate_key，整组单列，不自动配对。
- 文档明确：to_lowercase() 与 NTFS upcase table 不等价，这是跨平台一致性取舍。

## 仅按文件名降级模式

- key = basename，同样 NFC + 大小写归一化。
- 同名不同目录会误配，UI 顶部显著提示。
- basename 在任一侧出现多次 -> ambiguous，单列歧义组，不自动判 changed/unchanged。

## 必测用例

| 用例 | 左 | 右 | 期望 |
|---|---|---|---|
| 分隔符 | sub\a.mkv | sub/a.mkv | 同一 key |
| 点段 | ./sub/../sub/a.mkv | sub/a.mkv | 同一 key |
| 重复斜杠 | sub//a.mkv | sub/a.mkv | 同一 key |
| Unicode NFC/NFD | café(NFC) | café(NFD) | 同一 key |
| 大小写不敏感 | A.MKV | a.mkv | 同一 key |
| 大小写敏感 | A.MKV | a.mkv | 不同 key |
| 冲突 | A.mkv + a.mkv（同报告，不敏感） | — | duplicate_key |
| 仅文件名歧义 | x/a.mkv + y/a.mkv | a.mkv | ambiguous |
| 全角/半角 | ａ.mkv | a.mkv | 不同 key（不做 NFKC） |
| 越界点段 | ../a.mkv | — | out_of_root |

## root 不一致处理

| 场景 | 策略 | 默认 |
|---|---|---|
| root 相同 | 直接比相对路径 | 自动 |
| root 不同，用户指定共同根 | 重新计算相对路径；不在根下 -> out_of_root | 默认弹窗三选一 |
| root 不同，仅按文件名 | 降级 basename 对比 | 可选 |
| root 不同，按绝对路径 | 仅同机有意义 | 可选，明确警告 |

UI 不允许静默继续。