//! 导出展示标签（英文 / 简体中文）与文件树格式的标签。
//!
//! 设计取舍：只有「给人看」的格式（Markdown / TXT / Tree）走这套标签。
//! JSON、CSV、md5sum、BSD 四种硬编码英文键，因为它们是**机器契约**：
//! coreutils `md5sum -c`、CI 脚本、FFI/Dart 侧的解析器都按固定键名读取，
//! 一旦翻译就会直接破坏这些消费者。语言化只发生在展示层，绝不进入数据层。
//!
//! 另一处取舍：英文下 Markdown 用 `Generated at` 这样的表头，TXT 用
//! `generated_at` 这样的对齐键。两者历史输出不同，且已有测试断言小写键，
//! 所以 [`LabelStyle`] 保留这个差别，而不是把两种格式统一成一种写法。

/// 导出时使用的展示语言。
///
/// 默认英文：既有导出的字节内容因此保持不变，调用方必须显式选择中文。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ExportLocale {
    /// 英文（默认）。
    #[default]
    En,
    /// 简体中文。
    Zh,
}

impl ExportLocale {
    /// 把语言代码映射到 locale。
    ///
    /// 接受 `zh`/`zh-CN`/`zh-Hans` 与 `en`/`en-US`（大小写不敏感、忽略首尾空白、`_` 视同 `-`）。
    /// 未知代码返回 `None`：静默回退到默认语言会让用户以为设置生效了，
    /// 由调用方决定是报错还是显式回退。
    pub fn from_code(code: &str) -> Option<ExportLocale> {
        let normalized = code.trim().to_ascii_lowercase().replace('_', "-");
        match normalized.as_str() {
            "zh" | "zh-cn" | "zh-hans" => Some(ExportLocale::Zh),
            "en" | "en-us" => Some(ExportLocale::En),
            _ => None,
        }
    }

    /// 该语言的标签表。
    pub const fn labels(self) -> &'static Labels {
        match self {
            ExportLocale::En => &Labels::EN,
            ExportLocale::Zh => &Labels::ZH,
        }
    }

    /// 稳定标识，写入设置与诊断信息。
    pub const fn as_str(self) -> &'static str {
        match self {
            ExportLocale::En => "en",
            ExportLocale::Zh => "zh",
        }
    }
}

/// 标签的排版风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelStyle {
    /// Markdown 表格表头（英文为 `Generated at` 这样的标题式写法）。
    Table,
    /// 对齐文本 / 文件树的对齐键（英文为 `generated_at` 这样的标识符写法）。
    Text,
}

/// 报告元信息字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaField {
    /// 生成时间。
    GeneratedAt,
    /// 工具名。
    Tool,
    /// 工具版本。
    ToolVersion,
    /// 报告 schema 版本。
    SchemaVersion,
    /// 扫描根目录。
    Root,
    /// 根类型（fs / saf）。
    RootKind,
    /// 大小写敏感策略。
    CaseSensitive,
    /// 声明的算法列表。
    Algorithms,
    /// 后缀算法。
    SuffixAlgorithm,
    /// 后缀长度。
    SuffixLength,
    /// 后缀大小写。
    SuffixCase,
}

/// 汇总指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryField {
    /// 条目总数。
    Total,
    /// 成功数。
    Ok,
    /// 失败数。
    Failed,
    /// 跳过数。
    Skipped,
    /// 字节总数。
    TotalBytes,
    /// 是否发生导出截断。
    Truncated,
}

/// 条目表列名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryField {
    /// 相对路径。
    Path,
    /// 文件大小。
    Size,
    /// 修改时间。
    Mtime,
    /// 处理状态。
    Status,
    /// 错误类别。
    ErrorKind,
    /// 是否已加后缀。
    SuffixApplied,
    /// 后缀所用算法。
    SuffixAlgorithm,
    /// 重命名目标。
    RenamedTo,
}

/// 对比文档元信息字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMetaField {
    /// 生成时间。
    GeneratedAt,
    /// 对比 schema 版本。
    SchemaVersion,
    /// 比较模式。
    Mode,
    /// 大小写敏感策略。
    CaseSensitive,
    /// 参与比较的算法。
    AlgorithmsCompared,
    /// 左侧来源标签。
    Left,
    /// 右侧来源标签。
    Right,
}

/// 对比汇总字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSummaryField {
    /// 行总数。
    Total,
    /// 变更数。
    Changed,
    /// 新增数。
    Added,
    /// 删除数。
    Removed,
    /// 扫描是否不完整。
    Incomplete,
}

/// 一套完整的展示标签。
///
/// 所有文字都是 `&'static str`：语言在编译期确定，导出路径上不做分配，
/// 也不会出现「同一份报告两次导出因为语言设置变化而不同」的情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Labels {
    locale: ExportLocale,
    /// 报告文档标题（Markdown 渲染器自己加 `# ` 前缀）。
    pub report_title: &'static str,
    /// 对比文档标题。
    pub diff_title: &'static str,
    /// 汇总小节标题。
    pub summary_section: &'static str,
    /// 条目小节标题。
    pub entries_section: &'static str,
    /// 对比明细小节标题。
    pub diff_rows_section: &'static str,
    /// 文件树小节标题。
    pub tree_section: &'static str,
}

impl Labels {
    /// 英文标签，也是历史输出的基准。
    pub const EN: Labels = Labels {
        locale: ExportLocale::En,
        report_title: "BatchHash Report",
        diff_title: "BatchHash Diff",
        summary_section: "Summary",
        entries_section: "Entries",
        diff_rows_section: "Rows",
        tree_section: "Tree",
    };

    /// 简体中文标签。
    pub const ZH: Labels = Labels {
        locale: ExportLocale::Zh,
        report_title: "BatchHash 报告",
        diff_title: "BatchHash 对比",
        summary_section: "汇总",
        entries_section: "条目",
        diff_rows_section: "对比明细",
        tree_section: "文件树",
    };

    /// 指定语言的标签表。
    pub const fn for_locale(locale: ExportLocale) -> &'static Labels {
        locale.labels()
    }

    /// 这套标签对应的语言。
    pub const fn locale(&self) -> ExportLocale {
        self.locale
    }

    /// 元信息字段标签。
    pub fn meta(&self, field: MetaField, style: LabelStyle) -> &'static str {
        match self.locale {
            ExportLocale::En => match style {
                LabelStyle::Table => match field {
                    MetaField::GeneratedAt => "Generated at",
                    MetaField::Tool => "Tool",
                    MetaField::ToolVersion => "Tool version",
                    MetaField::SchemaVersion => "Schema version",
                    MetaField::Root => "Root",
                    MetaField::RootKind => "Root kind",
                    MetaField::CaseSensitive => "Case sensitive",
                    MetaField::Algorithms => "Algorithms",
                    MetaField::SuffixAlgorithm => "Suffix algorithm",
                    MetaField::SuffixLength => "Suffix length",
                    MetaField::SuffixCase => "Suffix case",
                },
                LabelStyle::Text => match field {
                    MetaField::GeneratedAt => "generated_at",
                    MetaField::Tool => "tool",
                    MetaField::ToolVersion => "tool_version",
                    MetaField::SchemaVersion => "schema_version",
                    MetaField::Root => "root",
                    MetaField::RootKind => "root_kind",
                    MetaField::CaseSensitive => "case_sensitive",
                    MetaField::Algorithms => "algorithms",
                    MetaField::SuffixAlgorithm => "suffix_algorithm",
                    MetaField::SuffixLength => "suffix_length",
                    MetaField::SuffixCase => "suffix_case",
                },
            },
            // 中文没有大小写差异，两种风格共用一套译名。
            ExportLocale::Zh => match field {
                MetaField::GeneratedAt => "生成时间",
                MetaField::Tool => "工具",
                MetaField::ToolVersion => "工具版本",
                MetaField::SchemaVersion => "报告格式版本",
                MetaField::Root => "根目录",
                MetaField::RootKind => "根类型",
                MetaField::CaseSensitive => "区分大小写",
                MetaField::Algorithms => "算法",
                MetaField::SuffixAlgorithm => "后缀算法",
                MetaField::SuffixLength => "后缀长度",
                MetaField::SuffixCase => "后缀大小写",
            },
        }
    }

    /// 汇总指标标签。
    pub fn summary(&self, field: SummaryField, style: LabelStyle) -> &'static str {
        match self.locale {
            ExportLocale::En => match style {
                LabelStyle::Table => match field {
                    SummaryField::Total => "Total",
                    SummaryField::Ok => "Ok",
                    SummaryField::Failed => "Failed",
                    SummaryField::Skipped => "Skipped",
                    SummaryField::TotalBytes => "Total bytes",
                    SummaryField::Truncated => "Truncated",
                },
                LabelStyle::Text => match field {
                    SummaryField::Total => "total",
                    SummaryField::Ok => "ok",
                    SummaryField::Failed => "failed",
                    SummaryField::Skipped => "skipped",
                    SummaryField::TotalBytes => "total_bytes",
                    SummaryField::Truncated => "truncated",
                },
            },
            ExportLocale::Zh => match field {
                SummaryField::Total => "总数",
                SummaryField::Ok => "正常",
                SummaryField::Failed => "失败",
                SummaryField::Skipped => "跳过",
                SummaryField::TotalBytes => "总字节数",
                SummaryField::Truncated => "已截断",
            },
        }
    }

    /// 条目列标签。
    pub fn entry(&self, field: EntryField, style: LabelStyle) -> &'static str {
        match self.locale {
            ExportLocale::En => match style {
                LabelStyle::Table => match field {
                    EntryField::Path => "Path",
                    EntryField::Size => "Size",
                    EntryField::Mtime => "Mtime",
                    EntryField::Status => "Status",
                    EntryField::ErrorKind => "Error kind",
                    EntryField::SuffixApplied => "Suffix applied",
                    EntryField::SuffixAlgorithm => "Suffix algorithm",
                    EntryField::RenamedTo => "Renamed to",
                },
                LabelStyle::Text => match field {
                    EntryField::Path => "path",
                    EntryField::Size => "size",
                    EntryField::Mtime => "mtime",
                    EntryField::Status => "status",
                    EntryField::ErrorKind => "error_kind",
                    EntryField::SuffixApplied => "suffix_applied",
                    EntryField::SuffixAlgorithm => "suffix_algorithm",
                    EntryField::RenamedTo => "renamed_to",
                },
            },
            ExportLocale::Zh => match field {
                EntryField::Path => "路径",
                EntryField::Size => "大小",
                EntryField::Mtime => "修改时间",
                EntryField::Status => "状态",
                EntryField::ErrorKind => "错误类别",
                EntryField::SuffixApplied => "已加后缀",
                EntryField::SuffixAlgorithm => "后缀算法",
                EntryField::RenamedTo => "重命名为",
            },
        }
    }

    /// 对比文档元信息标签。
    pub fn diff_meta(&self, field: DiffMetaField, style: LabelStyle) -> &'static str {
        match self.locale {
            ExportLocale::En => match style {
                LabelStyle::Table => match field {
                    DiffMetaField::GeneratedAt => "Generated at",
                    DiffMetaField::SchemaVersion => "Schema version",
                    DiffMetaField::Mode => "Mode",
                    DiffMetaField::CaseSensitive => "Case sensitive",
                    DiffMetaField::AlgorithmsCompared => "Algorithms compared",
                    DiffMetaField::Left => "Left",
                    DiffMetaField::Right => "Right",
                },
                LabelStyle::Text => match field {
                    DiffMetaField::GeneratedAt => "generated_at",
                    DiffMetaField::SchemaVersion => "schema_version",
                    DiffMetaField::Mode => "mode",
                    DiffMetaField::CaseSensitive => "case_sensitive",
                    DiffMetaField::AlgorithmsCompared => "algorithms_compared",
                    DiffMetaField::Left => "left",
                    DiffMetaField::Right => "right",
                },
            },
            ExportLocale::Zh => match field {
                DiffMetaField::GeneratedAt => "生成时间",
                DiffMetaField::SchemaVersion => "报告格式版本",
                DiffMetaField::Mode => "比较模式",
                DiffMetaField::CaseSensitive => "区分大小写",
                DiffMetaField::AlgorithmsCompared => "参与比较的算法",
                DiffMetaField::Left => "左侧",
                DiffMetaField::Right => "右侧",
            },
        }
    }

    /// 对比汇总标签。
    pub fn diff_summary(&self, field: DiffSummaryField, style: LabelStyle) -> &'static str {
        match self.locale {
            ExportLocale::En => match style {
                LabelStyle::Table => match field {
                    DiffSummaryField::Total => "Total",
                    DiffSummaryField::Changed => "Changed",
                    DiffSummaryField::Added => "Added",
                    DiffSummaryField::Removed => "Removed",
                    DiffSummaryField::Incomplete => "Incomplete",
                },
                LabelStyle::Text => match field {
                    DiffSummaryField::Total => "total",
                    DiffSummaryField::Changed => "changed",
                    DiffSummaryField::Added => "added",
                    DiffSummaryField::Removed => "removed",
                    DiffSummaryField::Incomplete => "incomplete",
                },
            },
            ExportLocale::Zh => match field {
                DiffSummaryField::Total => "总数",
                DiffSummaryField::Changed => "已变更",
                DiffSummaryField::Added => "新增",
                DiffSummaryField::Removed => "删除",
                DiffSummaryField::Incomplete => "不完整",
            },
        }
    }

    /// 条目状态的展示文案。
    ///
    /// 只用于人读格式；CSV 的 `status` 列仍走 `format::entry_status_str`，
    /// 那是机器契约，不随语言变化。
    pub fn status(&self, status: batchhash_types::EntryStatus) -> &'static str {
        match self.locale {
            ExportLocale::En => match status {
                batchhash_types::EntryStatus::Ok => "ok",
                batchhash_types::EntryStatus::Failed => "failed",
                batchhash_types::EntryStatus::Skipped => "skipped",
            },
            ExportLocale::Zh => match status {
                batchhash_types::EntryStatus::Ok => "成功",
                batchhash_types::EntryStatus::Failed => "失败",
                batchhash_types::EntryStatus::Skipped => "跳过",
            },
        }
    }

    /// 文件树里附在文件名后的状态注记；正常条目不加注记。
    ///
    /// 文案与 Dart 侧测试替身 `app/lib/src/services/report_rendering.dart`
    /// 的 `_Labels.statusFailed` / `statusSkipped` 逐字一致，改动需同步，
    /// 否则 widget 测试所见的文本会和真实产物不同。
    pub fn status_note(&self, status: batchhash_types::EntryStatus) -> Option<&'static str> {
        match status {
            batchhash_types::EntryStatus::Ok => None,
            batchhash_types::EntryStatus::Failed => Some(match self.locale {
                ExportLocale::En => "(failed)",
                ExportLocale::Zh => "（失败）",
            }),
            batchhash_types::EntryStatus::Skipped => Some(match self.locale {
                ExportLocale::En => "(skipped)",
                ExportLocale::Zh => "（跳过）",
            }),
        }
    }

    /// 布尔值文案。
    pub fn boolean(&self, value: bool) -> &'static str {
        match self.locale {
            ExportLocale::En => {
                if value {
                    "true"
                } else {
                    "false"
                }
            }
            ExportLocale::Zh => {
                if value {
                    "是"
                } else {
                    "否"
                }
            }
        }
    }

    /// 后缀十六进制大小写的文案。
    pub fn hex_case(&self, case: batchhash_types::HexCase) -> &'static str {
        match self.locale {
            ExportLocale::En => match case {
                batchhash_types::HexCase::Upper => "upper",
                batchhash_types::HexCase::Lower => "lower",
            },
            ExportLocale::Zh => match case {
                batchhash_types::HexCase::Upper => "大写",
                batchhash_types::HexCase::Lower => "小写",
            },
        }
    }

    /// 文件树汇总块用的标签。
    ///
    /// 这套文字**逐字**对应 Dart 侧测试替身
    /// `app/lib/src/services/report_rendering.dart` 的 `_Labels`：那边的英文是
    /// `Total` / `ok` / `failed` / `skipped` / `Total bytes`——大小写是混的，
    /// 既不是 [`LabelStyle::Table`] 也不是 [`LabelStyle::Text`]。因此这里不复用
    /// 那两套表，而是单独钉死，**改动必须与 Dart 同步**，否则 widget 测试
    /// 断言的文本会和真实产物对不上。
    pub fn tree_summary(&self, field: SummaryField) -> &'static str {
        match self.locale {
            // Capitalised like the rest of the report. The earlier mixed case
            // ("Total" next to "ok") was an accident of hand-writing these and
            // is worth fixing now that the Dart stand-in has to mirror them.
            ExportLocale::En => match field {
                SummaryField::Total => "Total",
                SummaryField::Ok => "Ok",
                SummaryField::Failed => "Failed",
                SummaryField::Skipped => "Skipped",
                SummaryField::TotalBytes => "Total bytes",
                SummaryField::Truncated => "Truncated",
            },
            ExportLocale::Zh => match field {
                SummaryField::Total => "总数",
                SummaryField::Ok => "正常",
                SummaryField::Failed => "失败",
                SummaryField::Skipped => "跳过",
                SummaryField::TotalBytes => "总字节数",
                SummaryField::Truncated => "已截断",
            },
        }
    }

    /// 截断提示。
    ///
    /// `truncated=true` 这段在所有语言里都保留原样：它对应报告里的
    /// `summary.truncated` 字段，读者需要能把它和 JSON 里的键对上。
    pub fn truncation(&self, shown: usize, total: usize) -> String {
        match self.locale {
            ExportLocale::En => {
                format!("Truncated: showing {shown} of {total} entries (truncated=true).")
            }
            ExportLocale::Zh => {
                format!("已截断：共 {total} 条，仅显示 {shown} 条（truncated=true）。")
            }
        }
    }
}

impl Default for &'static Labels {
    fn default() -> Self {
        &Labels::EN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_map_or_reject() {
        assert_eq!(ExportLocale::from_code("zh"), Some(ExportLocale::Zh));
        assert_eq!(ExportLocale::from_code("zh-CN"), Some(ExportLocale::Zh));
        assert_eq!(ExportLocale::from_code("ZH-hans"), Some(ExportLocale::Zh));
        assert_eq!(ExportLocale::from_code(" zh_CN "), Some(ExportLocale::Zh));
        assert_eq!(ExportLocale::from_code("en"), Some(ExportLocale::En));
        assert_eq!(ExportLocale::from_code("en-US"), Some(ExportLocale::En));
        // 未知代码必须返回 None，而不是静默回退到默认语言。
        assert_eq!(ExportLocale::from_code("fr"), None);
        assert_eq!(ExportLocale::from_code(""), None);
    }

    #[test]
    fn default_locale_is_english() {
        assert_eq!(ExportLocale::default(), ExportLocale::En);
        assert_eq!(ExportLocale::En.as_str(), "en");
        assert_eq!(*<&Labels>::default(), Labels::EN);
    }

    #[test]
    fn english_table_and_text_styles_stay_distinct() {
        let en = &Labels::EN;
        assert_eq!(
            en.meta(MetaField::SuffixCase, LabelStyle::Table),
            "Suffix case"
        );
        assert_eq!(
            en.meta(MetaField::SuffixCase, LabelStyle::Text),
            "suffix_case"
        );
        assert_eq!(en.entry(EntryField::Path, LabelStyle::Text), "path");
        assert_eq!(
            en.summary(SummaryField::TotalBytes, LabelStyle::Table),
            "Total bytes"
        );
    }

    #[test]
    fn truncation_note_matches_the_historical_english_wording() {
        assert_eq!(
            Labels::EN.truncation(500, 501),
            "Truncated: showing 500 of 501 entries (truncated=true)."
        );
        assert!(Labels::ZH.truncation(500, 501).contains("truncated=true"));
    }
}
