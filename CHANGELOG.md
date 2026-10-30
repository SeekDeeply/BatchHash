# Changelog

所有重要变更记录在此文件。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [未发布]

### 新增

- **中文优先界面**：默认语言为简体中文（不跟随系统语言），可在「设置 → 界面语言」切换到 English，
  切换后导航、按钮、状态与错误信息整体重译。全部文案集中在 `app/lib/src/l10n/app_strings.dart`
  的 zh/en 两表，页面统一通过 `context.strings('key')` 取词（见 [docs/localization.md](docs/localization.md)）。
- **缺库横幅**：未加载到本地哈希引擎时，界面顶部常驻「演示数据」警告，避免打包失误看起来像真实结果。

### 修复

- **重复导航**：宽屏此前同时渲染导航栏、目的地列表与页面标题，同一个入口出现两到三次。
  现在每个断点只有一个导航控件（窄屏底部栏 / 中屏列表 / 宽屏 rail），中宽屏不再重复显示页面标题。
- 对比表格的状态标签此前直接显示枚举原文（`duplicate key`、`outside root` 等），
  现在逐项映射到本地化文案，未变化/重复键/越界/损坏/不稳定不再被混同为「成功」或「未知」。
- 写名计划行状态与报告格式说明不再显示英文原文。

### 已知限制

- 语言选择仅本次会话有效，未持久化。

## [0.1.0-rc.1] - 2026-10-30

首个候选版本，随 GitHub Release 提供 Linux / Windows / macOS / Android 四个平台的发布产物。
每个产物都经过 CI 断言，确认内含 Rust 原生核心（见 docs/release-checklist.md）。

- iOS 未包含：静态库尚未链接进 Runner target，发布 iOS 版本会退回演示数据。
- Android 产物使用调试签名，仅供安装体验，不可上架；上架需配置签名密钥。
- macOS 产物为 ad-hoc 签名，未做公证。


### 新增

- 16 个哈希算法的单遍多哈希引擎（MD5、SHA-1、SHA-224/256/384/512、SHA3-256/512、
  BLAKE2b/2s、BLAKE3、XXH3-64/128、XXH64、CRC32/CRC32C），含 NIST / 官方 xxHash 已知向量测试。
- Android SAF 文件描述符输入（`dup + seek + 长度上限`，绝不关闭调用方 fd）。
- 报告 schema v1：JSON / CSV / Markdown / TXT / md5sum / BSD 导出，JSON / CSV / md5sum / BSD 导入，
  导入侧强制条目数/大小上限、拒绝未来 schema 与路径逃逸。
- 对比引擎：报告 vs 报告、报告 vs 实时目录、流式增量对比；`incomplete` / `unstable` /
  `duplicate_key` / `ambiguous` 语义齐全，取消扫描绝不伪造成"删除"。
- 文件名后缀写名：`原名[ABCDE].扩展名`，4..=16 位、碰撞估算、纯 dry-run 计划、
  大小写不敏感冲突检测、SQLite 日志与跨会话撤销。
- 开发者 CLI（`hash / report / diff / scan-diff / suffix / extract / probe`）。
- Flutter Material 3 UI（自适应断点 720/1100、双列对比、硬件可观测面板）。
- 原生桥接：JSON C ABI + `dart:ffi`（15 个导出函数），`RustBatchHashService` 实现全部 UI 服务，
  缺库时回退到 fake 服务。
- CI：11 个 job（格式、clippy、测试、Flutter analyze/test、桥接端到端、原生库打包、五平台 debug 构建）；
  另有 `Format (CI)` 与 `Release check` 两个按需工作流。

### 已知限制

- iOS 静态库已构建并随发布产物提供，但尚未链接进 Runner target（iOS 端回退到 fake 服务）。
- macOS / Windows 发布包的动态库复制逻辑已写入 `release.yml`，尚未在发布流程中实测。
- 桥接的 `computeHashes` 为一次性完成事件，不是逐文件流式进度。
- 未签名：发布需要配置签名/公证密钥（见 docs/release-secrets.md）。