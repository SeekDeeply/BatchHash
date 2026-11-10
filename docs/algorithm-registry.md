# AlgorithmRegistry

全部 44 个算法已注册化。本文件是注册表契约，代码实现为 crates/batchhash-types/src/algorithm.rs。

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
| category | 分类：`crypto` / `checksum` / `fast` / `legacy`，供 UI 分组；FFI 原样透传给 Dart |

## 调用方必须使用 stable_id

`stable_id` 是唯一的传输与序列化契约：FFI 的 `algorithms` 数组、报告 JSON、CSV 列头、
后缀前缀全部按它匹配，`AlgorithmId::from_stable_id` 找不到就直接返回
`invalid_request`，**整批请求失败**——一个名字写错不会退化成"少算一个算法"。

因此界面里的算法列表不能自造别名。曾经界面写 `xxh3`、引擎只认 `xxh3-64`，
结果整批计算报 `invalid_request`，看起来像"xxh3 不能用"。
界面不再硬编码算法清单：`BatchHashService.listAlgorithms()` 从 `bh_algorithms` 取全量
目录（含分类），`kAvailableAlgorithms` / `kSuffixAlgorithms` 只作为 Demo 与回退常量。
`app/test/bridge/rust_service_test.dart` 会用服务下发的每个 ID 真算一遍，把这个契约钉死。

## 分类（category）

四个分类，由 `AlgorithmCategory` 定义、`AlgorithmSpec.category` 承载：

| 分类 | 含义 | 成员 |
|---|---|---|
| `crypto` | 现代通用摘要，可用于完整性/安全用途 | SHA-2 / SHA-3 / BLAKE2 / BLAKE3 / Keccak / SHAKE / RIPEMD-160+ / SM3 / Streebog / Whirlpool / Tiger / Skein / Grøstl / JH 等 |
| `checksum` | 错误检测校验和，无安全承诺 | CRC32、CRC32C |
| `fast` | 高吞吐非密码学哈希 | XXH3-64、XXH3-128、XXH64 |
| `legacy` | 已损坏或被取代，仅为兼容保留 | MD2、MD4、MD5、SHA-1、RIPEMD-128、GOST R 34.11-94 |

规则：

- 分类是**单一真源**的一部分：加算法必须同时决定分类，否则编译不过（`REGISTRY` 是定长数组）。
- 界面按 `AlgorithmCategory.ALL` 的顺序分组，未知取值回退到 `legacy`，不会让选择器崩掉。
- 损坏摘要只能落在 `legacy`，不能出现在 `crypto` 分组里冒充安全算法。

## 注册表（规范顺序）

报告算法数组与 CSV 算法列严格按此顺序序列化。顺序变更属于 schema 破坏性变更。

| # | stable_id | 显示名 | hex 长度 | 分类 | 密码学 | 写名 | 长度歧义 |
|---|---|---|---|---|---|---|---|
| 1 | md5 | MD5 | 32 | legacy | 否 | 是（警告） | 是 |
| 2 | sha1 | SHA-1 | 40 | legacy | 否 | 是（警告） | 是 |
| 3 | sha224 | SHA-224 | 56 | crypto | 是 | 是 | 是 |
| 4 | sha256 | SHA-256 | 64 | crypto | 是 | 是 | 是 |
| 5 | sha384 | SHA-384 | 96 | crypto | 是 | 是 | 是 |
| 6 | sha512 | SHA-512 | 128 | crypto | 是 | 是 | 是 |
| 7 | sha3-256 | SHA3-256 | 64 | crypto | 是 | 是 | 是 |
| 8 | sha3-512 | SHA3-512 | 128 | crypto | 是 | 是 | 是 |
| 9 | blake2b | BLAKE2b-512 | 128 | crypto | 是 | 是 | 是 |
| 10 | blake2s | BLAKE2s-256 | 64 | crypto | 是 | 是 | 是 |
| 11 | blake3 | BLAKE3 | 64 | crypto | 是 | 是（默认） | 是 |
| 12 | xxh3-64 | XXH3-64 | 16 | fast | 否 | 是（警告） | 是 |
| 13 | xxh3-128 | XXH3-128 | 32 | fast | 否 | 是（警告） | 是 |
| 14 | xxh64 | XXH64 | 16 | fast | 否 | 是（警告） | 是 |
| 15 | crc32 | CRC32 | 8 | checksum | 否 | 是（警告） | 是 |
| 16 | crc32c | CRC32C | 8 | checksum | 否 | 是（警告） | 是 |
| 17 | md2 | MD2 | 32 | legacy | 否 | 是（警告） | 是 |
| 18 | md4 | MD4 | 32 | legacy | 否 | 是（警告） | 是 |
| 19 | sha512-224 | SHA-512/224 | 56 | crypto | 是 | 否 | 是 |
| 20 | sha512-256 | SHA-512/256 | 64 | crypto | 是 | 否 | 是 |
| 21 | sha3-224 | SHA3-224 | 56 | crypto | 是 | 否 | 是 |
| 22 | sha3-384 | SHA3-384 | 96 | crypto | 是 | 否 | 是 |
| 23 | keccak224 | Keccak-224 | 56 | crypto | 是 | 否 | 是 |
| 24 | keccak256 | Keccak-256 | 64 | crypto | 是 | 否 | 是 |
| 25 | keccak384 | Keccak-384 | 96 | crypto | 是 | 否 | 是 |
| 26 | keccak512 | Keccak-512 | 128 | crypto | 是 | 否 | 是 |
| 27 | shake128 | SHAKE128 | 32 | crypto | 是 | 否 | 是 |
| 28 | shake256 | SHAKE256 | 64 | crypto | 是 | 否 | 是 |
| 29 | ripemd128 | RIPEMD-128 | 32 | legacy | 否 | 是（警告） | 是 |
| 30 | ripemd160 | RIPEMD-160 | 40 | crypto | 是 | 否 | 是 |
| 31 | ripemd256 | RIPEMD-256 | 64 | crypto | 是 | 否 | 是 |
| 32 | ripemd320 | RIPEMD-320 | 80 | crypto | 是 | 否 | 否 |
| 33 | sm3 | SM3 | 64 | crypto | 是 | 否 | 是 |
| 34 | gost94 | GOST R 34.11-94 | 64 | legacy | 否 | 是（警告） | 是 |
| 35 | streebog256 | Streebog-256 | 64 | crypto | 是 | 否 | 是 |
| 36 | streebog512 | Streebog-512 | 128 | crypto | 是 | 否 | 是 |
| 37 | whirlpool | Whirlpool | 128 | crypto | 是 | 否 | 是 |
| 38 | tiger | Tiger | 48 | crypto | 是 | 否 | 否 |
| 39 | skein256 | Skein-256 | 64 | crypto | 是 | 否 | 是 |
| 40 | skein512 | Skein-512 | 128 | crypto | 是 | 否 | 是 |
| 41 | groestl256 | Grøstl-256 | 64 | crypto | 是 | 否 | 是 |
| 42 | groestl512 | Grøstl-512 | 128 | crypto | 是 | 否 | 是 |
| 43 | jh256 | JH-256 | 64 | crypto | 是 | 否 | 是 |
| 44 | jh512 | JH-512 | 128 | crypto | 是 | 否 | 是 |

## 长度推断与歧义

| hex 长度 | 候选算法 |
|---|---|
| 8 | crc32, crc32c |
| 16 | xxh3-64, xxh64 |
| 32 | md5, xxh3-128, md2, md4, shake128, ripemd128 |
| 40 | sha1, ripemd160 |
| 48 | tiger |
| 56 | sha224, sha512-224, sha3-224, keccak224 |
| 64 | sha256, sha3-256, blake2s, blake3, sha512-256, keccak256, shake256, ripemd256, sm3, gost94, streebog256, skein256, groestl256, jh256 |
| 80 | ripemd320 |
| 96 | sha384, sha3-384, keccak384 |
| 128 | sha512, sha3-512, blake2b, keccak512, streebog512, whirlpool, skein512, groestl512, jh512 |

只有 `48`（Tiger）和 `80`（RIPEMD-320）是单候选；其余长度都存在歧义。

- 单候选：可自动推断。
- 多候选：必须标记歧义，要求用户选择算法或启用短前缀。
- 后缀解析支持可选前缀形式：原名[sha256:ABCDE].ext；前缀默认关闭。

## 写名规则

- 写名算法唯一，报告记录 suffix_algorithm。
- 默认算法：BLAKE3（`default_suffix_algorithm = true`，全表唯一）。
- 非密码学算法（MD5/SHA-1/XXH/CRC）写名前必须警告：截断后缀不是安全摘要。
- 是否可写名由 `suffix_usable` 决定：44 个算法里只有前 18 个（到 MD4）可写名，
  其余较长摘要（SHA-512/224、Keccak、SHAKE、Streebog、Whirlpool、Skein、Grøstl、JH 等）
  只能作为内容摘要使用。UI 的后缀选择器必须按该字段过滤，不能按分类猜。
- 已包含可解析后缀的文件默认跳过并标记，不嵌套、不覆盖。

## 不变量

- 所有哈希输出小写十六进制。
- stable_id 唯一且不可重命名。
- 规范顺序 = AlgorithmId::ALL 顺序（有测试逐项比对）。
- 每个算法恰好属于一个 `category`；`crypto` 组内不允许出现 `cryptographic = false` 的算法。
- 注册表长度 = 44，`AlgorithmId::ALL` 长度 = 44，测试钉死两者一致。
- 报告导入拒绝未声明的算法列/键；CSV 列序按注册表规范顺序。