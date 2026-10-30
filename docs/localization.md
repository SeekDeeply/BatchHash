# 界面本地化（中文优先）

界面以**简体中文为默认语言**，不跟随系统语言；用户可在「设置 → 界面语言」切换到 English。
两套文案共用同一份 key 表，切换语言会立即重译整个界面（导航、按钮、错误信息、状态标签）。

## 结构

| 文件 | 职责 |
|---|---|
| [`app/lib/src/l10n/app_strings.dart`](../app/lib/src/l10n/app_strings.dart) | 唯一的 key → 文案表，`_zh` 与 `_en` 两个表；`defaultLocale = 'zh'` |
| [`app/lib/src/l10n/app_strings_scope.dart`](../app/lib/src/l10n/app_strings_scope.dart) | `AppStringsScope`（InheritedWidget）+ `context.strings('key')` 扩展 |
| [`app/lib/src/l10n/locale_controller.dart`](../app/lib/src/l10n/locale_controller.dart) | 运行时语言状态（`ChangeNotifier`），设置页写入 |
| [`app/lib/src/app.dart`](../app/lib/src/app.dart) | 安装 `AppStringsScope`、固定 `MaterialApp.locale`、语言变化时整体重建 |

## 使用规则

1. 所有用户可见文本都经由 `context.strings('some.key')`；带占位符的用
   `context.strings('hash.progressText', args: <String, String>{'done': '3', 'total': '9'})`。
2. **不要把算法 ID、文件路径、ValueKey、单位（ms）、schema/格式标识**本地化，它们是数据不是文案。
3. 新增文案时同时在 `_zh` 与 `_en` 里加一条；key 缺失时解析会退回中文，最后才退回 key 本身，
   因此漏加 `_en` 会静默显示中文——评审时以「两表 key 必须同名同占位符」为准。
4. 占位符名字必须与文案里的 `{name}` 完全一致，否则替换会静默失效。

## 为什么不用 `intl`/ARB

当前只有两种语言、无复数/性别规则，直接查表最简单且可被纯 Dart 单测覆盖
（见 [`app/test/l10n_test.dart`](../app/test/l10n_test.dart)）。若将来文案规模或规则复杂度上升，
`AppStrings` 的两个表可以原样作为 ARB 的生成输入，调用点（`context.strings`）不需要改。

## 测试

- `app/test/l10n_test.dart`：默认语言是中文、两种语言都有译文、设置页切换后整壳重译。
- `app/test/support/test_helpers.dart` 的 `wrapInApp(child, locale: 'en')` 会安装字符串作用域并固定英文，
  因此页面级测试断言的是英文文案；应用级测试通过 `BatchHashApp(locale: 'en')` 固定语言。
- 新增页面时，如果它渲染本地化文案，必须用 `wrapInApp` 包裹，否则 `AppStringsScope.of` 的断言会失败。

## 未做的部分

- 语言选择当前只在**本次会话内**有效，没有持久化；需要持久化时只需在 `LocaleController` 里读写存储。
- 没有 RTL 语言；新增时需检查 `Directionality` 相关布局。
- Rust 内核返回的错误/备注（例如写名计划里的 `message`）仍是英文原文，界面按原样展示。
