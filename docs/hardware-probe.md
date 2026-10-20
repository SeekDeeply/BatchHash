# HardwareProbe 与硬件加速可观测性

目标是"可观测性分级"：记录检测特性、策略、库报告后端；允许 unknown；不虚假精确。

## 字段

| 字段 | 取值 | 来源 |
|---|---|---|
| arch | x86_64 / aarch64 / 其他 | std::env::consts::ARCH |
| detected_features | SSE2 / SSE4.1 / AVX2 / AVX-512 / SHA-NI / NEON / SHA2 等 | cpufeatures / core 检测宏 |
| backend_policy | auto / forced-soft | 用户设置 |
| blake3_backend_reported | avx2 / avx512 / neon / portable / unknown | 库运行时探测或推断 |
| sha256_backend_reported | sha-ni / aarch64-sha2 / soft / unknown | 库运行时探测或推断 |
| source | crate-runtime / cpufeatures / inferred | 观测来源 |
| multithread | 整数 | Rayon 线程池 |

## 语义

- "reported" 表示库/运行环境报告的路径，**不保证 CPU 实际执行该路径**。
- 无法观测时必须写 unknown，不得编造 avx2 / sha-ni。
- 软件回退不是错误：MD5、SHA3 等通常无专用硬件指令，功能正常。
- 加速一致性：同一输入在 forced-soft 与 auto 下哈希必须逐字节一致（CI 断言，不断言具体后端）。

## Flutter 侧分区

区块 A：哈希计算加速（Rust 侧）
- CPU 架构、检测到指令、BLAKE3 报告后端、SHA-256 报告后端、后端策略、多线程。

区块 B：界面渲染加速（Flutter 侧）
- 硬件渲染：已启用 / 软件渲染（isHardwareAccelerated）。
- GPU 后端：Metal / Vulkan / OpenGL / Direct3D / unknown。

两个区块**严格分区展示**，避免用户混淆 GPU 渲染加速与哈希 CPU 加速。

## 状态语义

| 状态 | 颜色（MD3） | 含义 |
|---|---|---|
| 已检测并可能加速 | tertiary 容器 + 闪电图标 | 检测到可用指令，库自动选择 |
| 软件回退 | surfaceVariant + 信息图标 | 未检测到可用指令或用户强制软件，功能正常 |
| 不可用 / 未知 | outline + 问号图标 | 探测失败、库未报告或平台不适用 |

原则：软件回退不是错误；文案中性；提供"为什么？"说明入口。

## 平台说明

- Windows：不依赖 CNG/BCrypt 加速标志，统一走 Rust cpufeatures。
- macOS / iOS：不依赖 CommonCrypto HasSHA2()，Rust 直接检测 AArch64 SHA2 扩展。
- Linux：不经过 AF_ALG，依赖 CPU 指令集与 cpufeatures。
- Android：不通过 JNI 调用 JCA，Rust 核心直接检测 ARMv8 SHA2 扩展。
- CI runner 的 CPU 特性可能与用户设备不同：CI 只验证探测逻辑与结构，不断言特定后端。

## 记录位置

- 设置/关于页：完整面板 + 重新探测（计算中禁用）。
- 计算结果摘要卡：一行策略与报告后端。
- 报告元数据：JSON hardware 字段；CSV/TXT 头部简要信息。
- 对比：hardware 不参与差异判定，仅审计展示。