# Changelog

所有重要变更记录在此文件。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [未发布] - 0.1.0

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
