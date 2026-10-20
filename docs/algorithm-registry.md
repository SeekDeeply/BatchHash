# AlgorithmRegistry

16 个算法全部注册化。本文件是注册表契约，代码实现为 crates/batchhash-types/src/algorithm.rs。

## 字段

| 字段 | 含义 |
|---|---|
| stable_id | 报告/CSV/后缀算法使用的稳定 ID（小写，含连字符） |
| display_name | UI 显示名 |
| hex_len / digest_len | 十六进制长度 / 字节长度 |
| cryptographic | 是否密码学安全 |
| suffix_usable | 是否可用于写名 |
| suffix_warning | 用于写名时的安全提示 |
| suffix_prefix | 可选短前缀（默认关闭） |
| ambiguous_by_length | 该长度是否对应多个算法（推断歧义） |
| default_suffix_algorithm | 是否默认写名算法（BLAKE3） |

## 注册表（规范顺序）

报告算法数组与 CSV 算法列严格按此顺序序列化。顺序变更属于 schema 破坏性变更。

| # | stable_id | 显示名 | hex 长度 | 密码学 | 写名 | 长度歧义 |
|---|---|---|---|---|---|---|
| 1 | md5 | MD5 | 32 | 否 | 是（警告） | 是（与 xxh3-128） |
| 2 | sha1 | SHA-1 | 40 | 否 | 是（警告） | 否 |
| 3 | sha224 | SHA-224 | 56 | 是 | 是 | 否 |
| 4 | sha256 | SHA-256 | 64 | 是 | 是 | 是 |
| 5 | sha384 | SHA-384 | 96 | 是 | 是 | 否 |
| 6 | sha512 | SHA-512 | 128 | 是 | 是 | 是 |
| 7 | sha3-256 | SHA3-256 | 64 | 是 | 是 | 是 |
| 8 | sha3-512 | SHA3-512 | 128 | 是 | 是 | 是 |
| 9 | blake2b | BLAKE2b-512 | 128 | 是 | 是 | 是 |
| 10 | blake2s | BLAKE2s-256 | 64 | 是 | 是 | 是 |
| 11 | blake3 | BLAKE3 | 64 | 是 | 是（默认） | 是 |
| 12 | xxh3-64 | XXH3-64 | 16 | 否 | 是（警告） | 是（与 xxh64） |
| 13 | xxh3-128 | XXH3-128 | 32 | 否 | 是（警告） | 是（与 md5） |
| 14 | xxh64 | XXH64 | 16 | 否 | 是（警告） | 是（与 xxh3-64） |
| 15 | crc32 | CRC32 | 8 | 否 | 是（警告） | 是（与 crc32c） |
| 16 | crc32c | CRC32C | 8 | 否 | 是（警告） | 是（与 crc32） |

## 长度推断与歧义

| hex 长度 | 候选算法 |
|---|---|
| 8 | crc32, crc32c |
| 16 | xxh3-64, xxh64 |
| 32 | md5, xxh3-128 |
| 40 | sha1 |
| 56 | sha224 |
| 64 | sha256, sha3-256, blake2s, blake3 |
| 96 | sha384 |
| 128 | sha512, sha3-512, blake2b |

- 单候选：可自动推断。
- 多候选：必须标记歧义，要求用户选择算法或启用短前缀。
- 后缀解析支持可选前缀形式：原名[sha256:ABCDE].ext；前缀默认关闭。

## 写名规则

- 写名算法唯一，报告记录 suffix_algorithm。
- 默认算法：BLAKE3。
- 非密码学算法（MD5/SHA-1/XXH/CRC）写名前必须警告：截断后缀不是安全摘要。
- 已包含可解析后缀的文件默认跳过并标记，不嵌套、不覆盖。

## 不变量

- 所有哈希输出小写十六进制。
- stable_id 唯一且不可重命名。
- 规范顺序 = AlgorithmId::ALL 顺序。
- 报告导入拒绝未声明的算法列/键；CSV 列序按注册表规范顺序。
