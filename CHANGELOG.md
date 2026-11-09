# Changelog

所有重要变更记录在此文件。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [未发布]

### 新增

- **中文优先界面**：默认语言为简体中文（不跟随系统语言），可在「设置 → 界面语言」切换到 English，
  切换后导航、按钮、状态与错误信息整体重译。全部文案集中在 `app/lib/src/l10n/app_strings.dart`
  的 zh/en 两表，页面统一通过 `context.strings('key')` 取词（见 [docs/localization.md](docs/localization.md)）。
- **缺库横幅**：未加载到本地哈希引擎时，界面顶部常驻「演示数据」警告，避免打包失误看起来像真实结果。

### 修复（界面无法看到结果 / 系统选择器 / xxh3）

- **计算成功却看不到结果**：结果列表此前只显示「N 个哈希」的计数，摘要值从未渲染。现在每行
  展开列出所有已选算法的完整摘要，可选中复制，并附带一键复制按钮。
- **xxh3 无法使用**：界面发送的算法 ID 是 `xxh3`，而引擎注册表的稳定 ID 是 `xxh3-64`，
  整批请求因此被判为 `invalid_request`。现在界面与引擎统一使用 `xxh3-64`。
- **多选文件只算了一个**：`computeHashes` 会自动重扫并只使用第一个文件所在的根目录。
  现在通过 `bh_hash_entries` 精确哈希用户选中的那组路径，跨目录选择不会再漏文件，
  也不会把未选中的同目录文件混进结果。
- **选择文件和文件夹没有系统选择器**：接入 `file_picker`，文件走原生多选对话框，
  文件夹走系统目录选择器；平台不支持时仍回退到手工路径输入，不会变成无路可走。

### 改进（操作逻辑）

- 哈希页重排为四步：选择输入 → 选择算法 → 开始计算 → 查看结果，每步有编号与完成状态，
  已选输入有独立摘要卡，结果区直接提供 CSV / JSON 导出。
### 修复

- **重复导航**：宽屏此前同时渲染导航栏、目的地列表与页面标题，同一个入口出现两到三次。
  现在每个断点只有一个导航控件（窄屏底部栏 / 中屏列表 / 宽屏 rail），中宽屏不再重复显示页面标题。
- 对比表格的状态标签此前直接显示枚举原文（`duplicate key`、`outside root` 等），
  现在逐项映射到本地化文案，未变化/重复键/越界/损坏/不稳定不再被混同为「成功」或「未知」。
- 写名计划行状态与报告格式说明不再显示英文原文。

### 修复（Windows 包无法启动）

- **Windows 包在普通机器上启动即报「找不到 MSVCP140D.dll / VCRUNTIME140D.dll /
  VCRUNTIME140_1D.dll / ucrtbased.dll」**：这几个名字末尾的 `D` 是 **Debug CRT**，只在
  装了 Visual Studio 的机器上存在，且不允许随应用分发。根因是 CI 的 Windows job 把
  `runner/Debug` 整个目录当作 artifact 上传，看起来就像可直接运行的发布包。
  现在 CI 的 Windows job 只做编译检查（并断言 `batchhash.exe` 确实产出），**不再上传任何
  可执行包**；能下载的 Windows 包只来自 release.yml 的 Release 构建。
- **Windows Release 包补齐运行库**：把与当前 MSVC 工具链配套的 x64 **Release CRT**
  复制到 exe 同目录（app-local），干净 Windows 10/11 无需预装 VC++ Redistributable。
- **Windows Release 包新增依赖门禁**：打包前用 `dumpbin /dependents` 扫描包内每个
  exe/dll——命中任何 Debug CRT 依赖直接失败；依赖了 MSVC 运行时但包里没有对应 DLL
  也直接失败（不发布一个必然起不来的包）。打包后再用 `tar -tf` 复核 zip 内含
  `batchhash.exe`、`batchhash_ffi.dll`、`flutter_windows.dll` 与 CRT，且不含 Debug CRT。

### 修复（正确性审计）

- **写名可能覆盖已存在的文件**：执行阶段此前只信任计划结果，计划之后新建的目标文件会被静默替换。
  现在默认走"不覆盖"路径：Linux 用 `renameat2(RENAME_NOREPLACE)`（不支持则回退到
  先检查后改名），Windows 依赖 `std::fs::rename` 自身语义，并在执行前逐行复查。
  覆盖只在调用方**显式**把 `overwrite` 设为 true 时发生：FFI 的 `BatchRequestDto` 有这个字段
  （默认 false），界面和 CLI 都没有暴露它——"不覆盖"是默认保证，不是一个不可关闭的硬编码路径。
- **撤销可能覆盖用户文件**：撤销此前只读日志状态。现在每次反向改名都重新探测文件系统
  （原路径必须为空、待撤销路径必须存在），任何不符记 `Skipped` 并保留该行状态，不猜。
- **崩溃窗口内的 pending 行会永远悬空**：打开日志时自动回收，三值判定
  （改名已生效 / 未生效 / 无法判定），无法判定的一律记 `Conflict` 而不是假定成功。
- **取消后仍会继续改名**：现在在 `ApplyingSuffix` 事件之后、第一次改名前、以及**每一行**改名之前
  都复查取消位；未执行的行记为 `not_attempted` → `Conflict`，报告也不再标注未发生的改名。
- **报告导入不校验**：CSV 未知列此前被静默丢弃，现在直接拒绝；JSON/CSV/md5sum/BSD 导入统一在
  返回前跑 `Report::validate()`；摘要与条目不一致（伪造或陈旧摘要）被拒绝。
- **对比把"缺哈希"当成"相同"**：双方声明了某算法、但某一侧没有该摘要时，此前会被跳过并判为
  `unchanged`；现在判 `Unknown` 并指名缺失的算法。
- **BLAKE3 的 mmap 路径**：已整体移除（含 `unsafe` 代码与 `memmap2` 依赖），大文件统一走
  分块流式读取，行为只有一条代码路径。
- **界面里的"扫描"其实读的是演示数据**：路径输入此前是硬编码的示例目录。现在改为应用内路径输入
  （含历史建议），并如实把 `recursive` 透传到扫描与写名执行——"只扫这一层"不会再变成整棵子树。
- **历史页跨启动为空**：现在把用过的根目录与日志路径持久化到应用支持目录，重启后仍能撤销。
- **报告导出没有落点控制**：可选择导出目录，默认写入用户导出目录而不是临时目录。
- **FFI 导入无大小预检**：现在先查大小再读文件，超限返回 `too_large`；撤销结果区分
  undone / failed / skipped，不再把跳过算作成功。
- **CLI 写名失败仍返回 0**：现在 `suffix` 计划执行与撤销在存在失败行时以非零码退出。

### 新增（本轮）

- 对比页支持**两份报告文件互相对比**（此前只能"报告 vs 目录"）。
- CI：`Version / changelog gate` 校验版本三处一致；`--locked` 让 `Cargo.lock` 成为被验证产物；
  Flutter / Rust 工具链固定版本。
- CI：Release 工作流权限收敛为工作流级 `contents: read`，只有 `publish` job 持有
  `contents: write` 且仅在 tag 推送时创建 Release；各平台改上传 artifact。
- CI：Windows job 固定到 `windows-2022`。`windows-latest` 已切到 `windows-2025-vs2026`
  （VS 18），而 Flutter 3.24.5 只把 VS 17 映射到 `Visual Studio 17 2022` generator，其余
  一律发 `Visual Studio 16 2019`，该镜像上并不存在，Windows 构建会直接失败。
- CI：配置 secrets 时给 Android 注入 release 签名并用 `apksigner` 断言**不是** debug 签名；
  iOS 发布 job 改为显式门禁失败，另设无签名诊断 job。
- CI：`verify-versions.mjs` 还会比对 `rust-toolchain.toml` 的 channel 与各工作流的
  `dtolnay/rust-toolchain@<版本>`，并要求 channel 不低于 `Cargo.toml` 的 MSRV 下限。
  这条检查是本轮事故的产物：`rust-toolchain.toml` 曾写 `stable` 而工作流装的是另一个版本 +
  Android target，rustup 在仓库内强制切 channel，导致打包 job 报 `can't find crate for core`。
- Rust 工具链固定为 **1.89.0**（原 1.82.0 站不住）：锁定依赖集自己要求它——`uuid 1.29.0`
  声明 1.89.0，`cpufeatures`/`getrandom`/`hashbrown`/`indexmap` 是 edition 2024 crate，
  更低版本的 Cargo 连它们的清单都解析不了。
- `dynamic_color` 约束收紧为 `>=1.7.0 <1.8.0`：1.8.0 起用了只有 Flutter 3.27+ 才有的
  `Color.toARGB32()`，而包的约束仍写 `flutter >=3.4`；CI 钉 Flutter 3.24.5，
  按 `^1.7.0` 解析会选中编译不过的版本。

### 已知限制

- 语言选择仅本次会话有效，未持久化。
- 没有系统文件选择器（不引入原生插件），路径由应用内输入完成。
- Rust 返回的错误文本是英文，界面只本地化固定文案。
- iOS 静态库仍未链接进 Runner target。
- Android release 签名、macOS 公证、Windows 代码签名均**未在真实运行中验证**。

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