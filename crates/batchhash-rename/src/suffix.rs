//! Filename suffix rules: format a truncated digest into `name[TOKEN].ext`,
//! parse existing suffixes and estimate suffix collisions.
//!
//! The filename format is part of the product contract:
//!
//! ```text
//! original.ext        -> original[ABCDE].ext
//! original            -> original[ABCDE]
//! original[ABCDE].ext -> skipped (already suffixed)
//! ```
//!
//! Bare suffixes are inherently ambiguous: several registered algorithms share
//! a digest length. [`parse_filename`] reports that through
//! [`ParsedSuffix::ambiguous`], and callers that need a self-describing token
//! should enable [`SuffixFormat::prefix_enabled`].

use batchhash_types::{
    default_suffix_algorithm, is_hex, spec_by_suffix_prefix, AlgorithmId, HexCase, REGISTRY,
};
use serde::{Deserialize, Serialize};

/// Default maximum filename length, in UTF-16 code units, used by the Windows
/// long-name precheck.
pub const DEFAULT_MAX_UTF16_LEN: usize = 255;

/// Minimum accepted suffix length in hex characters (the contract is 4..=16).
pub const MIN_SUFFIX_LENGTH: u8 = 4;

/// Maximum accepted suffix length in hex characters (the contract is 4..=16).
pub const MAX_SUFFIX_LENGTH: u8 = 16;

/// Configuration for the filename suffix written by a rename batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuffixFormat {
    /// Number of leading digest hex characters placed in the suffix (4..=16).
    pub length: u8,
    /// Letter case used for the hex token.
    pub case: HexCase,
    /// Algorithm whose digest supplies the token.
    pub algorithm: AlgorithmId,
    /// When true, emit `prefix:hex` (for example `blake3:ABCDE`) instead of a
    /// bare hex token. Off by default.
    pub prefix_enabled: bool,
}

impl Default for SuffixFormat {
    fn default() -> Self {
        Self {
            length: 5,
            case: HexCase::Upper,
            algorithm: default_suffix_algorithm(),
            prefix_enabled: false,
        }
    }
}

impl SuffixFormat {
    /// Validate the suffix length bounds and the algorithm usability.
    ///
    /// Returns [`SuffixError::InvalidLength`] for a length outside 4..=16 and
    /// [`SuffixError::AlgorithmNotSuffixUsable`] for an algorithm that must not
    /// be written into a filename.
    pub fn validate(&self) -> Result<(), SuffixError> {
        if !(MIN_SUFFIX_LENGTH..=MAX_SUFFIX_LENGTH).contains(&self.length) {
            return Err(SuffixError::InvalidLength(self.length));
        }
        if !self.algorithm.spec().suffix_usable {
            return Err(SuffixError::AlgorithmNotSuffixUsable(self.algorithm));
        }
        Ok(())
    }
}

/// Failure modes for suffix formatting and configuration validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SuffixError {
    /// The configured suffix length is outside 4..=16.
    #[error("suffix length must be between 4 and 16, got {0}")]
    InvalidLength(u8),
    /// The algorithm is registered but must not be used in a filename suffix.
    #[error("algorithm {0} is not usable for filename suffixes")]
    AlgorithmNotSuffixUsable(AlgorithmId),
    /// The supplied digest is shorter than the configured suffix length.
    #[error("digest has {actual} hex characters, suffix needs at least {required}")]
    DigestTooShort {
        /// Number of hex characters required by the suffix format.
        required: usize,
        /// Number of hex characters supplied by the caller.
        actual: usize,
    },
    /// The supplied digest is not a non-empty ASCII hex string.
    #[error("digest is not a valid hex string")]
    InvalidDigest,
    /// The name already carries a parseable suffix; renaming again would nest.
    #[error("filename already carries a parseable suffix")]
    AlreadySuffixed,
    /// The bracket group before the extension is neither hex nor `prefix:hex`.
    #[error("filename has an unparseable suffix-like bracket group")]
    UnparseableName,
    /// The formatted name cannot fit into the platform filename limit.
    #[error(
        "formatted name needs {utf16_len} UTF-16 code units, limit is {max_utf16_len}"
    )]
    NameTooLong {
        /// Length of the candidate name in UTF-16 code units.
        utf16_len: usize,
        /// Maximum number of UTF-16 code units allowed by the caller.
        max_utf16_len: usize,
    },
    /// The filename passed to the formatter is empty.
    #[error("filename is empty")]
    EmptyName,
}

/// One suffix parsed out of a filename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedSuffix {
    /// Raw bracket content exactly as found in the filename, case preserved
    /// (for example `ABCDE` or `blake3:ABCDE`).
    pub token: String,
    /// Lowercase hex portion of the token (the digest prefix).
    pub hex: String,
    /// Algorithm inferred from a recognized `prefix:hex` token or from a bare
    /// hex token whose length maps to exactly one registered algorithm.
    pub algorithm: Option<AlgorithmId>,
    /// True when the bare hex length maps to more than one algorithm, so
    /// [`ParsedSuffix::algorithm`] stays [`None`].
    pub ambiguous: bool,
}

/// Result of inspecting the bracket group immediately before a filename
/// extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuffixParseResult {
    /// No bracket group immediately before the extension.
    NotSuffixed,
    /// A valid `[token]` or `[prefix:hex]` suffix was found.
    Parsed(ParsedSuffix),
    /// A bracket group exists but its content is neither hex of length 4..=16
    /// nor a recognized `prefix:hex` token. A rename planner must skip such
    /// names rather than nesting a new suffix.
    Unparseable,
}

/// Format `name` with a truncated digest suffix, preserving the extension.
///
/// The result is `stem[token]extension` where `token` is the first
/// [`SuffixFormat::length`] hex characters of `digest_hex` in the configured
/// case, optionally prefixed with the algorithm prefix (`prefix:hex`). A name
/// without an extension gets the suffix appended at the end.
///
/// When the result exceeds `max_utf16_len` UTF-16 code units, the stem is
/// truncated (never the suffix or the extension), keeping at least one stem
/// character. If even that does not fit, [`SuffixError::NameTooLong`] is
/// returned.
///
/// Names that are already suffixed yield [`SuffixError::AlreadySuffixed`], and
/// names whose bracket group looks suffix-like but is malformed yield
/// [`SuffixError::UnparseableName`].
pub fn format_filename(
    name: &str,
    digest_hex: &str,
    format: &SuffixFormat,
    max_utf16_len: usize,
) -> Result<String, SuffixError> {
    if name.is_empty() {
        return Err(SuffixError::EmptyName);
    }
    format.validate()?;
    if !is_hex(digest_hex) {
        return Err(SuffixError::InvalidDigest);
    }
    let required = usize::from(format.length);
    if digest_hex.len() < required {
        return Err(SuffixError::DigestTooShort {
            required,
            actual: digest_hex.len(),
        });
    }
    match parse_filename(name) {
        SuffixParseResult::Parsed(_) => return Err(SuffixError::AlreadySuffixed),
        SuffixParseResult::Unparseable => return Err(SuffixError::UnparseableName),
        SuffixParseResult::NotSuffixed => {}
    }

    let (stem, extension) = split_name(name);
    let hex = &digest_hex[..required];
    let token = match format.case {
        HexCase::Upper => hex.to_ascii_uppercase(),
        HexCase::Lower => hex.to_ascii_lowercase(),
    };
    let token = if format.prefix_enabled {
        format!("{}:{}", format.algorithm.spec().suffix_prefix, token)
    } else {
        token
    };
    let suffix = format!("[{token}]");
    let candidate = format!("{stem}{suffix}{extension}");
    let candidate_len = utf16_len(&candidate);
    if candidate_len <= max_utf16_len {
        return Ok(candidate);
    }

    let fixed_len = utf16_len(&suffix) + utf16_len(extension);
    let keep_units = max_utf16_len.saturating_sub(fixed_len);
    if keep_units == 0 {
        return Err(SuffixError::NameTooLong {
            utf16_len: candidate_len,
            max_utf16_len,
        });
    }
    let mut truncated_stem = String::new();
    let mut used_units = 0usize;
    for character in stem.chars() {
        let width = character.len_utf16();
        if used_units + width > keep_units {
            break;
        }
        truncated_stem.push(character);
        used_units += width;
    }
    if truncated_stem.is_empty() {
        return Err(SuffixError::NameTooLong {
            utf16_len: candidate_len,
            max_utf16_len,
        });
    }
    let truncated = format!("{truncated_stem}{suffix}{extension}");
    let truncated_len = utf16_len(&truncated);
    if truncated_len > max_utf16_len {
        return Err(SuffixError::NameTooLong {
            utf16_len: truncated_len,
            max_utf16_len,
        });
    }
    Ok(truncated)
}

/// Inspect the bracket group immediately before the extension of `name`.
///
/// The inspected group is the last `[` followed by a `]` at the end of the
/// stem (the name without its final extension). [`SuffixParseResult::NotSuffixed`]
/// means there is no such group, [`SuffixParseResult::Unparseable`] means the
/// group content is malformed, and [`SuffixParseResult::Parsed`] carries the
/// token plus algorithm inference.
pub fn parse_filename(name: &str) -> SuffixParseResult {
    let (stem, _extension) = split_name(name);
    if !stem.ends_with(']') {
        return SuffixParseResult::NotSuffixed;
    }
    let Some(open) = stem.rfind('[') else {
        return SuffixParseResult::NotSuffixed;
    };
    let content = &stem[open + 1..stem.len() - 1];
    match parse_suffix_token(content) {
        Some(parsed) => SuffixParseResult::Parsed(parsed),
        None => SuffixParseResult::Unparseable,
    }
}

/// Birthday-collision probability for `count` filenames sharing one suffix
/// alphabet of `16^length` tokens.
///
/// The estimate is `1 - exp(-count * (count - 1) / (2 * 16^length))` clamped
/// into `0.0..=1.0`. It is monotonic: growing with `count` and shrinking with
/// `length`.
pub fn collision_probability(count: usize, length: u8) -> f64 {
    if count <= 1 {
        return 0.0;
    }
    let space = 16f64.powi(i32::from(length));
    if !space.is_finite() {
        return 0.0;
    }
    if space <= 1.0 {
        return 1.0;
    }
    let count = count as f64;
    let exponent = -(count * (count - 1.0)) / (2.0 * space);
    (1.0 - exponent.exp()).clamp(0.0, 1.0)
}

/// Recommended default suffix length for a batch of `count` files.
///
/// The table is part of the product contract: up to 100 files use 5 hex
/// characters, up to 1,000 use 8, up to 100,000 use 10 and larger batches use
/// 12.
pub fn recommended_length(count: usize) -> u8 {
    if count <= 100 {
        5
    } else if count <= 1_000 {
        8
    } else if count <= 100_000 {
        10
    } else {
        12
    }
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    }
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// Longest hex token accepted when parsing an existing filename.
///
/// Writing is limited to MIN_SUFFIX_LENGTH..=MAX_SUFFIX_LENGTH, but reverse
/// extraction must also recognise full digests written by other tools (a 40
/// character SHA-1, a 64 character BLAKE3, and so on), so parsing accepts any
/// length up to the longest registered digest.
fn max_parse_token_length() -> usize {
    REGISTRY
        .iter()
        .map(|spec| spec.hex_len)
        .max()
        .unwrap_or(usize::from(MAX_SUFFIX_LENGTH))
}

fn is_hex_token(value: &str) -> bool {
    let min = usize::from(MIN_SUFFIX_LENGTH);
    (min..=max_parse_token_length()).contains(&value.len()) && is_hex(value)
}

fn parse_suffix_token(content: &str) -> Option<ParsedSuffix> {
    if let Some((prefix, hex)) = content.split_once(':') {
        let spec = spec_by_suffix_prefix(prefix)?;
        if !is_hex_token(hex) {
            return None;
        }
        return Some(ParsedSuffix {
            token: content.to_string(),
            hex: hex.to_ascii_lowercase(),
            algorithm: Some(spec.id),
            ambiguous: false,
        });
    }
    if !is_hex_token(content) {
        return None;
    }
    let inferred = AlgorithmId::infer_by_hex_len(content.len());
    let algorithm = if inferred.len() == 1 {
        inferred.first().copied()
    } else {
        None
    };
    Some(ParsedSuffix {
        token: content.to_string(),
        hex: content.to_ascii_lowercase(),
        algorithm,
        ambiguous: inferred.len() > 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use batchhash_types::{AlgorithmId, HexCase};

    fn digest() -> String {
        "abcdef0123456789".repeat(4)
    }

    fn upper_format(length: u8) -> SuffixFormat {
        SuffixFormat {
            length,
            case: HexCase::Upper,
            algorithm: AlgorithmId::Blake3,
            prefix_enabled: false,
        }
    }

    #[test]
    fn format_preserves_extension_and_pads_suffix() {
        let format = upper_format(5);
        let formatted = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting a plain name must succeed");
        assert_eq!(formatted, "movie[ABCDE].mkv");
    }

    #[test]
    fn format_handles_lower_case() {
        let format = SuffixFormat {
            case: HexCase::Lower,
            ..upper_format(5)
        };
        let formatted = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting a plain name must succeed");
        assert_eq!(formatted, "movie[abcde].mkv");
    }

    #[test]
    fn format_handles_no_extension_and_multi_dot_names() {
        let format = upper_format(5);
        let no_extension = format_filename("README", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting a name without extension must succeed");
        assert_eq!(no_extension, "README[ABCDE]");

        let multi_dot = format_filename("archive.tar.gz", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting a multi-dot name must succeed");
        assert_eq!(multi_dot, "archive.tar[ABCDE].gz");
    }

    #[test]
    fn format_handles_hidden_files() {
        let format = upper_format(5);
        let hidden = format_filename(".bashrc", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting a hidden file must succeed");
        assert_eq!(hidden, ".bashrc[ABCDE]");
    }

    #[test]
    fn format_can_add_algorithm_prefix() {
        let format = SuffixFormat {
            length: 5,
            case: HexCase::Upper,
            algorithm: AlgorithmId::Blake3,
            prefix_enabled: true,
        };
        let formatted = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting with prefix must succeed");
        assert_eq!(formatted, "movie[blake3:ABCDE].mkv");
    }

    #[test]
    fn format_rejects_already_suffixed_or_unparseable_names() {
        let format = upper_format(5);
        assert_eq!(
            format_filename("movie[ABCDE].mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::AlreadySuffixed)
        );
        assert_eq!(
            format_filename("movie[blake3:ABCDE].mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::AlreadySuffixed)
        );
        assert_eq!(
            format_filename("movie[1080p].mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::UnparseableName)
        );
        assert_eq!(
            format_filename("", &digest(), &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::EmptyName)
        );
    }

    #[test]
    fn format_and_parse_round_trip() {
        let format = upper_format(5);
        let formatted = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting must succeed");
        match parse_filename(&formatted) {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.token, "ABCDE");
                assert_eq!(parsed.hex, "abcde");
                assert_eq!(parsed.algorithm, None);
                assert!(!parsed.ambiguous);
            }
            other => panic!("expected a parsed suffix, got {other:?}"),
        }

        let prefixed = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting must succeed");
        assert_eq!(prefixed, "movie[ABCDE].mkv");
        let format = SuffixFormat {
            length: 16,
            prefix_enabled: true,
            ..upper_format(16)
        };
        let formatted = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("formatting with prefix must succeed");
        match parse_filename(&formatted) {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.token, "blake3:ABCDEF0123456789");
                assert_eq!(parsed.hex, "abcdef0123456789");
                assert_eq!(parsed.algorithm, Some(AlgorithmId::Blake3));
                assert!(!parsed.ambiguous);
            }
            other => panic!("expected a parsed prefix suffix, got {other:?}"),
        }
    }

    #[test]
    fn parse_table_covers_length_inference() {
        match parse_filename("movie[2020].mkv") {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.token, "2020");
                assert_eq!(parsed.hex, "2020");
                assert_eq!(parsed.algorithm, None);
                assert!(!parsed.ambiguous);
            }
            other => panic!("expected parsed 4-hex token, got {other:?}"),
        }

        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        match parse_filename(&format!("movie[{sha1}].mkv")) {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.algorithm, Some(AlgorithmId::Sha1));
                assert!(!parsed.ambiguous);
            }
            other => panic!("expected parsed sha1 token, got {other:?}"),
        }

        let crc = "01234567";
        match parse_filename(&format!("movie[{crc}].mkv")) {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.algorithm, None);
                assert!(parsed.ambiguous);
            }
            other => panic!("expected ambiguous crc-length token, got {other:?}"),
        }

        let long = "0123456789ABCDEF";
        match parse_filename(&format!("movie[{long}].mkv")) {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.hex, "0123456789abcdef");
                assert_eq!(parsed.algorithm, None);
                assert!(parsed.ambiguous);
            }
            other => panic!("expected ambiguous 16-hex token, got {other:?}"),
        }
    }

    #[test]
    fn parse_reports_not_suffixed_without_a_bracket_group() {
        assert_eq!(parse_filename("movie.mkv"), SuffixParseResult::NotSuffixed);
        assert_eq!(
            parse_filename("movie[ABCDE].tar.gz"),
            SuffixParseResult::NotSuffixed
        );
        assert_eq!(
            parse_filename("movie[ABCDE"),
            SuffixParseResult::NotSuffixed
        );
        assert_eq!(
            parse_filename("movie[ABCDE]extra.mkv"),
            SuffixParseResult::NotSuffixed
        );
        assert_eq!(parse_filename(""), SuffixParseResult::NotSuffixed);
    }

    #[test]
    fn parse_reports_malformed_bracket_groups() {
        assert_eq!(
            parse_filename("movie[].mkv"),
            SuffixParseResult::Unparseable
        );
        assert_eq!(
            parse_filename("movie[zz].mkv"),
            SuffixParseResult::Unparseable
        );
        assert_eq!(
            parse_filename("movie[sha999:ABCD].mkv"),
            SuffixParseResult::Unparseable
        );
        assert_eq!(
            parse_filename("movie[blake3:ZZ].mkv"),
            SuffixParseResult::Unparseable
        );
        assert_eq!(
            parse_filename("movie[ABCDE:ABCDE].mkv"),
            SuffixParseResult::Unparseable
        );
    }

    #[test]
    fn parse_accepts_prefix_case_insensitively() {
        match parse_filename("movie[BLAKE3:ABCDE].mkv") {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.algorithm, Some(AlgorithmId::Blake3));
                assert_eq!(parsed.token, "BLAKE3:ABCDE");
                assert_eq!(parsed.hex, "abcde");
            }
            other => panic!("expected parsed prefix token, got {other:?}"),
        }
    }

    #[test]
    fn truncates_cjk_stem_to_fit_utf16_limit() {
        let format = upper_format(5);
        let formatted = format_filename("日本語.txt", &digest(), &format, 12)
            .expect("stem truncation must succeed");
        assert_eq!(formatted, "日[ABCDE].txt");
        assert_eq!(formatted.encode_utf16().count(), 12);
    }

    #[test]
    fn truncates_emoji_stem_on_code_point_boundaries() {
        let format = upper_format(5);
        let formatted = format_filename("ab😀cd", &digest(), &format, 10)
            .expect("stem truncation must succeed");
        assert_eq!(formatted, "ab[ABCDE]");
        assert!(formatted.encode_utf16().count() <= 10);
    }

    #[test]
    fn reports_name_too_long_when_no_stem_character_fits() {
        let format = upper_format(5);
        assert!(matches!(
            format_filename("a.txt", &digest(), &format, 10),
            Err(SuffixError::NameTooLong { .. })
        ));
        assert!(matches!(
            format_filename("😀abc", &digest(), &format, 8),
            Err(SuffixError::NameTooLong { .. })
        ));
        assert!(matches!(
            format_filename("abc.txt", &digest(), &format, 3),
            Err(SuffixError::NameTooLong { .. })
        ));
    }

    #[test]
    fn validate_rejects_out_of_range_lengths() {
        for length in [0u8, 3, 17, 255] {
            let format = upper_format(length);
            assert_eq!(
                format.validate(),
                Err(SuffixError::InvalidLength(length)),
                "length {length}"
            );
        }
        for length in MIN_SUFFIX_LENGTH..=MAX_SUFFIX_LENGTH {
            assert!(upper_format(length).validate().is_ok(), "length {length}");
        }
        for algorithm in AlgorithmId::ALL {
            let format = SuffixFormat {
                algorithm,
                ..upper_format(8)
            };
            assert!(format.validate().is_ok(), "algorithm {algorithm}");
        }
    }

    #[test]
    fn format_rejects_bad_digests() {
        let format = upper_format(8);
        assert_eq!(
            format_filename("movie.mkv", "abc", &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::DigestTooShort {
                required: 8,
                actual: 3,
            })
        );
        assert_eq!(
            format_filename("movie.mkv", "zzzzzzzz", &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::InvalidDigest)
        );
        assert_eq!(
            format_filename("movie.mkv", "", &format, DEFAULT_MAX_UTF16_LEN),
            Err(SuffixError::InvalidDigest)
        );
    }

    #[test]
    fn collision_probability_is_monotonic_and_bounded() {
        assert_eq!(collision_probability(0, 5), 0.0);
        assert_eq!(collision_probability(1, 5), 0.0);
        let small = collision_probability(100, 5);
        let large = collision_probability(10_000, 5);
        let longer = collision_probability(10_000, 8);
        assert!((0.0..=1.0).contains(&small));
        assert!((0.0..=1.0).contains(&large));
        assert!((0.0..=1.0).contains(&longer));
        assert!(small < large);
        assert!(longer < large);
        assert_eq!(collision_probability(2, 0), 1.0);
    }

    #[test]
    fn recommended_length_matches_the_contract_table() {
        assert_eq!(recommended_length(0), 5);
        assert_eq!(recommended_length(1), 5);
        assert_eq!(recommended_length(100), 5);
        assert_eq!(recommended_length(101), 8);
        assert_eq!(recommended_length(1_000), 8);
        assert_eq!(recommended_length(1_001), 10);
        assert_eq!(recommended_length(100_000), 10);
        assert_eq!(recommended_length(100_001), 12);
    }
}
