//! Unified file-entry and root model shared by hashing, reports, realtime
//! directory comparison and rename planning.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::algorithm::AlgorithmId;

/// How the batch root is addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootKind {
    /// Filesystem path root (desktop, iOS, or a plain directory).
    Fs,
    /// Android Storage Access Framework tree root.
    Saf,
}

impl RootKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            RootKind::Fs => "fs",
            RootKind::Saf => "saf",
        }
    }
}

/// Per-entry processing status. Only `Ok` entries participate in hash comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Ok,
    Failed,
    Skipped,
}

impl EntryStatus {
    pub const fn is_ok(self) -> bool {
        matches!(self, EntryStatus::Ok)
    }
}

/// Machine-readable reason for a failed or skipped entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    PermissionDenied,
    NotFound,
    IsDirectory,
    IoError,
    Unsupported,
    Cancelled,
    /// The file changed while it was being scanned/hashed.
    Unstable,
    /// The path escaped the selected batch root.
    OutOfRoot,
    /// A suffix already exists on the filename and was left untouched.
    SuffixExists,
    /// The resulting filename is not valid on the target platform.
    InvalidName,
    /// Android SAF documents provider does not support renaming.
    RenameUnsupported,
    /// Platform rename call ran but failed.
    RenameFailed,
    /// Cross-session platform access (bookmark / persisted URI) was lost.
    PlatformAccessLost,
    /// Recorded hash does not match the recomputed hash.
    HashMismatch,
    /// Input could not be parsed.
    ParseError,
    /// Any other reason; `error_message` carries details.
    Other,
}

impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorKind::PermissionDenied => "permission_denied",
            ErrorKind::NotFound => "not_found",
            ErrorKind::IsDirectory => "is_directory",
            ErrorKind::IoError => "io_error",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Unstable => "unstable",
            ErrorKind::OutOfRoot => "out_of_root",
            ErrorKind::SuffixExists => "suffix_exists",
            ErrorKind::InvalidName => "invalid_name",
            ErrorKind::RenameUnsupported => "rename_unsupported",
            ErrorKind::RenameFailed => "rename_failed",
            ErrorKind::PlatformAccessLost => "platform_access_lost",
            ErrorKind::HashMismatch => "hash_mismatch",
            ErrorKind::ParseError => "parse_error",
            ErrorKind::Other => "other",
        }
    }

    /// Whether the entry should be treated as `unknown` in a diff rather than
    /// `removed` (e.g. cancellation must not fabricate removals).
    pub const fn yields_unknown_in_diff(self) -> bool {
        matches!(
            self,
            ErrorKind::Cancelled
                | ErrorKind::PermissionDenied
                | ErrorKind::PlatformAccessLost
                | ErrorKind::IoError
                | ErrorKind::Unstable
        )
    }
}

/// Non-portable platform audit data (Android document URI, iOS bookmark id).
/// Never part of the default diff comparison key.
pub type PlatformMeta = BTreeMap<String, String>;

/// Identity of a batch root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSpec {
    /// Stable ID of the batch root (batch-local).
    pub id: String,
    pub kind: RootKind,
    /// UI display path (absolute filesystem path or SAF tree display name).
    pub display_path: String,
    /// Platform-suggested case sensitivity, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_sensitive: Option<bool>,
}

/// One file inside a batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub root_id: String,
    pub root_kind: RootKind,
    /// Path relative to the batch root, always `/` separated. Report storage field.
    pub relative_path: String,
    /// Path shown in the UI.
    pub display_path: String,
    #[serde(default, skip_serializing_if = "PlatformMeta::is_empty")]
    pub platform_meta: PlatformMeta,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
    #[serde(default)]
    pub hashes: IndexMap<AlgorithmId, String>,
    pub status: EntryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ErrorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl FileEntry {
    /// Create an entry in the `ok`/empty-hash state, ready for hashing.
    pub fn new(
        root_id: impl Into<String>,
        root_kind: RootKind,
        relative_path: impl Into<String>,
        display_path: impl Into<String>,
        size: u64,
        mtime: Option<String>,
    ) -> Self {
        Self {
            root_id: root_id.into(),
            root_kind,
            relative_path: relative_path.into(),
            display_path: display_path.into(),
            platform_meta: PlatformMeta::new(),
            size,
            mtime,
            hashes: IndexMap::new(),
            status: EntryStatus::Ok,
            error_kind: None,
            error_message: None,
        }
    }

    /// Mark this entry as failed with a machine-readable reason.
    pub fn fail(&mut self, kind: ErrorKind, message: impl Into<String>) {
        self.status = EntryStatus::Failed;
        self.error_kind = Some(kind);
        self.error_message = Some(message.into());
        self.hashes.clear();
    }

    /// Mark this entry as skipped with a machine-readable reason.
    pub fn skip(&mut self, kind: ErrorKind, message: impl Into<String>) {
        self.status = EntryStatus::Skipped;
        self.error_kind = Some(kind);
        self.error_message = Some(message.into());
    }

    /// Whether this entry can take part in hash comparison.
    pub fn comparable(&self) -> bool {
        self.status.is_ok() && !self.hashes.is_empty()
    }
}

/// Error returned when a path is not a valid root-relative path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("relative path must not be empty")]
    Empty,
    #[error("relative path must not be absolute: {0}")]
    Absolute(String),
    #[error("relative path must not contain a Windows drive prefix: {0}")]
    DrivePrefixed(String),
    #[error("relative path must not contain a backslash: {0}")]
    Backslash(String),
    #[error("relative path escapes the batch root: {0}")]
    Escape(String),
    #[error("relative path contains a NUL byte")]
    NulByte,
}

/// Validate a root-relative path. This is the import-side guard that rejects
/// path traversal before any record reaches the diff engine (`docs/report-schema-v1.md`).
///
/// Rules:
/// * non-empty, no NUL, no backslash (reports always store `/`),
/// * not absolute, no drive prefix,
/// * no `..` segment that would escape the root (`a/../b` is accepted and
///   normalized by PathNormalizer; `../b` is rejected).
pub fn validate_relative_path(path: &str) -> Result<(), PathError> {
    if path.is_empty() {
        return Err(PathError::Empty);
    }
    if path.as_bytes().contains(&0) {
        return Err(PathError::NulByte);
    }
    if path.starts_with('/') {
        return Err(PathError::Absolute(path.to_string()));
    }
    if path.contains('\\') {
        return Err(PathError::Backslash(path.to_string()));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(PathError::DrivePrefixed(path.to_string()));
    }
    let mut depth: i32 = 0;
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return Err(PathError::Escape(path.to_string()));
                }
            }
            _ => depth += 1,
        }
    }
    Ok(())
}

/// Join a root-relative path onto a root display path for UI display.
/// Kept intentionally dumb: it never normalizes, it only joins.
pub fn join_display_path(root: &str, relative: &str) -> String {
    if root.is_empty() {
        return relative.to_string();
    }
    let sep = if root.contains(':') && !root.starts_with('/') {
        "\\"
    } else {
        "/"
    };
    format!("{}{}{}", root.trim_end_matches(['/', '\\']), sep, relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_validation_accepts_safe_paths() {
        for path in ["a.mkv", "sub/a.mkv", "sub/../a.mkv", "./a.mkv", "sub//a.mkv"] {
            assert!(validate_relative_path(path).is_ok(), "{path}");
        }
    }

    #[test]
    fn relative_path_validation_rejects_escapes() {
        assert_eq!(
            validate_relative_path("../a.mkv"),
            Err(PathError::Escape("../a.mkv".into()))
        );
        assert!(matches!(
            validate_relative_path("/etc/passwd"),
            Err(PathError::Absolute(_))
        ));
        assert!(matches!(
            validate_relative_path("C:/Windows"),
            Err(PathError::DrivePrefixed(_))
        ));
        assert!(matches!(
            validate_relative_path("sub\\a.mkv"),
            Err(PathError::Backslash(_))
        ));
        assert_eq!(validate_relative_path(""), Err(PathError::Empty));
    }

    #[test]
    fn entry_state_transitions_are_explicit() {
        let mut entry = FileEntry::new("root", RootKind::Fs, "a.mkv", "/tmp/a.mkv", 10, None);
        assert!(entry.status.is_ok());
        assert!(!entry.comparable());
        entry.hashes.insert(AlgorithmId::Blake3, "00".repeat(32));
        assert!(entry.comparable());
        entry.skip(ErrorKind::SuffixExists, "already suffixed");
        assert_eq!(entry.status, EntryStatus::Skipped);
        assert_eq!(entry.error_kind, Some(ErrorKind::SuffixExists));
    }

    #[test]
    fn display_path_join_handles_separators() {
        assert_eq!(join_display_path("/data/media", "sub/a.mkv"), "/data/media/sub/a.mkv");
        assert_eq!(join_display_path("C:\\media", "sub/a.mkv"), "C:\\media\\sub/a.mkv");
        assert_eq!(join_display_path("", "a.mkv"), "a.mkv");
    }
}
