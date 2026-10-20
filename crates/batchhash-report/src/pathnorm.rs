//! Path normalization for comparison keys (project plan §3.7.2 / §3.7.8).
//!
//! Two path spellings that name the same file on the target filesystem must map
//! to the same comparison key, without touching the filesystem:
//!
//! 1. resolve `.` and `..` segments — escaping the root is
//!    [`NormalizeError::OutOfRoot`],
//! 2. convert Windows separators (`\`) to `/`,
//! 3. collapse and trim separators,
//! 4. Unicode NFC,
//! 5. case policy (Unicode-aware `to_lowercase` when case-insensitive),
//! 6. NFC again (lowercasing can denormalize),
//!
//! NFKC is deliberately **not** applied: full-width `a` and ASCII `a` are
//! different filenames and must stay different keys. Symlinks are never
//! resolved because a report is a snapshot, not a live filesystem walk.

use std::fmt;

use unicode_normalization::UnicodeNormalization;

/// Which component of a path a normalization call produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizeMode {
    /// Normalize the complete path (default comparison key).
    Full,
    /// Normalize only the final path component (degraded basename mode).
    Basename,
}

impl NormalizeMode {
    /// Stable identifier used in diff reasons and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            NormalizeMode::Full => "full",
            NormalizeMode::Basename => "basename",
        }
    }
}

impl fmt::Display for NormalizeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Failure modes of [`PathNormalizer`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NormalizeError {
    /// The input (or its normalized form) is empty.
    #[error("path is empty")]
    Empty,
    /// A `..` segment would escape the comparison root.
    #[error("path escapes the comparison root")]
    OutOfRoot,
    /// The input is not a valid path (for example it contains a NUL byte).
    #[error("path is not valid: {0}")]
    Invalid(String),
}

/// Normalizes paths into deterministic comparison keys.
///
/// The only policy input is case sensitivity; the pipeline itself is pure and
/// platform independent so golden tests are reproducible on every host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathNormalizer {
    case_sensitive: bool,
}

impl Default for PathNormalizer {
    fn default() -> Self {
        Self::new(false)
    }
}

impl PathNormalizer {
    /// Create a normalizer with the given case policy.
    ///
    /// `case_sensitive == false` folds keys with the Unicode-aware lowercase
    /// mapping, so two spellings that a case-insensitive filesystem considers
    /// identical produce the same key.
    pub const fn new(case_sensitive: bool) -> Self {
        Self { case_sensitive }
    }

    /// Whether keys keep their original case.
    pub const fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Normalize a full path into its comparison key.
    ///
    /// Returns [`NormalizeError::Empty`] for empty input and
    /// [`NormalizeError::OutOfRoot`] when a `..` segment escapes the root.
    pub fn normalize(&self, path: &str) -> Result<String, NormalizeError> {
        self.normalize_with_mode(path, NormalizeMode::Full)
    }

    /// Normalize only the final path component.
    ///
    /// Used by degraded basename comparison. Dot segments are not resolved
    /// beyond selecting the last real component; callers are expected to have
    /// already decided that basename comparison is acceptable.
    pub fn normalize_basename(&self, path: &str) -> Result<String, NormalizeError> {
        self.normalize_with_mode(path, NormalizeMode::Basename)
    }

    /// Normalize using an explicit [`NormalizeMode`].
    pub fn normalize_with_mode(
        &self,
        path: &str,
        mode: NormalizeMode,
    ) -> Result<String, NormalizeError> {
        if path.is_empty() {
            return Err(NormalizeError::Empty);
        }
        if path.as_bytes().contains(&0) {
            return Err(NormalizeError::Invalid(
                "path contains a NUL byte".to_string(),
            ));
        }

        // Steps 2 and 3 are folded into the split below: separators are unified
        // before segment resolution so Windows-style input is handled too.
        let unified = path.replace('\\', "/");
        match mode {
            NormalizeMode::Full => {
                let mut components: Vec<&str> = Vec::new();
                for segment in unified.split('/') {
                    match segment {
                        "" | "." => {}
                        ".." => {
                            if components.pop().is_none() {
                                return Err(NormalizeError::OutOfRoot);
                            }
                        }
                        other => components.push(other),
                    }
                }
                if components.is_empty() {
                    // An absolute root such as `/` normalizes to the empty key;
                    // a relative "no components" path is not a usable key.
                    return if unified.starts_with('/') {
                        Ok(String::new())
                    } else {
                        Err(NormalizeError::Empty)
                    };
                }
                self.apply_unicode_policy(&components.join("/"))
            }
            NormalizeMode::Basename => {
                let mut last: Option<&str> = None;
                for segment in unified.split('/') {
                    if segment.is_empty() || segment == "." {
                        continue;
                    }
                    last = Some(segment);
                }
                match last {
                    None => Err(NormalizeError::Empty),
                    Some("..") => Err(NormalizeError::OutOfRoot),
                    Some(segment) => self.apply_unicode_policy(segment),
                }
            }
        }
    }

    /// Steps 4 to 6 of the pipeline: NFC, case policy, NFC again.
    fn apply_unicode_policy(&self, value: &str) -> Result<String, NormalizeError> {
        let nfc: String = value.nfc().collect();
        if nfc.is_empty() {
            return Err(NormalizeError::Empty);
        }
        // `str::to_lowercase` is the Unicode-aware full lowercase mapping and
        // can produce decomposed sequences, hence the second NFC pass.
        let folded = if self.case_sensitive {
            nfc
        } else {
            nfc.to_lowercase()
        };
        Ok(folded.nfc().collect())
    }

    /// Strip a normalized root prefix from an absolute path.
    ///
    /// The comparison happens on normalized keys and only on a path boundary:
    /// root `/data/media` matches `/data/media/a.mkv` but never
    /// `/data/medias/a.mkv`. Both paths must already be absolute-ish; when
    /// `root` normalizes to the empty key (filesystem root) the whole normalized
    /// path is returned.
    ///
    /// Returns [`NormalizeError::OutOfRoot`] when `absolute` is not under
    /// `root`, and an empty string when the two are the same path.
    pub fn relative_to_root(&self, absolute: &str, root: &str) -> Result<String, NormalizeError> {
        let absolute_key = self.normalize(absolute)?;
        let root_key = self.normalize(root)?;
        if root_key.is_empty() {
            return Ok(absolute_key);
        }
        if absolute_key == root_key {
            return Ok(String::new());
        }
        match absolute_key.strip_prefix(&root_key) {
            Some(rest) if rest.starts_with('/') => Ok(rest.trim_start_matches('/').to_string()),
            _ => Err(NormalizeError::OutOfRoot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insensitive() -> PathNormalizer {
        PathNormalizer::new(false)
    }

    fn sensitive() -> PathNormalizer {
        PathNormalizer::new(true)
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(insensitive().normalize(""), Err(NormalizeError::Empty));
        assert_eq!(
            insensitive().normalize_basename(""),
            Err(NormalizeError::Empty)
        );
    }

    #[test]
    fn nul_byte_is_invalid() {
        assert!(matches!(
            insensitive().normalize("a\u{0}b"),
            Err(NormalizeError::Invalid(_))
        ));
    }

    #[test]
    fn backslash_and_slash_are_the_same_key() {
        assert_eq!(insensitive().normalize(r"sub\a.mkv").unwrap(), "sub/a.mkv");
        assert_eq!(
            insensitive().normalize(r"sub\a.mkv").unwrap(),
            insensitive().normalize("sub/a.mkv").unwrap()
        );
        assert_eq!(
            insensitive().normalize(r"C:\media\a.mkv").unwrap(),
            "c:/media/a.mkv"
        );
    }

    #[test]
    fn dot_segments_are_resolved() {
        assert_eq!(
            insensitive().normalize("./sub/../sub/a.mkv").unwrap(),
            "sub/a.mkv"
        );
        assert_eq!(
            insensitive().normalize("./sub/../sub/a.mkv").unwrap(),
            insensitive().normalize("sub/a.mkv").unwrap()
        );
        assert_eq!(insensitive().normalize("sub/./a.mkv").unwrap(), "sub/a.mkv");
    }

    #[test]
    fn repeated_and_edge_separators_collapse() {
        assert_eq!(
            insensitive().normalize("//sub///a.mkv//").unwrap(),
            "sub/a.mkv"
        );
        assert_eq!(
            insensitive().normalize("sub//a.mkv").unwrap(),
            insensitive().normalize("sub/a.mkv").unwrap()
        );
    }

    #[test]
    fn nfc_and_nfd_spellings_are_the_same_key() {
        let nfd = "cafe\u{301}.mkv";
        let nfc = "caf\u{e9}.mkv";
        assert_eq!(insensitive().normalize(nfd).unwrap(), nfc);
        assert_eq!(
            insensitive().normalize(nfd).unwrap(),
            insensitive().normalize(nfc).unwrap()
        );
        // Case folding + NFC must handle uppercase decomposed input too.
        let upper_nfd = "CAFE\u{301}.MKV";
        assert_eq!(
            insensitive().normalize(upper_nfd).unwrap(),
            insensitive().normalize("caf\u{e9}.mkv").unwrap()
        );
    }

    #[test]
    fn case_insensitive_folds_and_case_sensitive_keeps() {
        assert_eq!(insensitive().normalize("A.MKV").unwrap(), "a.mkv");
        assert_eq!(
            insensitive().normalize("A.MKV").unwrap(),
            insensitive().normalize("a.mkv").unwrap()
        );
        assert_ne!(
            sensitive().normalize("A.MKV").unwrap(),
            sensitive().normalize("a.mkv").unwrap()
        );
        assert_eq!(sensitive().normalize("A.MKV").unwrap(), "A.MKV");
    }

    #[test]
    fn no_nfkc_folding_of_fullwidth_characters() {
        // U+FF41 FULLWIDTH LATIN SMALL LETTER A must stay distinct from ASCII a.
        let fullwidth = "\u{ff41}.mkv";
        assert_ne!(
            insensitive().normalize(fullwidth).unwrap(),
            insensitive().normalize("a.mkv").unwrap()
        );
        assert_eq!(insensitive().normalize(fullwidth).unwrap(), fullwidth);
    }

    #[test]
    fn escaping_dot_dot_is_out_of_root() {
        assert_eq!(
            insensitive().normalize("../a.mkv"),
            Err(NormalizeError::OutOfRoot)
        );
        assert_eq!(
            insensitive().normalize("sub/../../a.mkv"),
            Err(NormalizeError::OutOfRoot)
        );
        // Escapes cannot be laundered through backslashes either.
        assert_eq!(
            insensitive().normalize(r"..\a.mkv"),
            Err(NormalizeError::OutOfRoot)
        );
        // `a/../b` stays inside the root.
        assert_eq!(insensitive().normalize("a/../b.mkv").unwrap(), "b.mkv");
    }

    #[test]
    fn basename_mode_keeps_only_the_last_component() {
        assert_eq!(
            insensitive().normalize_basename("sub/../other/a.mkv").unwrap(),
            "a.mkv"
        );
        assert_eq!(
            insensitive().normalize_basename(r"dir\A.MKV").unwrap(),
            "a.mkv"
        );
        assert_eq!(
            insensitive().normalize_basename("/abs/path/cafe\u{301}.mkv").unwrap(),
            "caf\u{e9}.mkv"
        );
        assert_eq!(
            insensitive().normalize_basename(".."),
            Err(NormalizeError::OutOfRoot)
        );
    }

    #[test]
    fn relative_to_root_strips_only_on_a_boundary() {
        let normalizer = insensitive();
        assert_eq!(
            normalizer
                .relative_to_root("/data/media/sub/a.mkv", "/data/media")
                .unwrap(),
            "sub/a.mkv"
        );
        assert_eq!(
            normalizer
                .relative_to_root("/data/media/sub/a.mkv", "/data/media/")
                .unwrap(),
            "sub/a.mkv"
        );
        assert_eq!(
            normalizer
                .relative_to_root(r"D:\Media\Sub\a.mkv", r"d:\media")
                .unwrap(),
            "sub/a.mkv"
        );
        assert_eq!(
            normalizer.relative_to_root("/data/medias/a.mkv", "/data/media"),
            Err(NormalizeError::OutOfRoot)
        );
        assert_eq!(
            normalizer.relative_to_root("/data/media", "/data/media").unwrap(),
            ""
        );
        // A filesystem root prefix matches everything.
        assert_eq!(normalizer.relative_to_root("/a.mkv", "/").unwrap(), "a.mkv");
    }

    #[test]
    fn relative_check_is_case_sensitive_only_when_configured() {
        let insensitive = insensitive();
        assert_eq!(
            insensitive
                .relative_to_root("/DATA/MEDIA/Sub/A.MKV", "/data/media")
                .unwrap(),
            "sub/a.mkv"
        );
        let sensitive = sensitive();
        assert_eq!(
            sensitive.relative_to_root("/data/media/Sub/A.MKV", "/data/media"),
            Err(NormalizeError::OutOfRoot)
        );
    }
}
