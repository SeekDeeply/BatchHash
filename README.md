# BatchHash

**全平台批量 Hash 校验、导出与比对的瑞士军刀。**

一次扫描，同时算出十几种校验和；写成报告，随时拿回来比对；需要的话还能把摘要直接刻进
文件名。全部运算在本机完成，文件不出本地。

[![Release](https://img.shields.io/github/v/release/SeekDeeply/BatchHash)](https://github.com/SeekDeeply/BatchHash/releases/latest)
[![CI](https://github.com/SeekDeeply/BatchHash/actions/workflows/ci.yml/badge.svg)](https://github.com/SeekDeeply/BatchHash/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

如果这个项目对你有帮助，欢迎在 GitHub 上点亮 Star —— 这是最直接的支持，也能让更多人找到它。

## 能做什么

- **44 种哈希算法一次算完**：MD5、SHA-1、SHA-224/256/384/512、SHA3-256/512、BLAKE2b/2s、BLAKE3、
  XXH3-64/128、XXH64、CRC32/CRC32C 等。单遍读取同时喂给所有勾选算法，大文件按块流式处理，
  并行由 Rayon 调度。算法列表可搜索，支持一键全选（搜索生效时只作用于当前可见结果）。
- **报告导出与比对**：JSON / CSV / Markdown / TXT / md5sum / BSD 六种格式导出，JSON / CSV /
  md5sum / BSD 四种格式导入。报告对报告、报告对实时目录都能比，PathNormalizer 归一化后做
  full outer join，流式增量输出，能区分 `incomplete` / `unstable`。
- **文本对比带哈希**：两段文本对比完，各自用 SHA-256 与 BLAKE3 算出的摘要会并排列出，
  并标出是否一致。摘要直接对内存字节计算，不改写换行，所以和把同样内容当文件算出来的结果一致。
- **批量重命名**：`原名[ABCDE].扩展名`，长度可调、大小写可选，先出计划（不写盘）、检测冲突，
  再执行；写入 SQLite 日志，支持跨会话撤销。
- **操作日志**：扫描、哈希、重命名、校验、导出、对比的每一次执行都留痕，记录结果、耗时、
  错误信息与产生的摘要。可按类型过滤、按关键词搜索、按成功/失败筛选、按时间正逆序排列，
  保留条数可配置。
- **主题色自定义**：内置预设色块点一下即生效，也可以输入 `#RRGGBB`、`#RGB`、`#AARRGGBB`、
  `rgb(r,g,b)`、`rgba(r,g,b,a)` 自定义主色。色块显示的是该种子**实际生成**的界面主色，
  不是原始种子色 —— 这样点之前就知道会变成什么样。
- **界面中英双语**：简体中文优先，设置页可切换英文，整壳重译。
- **首次启动需同意条款**：初次运行弹出免责声明与用户条款，确认后才进入应用，之后不再打扰。
  条款内容内置，离线可查。
- **记住窗口大小**：桌面端关闭时记录尺寸，下次启动恢复。

## 下载

到 [Releases](https://github.com/SeekDeeply/BatchHash/releases/latest) 取最新产物：

| 平台 | 文件 | 说明 |
|---|---|---|
| Windows | `BatchHash-windows.zip` | 解压后运行 `batchhash.exe` |
| macOS | `BatchHash-macos.zip` | ad-hoc 签名，未公证 |
| Linux | `BatchHash-linux.tar.gz` | 解压后运行 |
| Android | `app-arm64-v8a-release.apk` 等 | 另有 `app-release.aab` 供上架 |

### 为什么 Windows 只发 Release 包

`BatchHash-windows.zip` 已把 MSVC Release 运行时（`vcruntime140.dll`、`msvcp140.dll` 等）
放在 exe 同目录，干净的 Windows 10/11 无需预装 VC++ Redistributable，也不需要 Visual Studio。

如果看到 `由于找不到 MSVCP140D.dll / VCRUNTIME140D.dll / ucrtbased.dll，无法继续执行代码`，
那是 **debug 构建**（名字末尾的 `D` 就是 Debug CRT）。Debug CRT 只在装了 Visual Studio 的机器上
存在，且不允许随应用分发。CI 的 Windows job 因此只做编译检查、不上传可执行包。请改用 Release
产物，**不要**去第三方站点下载单个 DLL 塞进系统目录。

## 平台支持状态

| 平台 | 状态 |
|---|---|
| **Windows** | **主要目标平台，已做充分测试与验证**。日常开发与验证都在这里进行，推荐优先使用。 |
| macOS | 已提供构建产物，实际使用反馈较少，仍在持续适配。 |
| Linux | 已提供构建产物，实际使用反馈较少，仍在持续适配。 |
| Android | 已提供构建产物（含 `.aab`），仍在持续适配。 |
| iOS | 静态库与模拟器构建已打通，正式发布通道默认关闭，**不建议**当作可用版本。 |

除 Windows 外的平台欢迎提交 Issue 反馈问题、参与多平台测试。提交时请附上平台、版本号与复现步骤，
能显著提高修复效率。

## 构建说明

**目前推荐使用官方 CI 发布的预编译二进制，暂未开放本地环境源码编译。**

原因很直接：本项目的验证链条（Rust 工作区测试、clippy、Flutter analyze/test、六平台交叉构建、
原生库打包断言）全部由 GitHub Actions 承载。本地编译既无法复现 CI 的交叉工具链与平台依赖
（GTK、MSVC、Xcode、NDK），也容易产出与官方产物行为不一致的二进制，反而增加排障成本。

- 所有编译、静态检查、测试与产物构建都在 CI 完成，见
  [`.github/workflows/ci.yml`](.github/workflows/ci.yml) 与
  [`.github/workflows/release.yml`](.github/workflows/release.yml)。
- 想从源码构建的开发者，可以直接参照上述工作流中的步骤与依赖清单；
  [`scripts/prepare-app.mjs`](scripts/prepare-app.mjs) 负责生成各平台脚手架与应用标识。
- 本地仅建议做「编辑代码、阅读文档、静态审阅」这类工作。

CI 中的 `Version / changelog gate` 会校验 `Cargo.toml`、`app/pubspec.yaml`、`CHANGELOG.md`
与 `Cargo.lock` 的版本一致性，发版由打 tag 触发。

## 仓库结构

```
crates/
  batchhash-types/    共享类型：算法注册表、文件条目、Report schema v1、Diff schema v1
  batchhash-hash/     哈希引擎：单遍多哈希、fd 输入、Rayon 批处理、硬件探测
  batchhash-report/   PathNormalizer、Diff Engine、报告六种导出 / 四种导入
  batchhash-rename/   后缀规则、重命名计划与执行、SQLite 日志与撤销
  batchhash-core/     门面 crate（供桥接层与 CLI 使用）
  batchhash-cli/      开发者 CLI（调试用，不参与分发）
app/                  Flutter UI（Material 3、自适应布局、中英双语）
  lib/src/pages/      各功能页：哈希、重命名、报告、对比、历史、设置、法律、操作日志
  lib/src/services/   服务层：FFI 实现、假实现、操作日志、配置与窗口状态持久化
  lib/src/widgets/    复用组件：算法选择、文本对比、色卡、硬件面板
docs/                 规划、schema 与平台文档
scripts/              CI 构建辅助脚本
.github/workflows/    CI 验证流水线与 Release 流水线
```

## 文档

| 文档 | 内容 |
|---|---|
| [docs/PROJECT-PLAN.md](docs/PROJECT-PLAN.md) | 项目规划与阶段路线图 |
| [docs/internal-api-contracts.md](docs/internal-api-contracts.md) | Rust crate 间接口契约 |
| [docs/report-schema-v1.md](docs/report-schema-v1.md) | 报告 JSON schema v1 与六种格式规范 |
| [docs/diff-schema-v1.md](docs/diff-schema-v1.md) | Diff schema v1 与状态语义 |
| [docs/algorithm-registry.md](docs/algorithm-registry.md) | 算法注册表与 stable_id 契约 |
| [docs/hash-workflow.md](docs/hash-workflow.md) | 哈希页操作流程与文件选择行为 |
| [docs/path-normalization.md](docs/path-normalization.md) | 对比 key 归一化规范与用例表 |
| [docs/hardware-probe.md](docs/hardware-probe.md) | 硬件探测字段与 reported/unknown 语义 |
| [docs/file-entry-root-spec.md](docs/file-entry-root-spec.md) | 文件条目与单根批处理规范 |
| [docs/rename-log-schema.md](docs/rename-log-schema.md) | 重命名日志 SQLite schema 与撤销状态机 |
| [docs/android-saf.md](docs/android-saf.md) | Android SAF / fd / Kotlin 重命名职责边界 |
| [docs/privacy-and-security.md](docs/privacy-and-security.md) | 隐私、截断哈希安全语义、导入校验 |
| [docs/cli.md](docs/cli.md) | 开发者 CLI 命令、选项与退出码 |
| [docs/localization.md](docs/localization.md) | 中文优先的文案表与测试约定 |
| [docs/bridge.md](docs/bridge.md) | 原生桥接 ABI 契约、Dart 绑定与 CI 校验方式 |
| [docs/native-packaging.md](docs/native-packaging.md) | 各平台原生库产物与打包方式 |
| [docs/ci.md](docs/ci.md) | CI 流水线结构 |

## 安全语义

- MD5 / SHA-1 已被密码学攻破；XXH / CRC 是非密码学校验。它们**不能**用于安全场景。
- 文件名中的截断后缀只用于便利比对，**不是安全摘要**；报告同时保存完整哈希，比对始终使用完整哈希。
- 报告导入默认不可信：限制条目数与文件大小、拒绝未来 schema 版本、拒绝路径逃逸。
- 所有哈希在本机计算，不上传任何文件或摘要。

## 已知限制

以下几点请在使用前知悉：

- **界面语言不持久化**：切换只在当前会话有效，重启回到默认中文。
- **错误信息语言**：Rust 核心返回的错误文本是英文（`kind` 如 `invalid_request` / `scan` / `log`），
  界面只本地化固定文案，不翻译 Rust 消息。
- **二进制未签名**：Windows 未接入代码签名，SmartScreen 会提示；macOS 为 ad-hoc 签名、未公证，
  Gatekeeper 会提示；Android 若未配置签名密钥则为 debug 签名，`.aab` 上架前需重新签名。
- **运行时加载验证**：CI 断言产物内**包含**原生库并跑通桥接链路，但未在真实设备安装包上长期实测。
- **iOS**：正式发布通道默认关闭（`ENABLE_IOS_RELEASE` 未开启），当前不作为可用版本。

## 参与贡献

Issue 与 PR 都欢迎，尤其是非 Windows 平台的实测反馈。提交前建议先看一眼
[CONTRIBUTING.md](CONTRIBUTING.md) 与 CI 的工作流定义，两者能省掉不少来回。

如果你的使用场景跑通了某个平台，欢迎在 Issue 里说一声——这比什么都更有用。

## 许可证

Apache-2.0，见 [LICENSE](LICENSE)。
