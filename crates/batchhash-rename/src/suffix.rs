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
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

/// Default maximum filename length, in UTF-16 code units, used by the Windows
/// long-name precheck.
pub const DEFAULT_MAX_UTF16_LEN: usize = 255;

/// Largest filename length, in bytes, that Linux and macOS filesystems accept.
///
/// Windows limits names by UTF-16 code units, but Linux and macOS limit them by
/// bytes (typically 255). A stem of CJK characters is three bytes each, so a
/// name that fits the UTF-16 limit can still exceed the byte limit and fail at
/// rename time. Both limits are enforced.
pub const MAX_NAME_BYTES: usize = 255;

/// Separator placed between the tokens of a multi-algorithm suffix group.
///
/// It is deliberately a character that can never appear in a hex token or in an
/// algorithm prefix, so a combined group stays unambiguous when parsed back.
pub const MULTI_SUFFIX_SEPARATOR: char = '+';

/// Separator written between an algorithm name and its digest.
///
/// `-` rather than `:`: a colon is illegal in a filename on Windows NTFS, and
/// also on exFAT, FAT32 and most SMB/NAS shares, which is exactly the media a
/// cross-machine verification tool is used on. macOS additionally renders a
/// colon in Finder as a slash, making the name look like a path.
pub const SUFFIX_PREFIX_SEPARATOR: char = '-';

/// Separator accepted when reading a suffix, in addition to
/// [`SUFFIX_PREFIX_SEPARATOR`].
///
/// Older releases wrote `[blake3:ABCDEF]`. Those names exist on disk and must
/// keep parsing, or every file tagged by an earlier version would look
/// untagged and be renamed again.
pub const LEGACY_SUFFIX_PREFIX_SEPARATOR: char = ':';

/// Maximum number of algorithms that may be combined into one filename suffix.
///
/// The cap exists so a filename cannot be inflated without bound: sixteen tokens
/// of up to 16 hex characters plus separators still fit every platform limit
/// once the stem is truncated.
pub const MAX_SUFFIX_PARTS: usize = 16;

/// Minimum accepted suffix length in hex characters.
///
/// Zero is allowed on purpose: a zero-length suffix writes only the separator
/// and the prefix, which is a legitimate way to tag a batch without embedding
/// any digest. Rejecting it forced the UI to start at 4 while still offering
/// the slider, so the page and the core disagreed about the same control.
pub const MIN_SUFFIX_LENGTH: u8 = 0;

/// Maximum accepted suffix length in hex characters.
///
/// This is the hard ceiling for parsing. A written format is additionally
/// bounded by the digest it reads from, which is per algorithm; see
/// [SuffixFormat::validate].
pub const MAX_SUFFIX_LENGTH: u8 = 64;

/// Configuration for the filename suffix written by a rename batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuffixFormat {
    /// Number of leading digest hex characters placed in the suffix.
    ///
    /// Bounded by 0..=MAX_SUFFIX_LENGTH here and further by the selected
    /// algorithm's own digest width in [SuffixFormat::validate].
    pub length: u8,
    /// Letter case used for the hex token.
    pub case: HexCase,
    /// Algorithm whose digest supplies the token.
    pub algorithm: AlgorithmId,
    /// When true, emit `prefix-hex` (for example `blake3-ABCDE`) instead of a
    /// bare hex token. Off by default.
    pub prefix_enabled: bool,
}

impl Default for SuffixFormat {
    fn default() -> Self {
        Self {
            length: 6,
            case: HexCase::Upper,
            algorithm: default_suffix_algorithm(),
            prefix_enabled: false,
        }
    }
}

impl SuffixFormat {
    /// Validate the suffix length bounds and the algorithm usability.
    ///
    /// Returns [`SuffixError::InvalidLength`] when the length exceeds either
    /// the hard ceiling or the selected algorithm's digest width, and
    /// [`SuffixError::AlgorithmNotSuffixUsable`] for an algorithm that must not
    /// be written into a filename.
    ///
    /// The per-algorithm ceiling matters: asking for more hex characters than
    /// the digest holds could only pad or silently truncate, and both make the
    /// written suffix disagree with the digest it claims to encode.
    pub fn validate(&self) -> Result<(), SuffixError> {
        if !self.algorithm.spec().suffix_usable {
            return Err(SuffixError::AlgorithmNotSuffixUsable(self.algorithm));
        }
        let ceiling = self
            .algorithm
            .spec()
            .hex_len
            .min(usize::from(MAX_SUFFIX_LENGTH));
        if usize::from(self.length) > ceiling {
            return Err(SuffixError::InvalidLength(self.length));
        }
        Ok(())
    }
}

/// One or more algorithm tokens written into a single filename suffix group.
///
/// A one-part format reproduces the historical behaviour byte for byte
/// (`movie[ABCDEF].mkv`), and a multi-part format joins the tokens with
/// [`MULTI_SUFFIX_SEPARATOR`] inside one bracket group
/// (`movie[blake3-ABCDEF+md5-123456].mkv`). Keeping the tokens in one group is
/// what makes reverse extraction safe: the suffix is still the last bracket group
/// before the extension, and an unparseable group is still reported as such
/// instead of being half-consumed.
///
/// The type deserializes from both shapes so stored plans and older clients keep
/// working:
///
/// * `{"length":6,"case":"upper","algorithm":"blake3","prefix":false}` (legacy,
///   one implicit part),
/// * `{"parts":[{...},{...}]}` (explicit, one or more parts).
///
/// Serialization always emits `{"parts":[...]}` with the normalized part list, so
/// round-tripping a legacy document upgrades it without losing information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MultiSuffixFormat {
    /// Algorithms to write, in the order their tokens appear in the filename.
    parts: Vec<SuffixFormat>,
}

impl MultiSuffixFormat {
    /// Build a format from an explicit, ordered part list.
    ///
    /// Returns [`SuffixError::EmptySuffixFormat`] when `parts` is empty and
    /// [`SuffixError::TooManySuffixParts`] above [`MAX_SUFFIX_PARTS`].
    pub fn new(parts: Vec<SuffixFormat>) -> Result<Self, SuffixError> {
        if parts.is_empty() {
            return Err(SuffixError::EmptySuffixFormat);
        }
        if parts.len() > MAX_SUFFIX_PARTS {
            return Err(SuffixError::TooManySuffixParts {
                count: parts.len(),
                max: MAX_SUFFIX_PARTS,
            });
        }
        Ok(Self { parts })
    }

    /// Format with exactly one part: identical to [`format_filename`] with `format`.
    pub fn single(format: SuffixFormat) -> Self {
        Self {
            parts: vec![format],
        }
    }

    /// The ordered parts.
    pub fn parts(&self) -> &[SuffixFormat] {
        &self.parts
    }

    /// Number of algorithms written into the suffix group.
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Whether the format carries no parts.
    ///
    /// Every public constructor rejects an empty part list, so this is only true
    /// for a value built directly from the (private) field.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Whether more than one algorithm is written.
    pub fn is_multi(&self) -> bool {
        self.parts.len() > 1
    }

    /// The single part, when the format is not multi-algorithm.
    pub fn as_single(&self) -> Option<SuffixFormat> {
        match self.parts.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The algorithms written, in filename order.
    pub fn algorithms(&self) -> Vec<AlgorithmId> {
        self.parts.iter().map(|part| part.algorithm).collect()
    }

    /// Validate every part, then the combination.
    ///
    /// On top of the per-part checks this rejects repeated algorithms, because
    /// two tokens of the same algorithm add no information to a filename and make
    /// the rendered group needlessly ambiguous.
    pub fn validate(&self) -> Result<(), SuffixError> {
        if self.parts.is_empty() {
            return Err(SuffixError::EmptySuffixFormat);
        }
        if self.parts.len() > MAX_SUFFIX_PARTS {
            return Err(SuffixError::TooManySuffixParts {
                count: self.parts.len(),
                max: MAX_SUFFIX_PARTS,
            });
        }
        let mut seen: Vec<AlgorithmId> = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            part.validate()?;
            if seen.contains(&part.algorithm) {
                return Err(SuffixError::DuplicateSuffixAlgorithm(part.algorithm));
            }
            seen.push(part.algorithm);
        }
        Ok(())
    }

    /// Whether a multi-algorithm group needs an algorithm prefix on every part
    /// to stay attributable when parsed back.
    pub fn requires_prefix(&self) -> bool {
        self.is_multi() && self.parts.iter().any(|part| !part.prefix_enabled)
    }
}

impl Default for MultiSuffixFormat {
    fn default() -> Self {
        Self::single(SuffixFormat::default())
    }
}

impl From<SuffixFormat> for MultiSuffixFormat {
    fn from(format: SuffixFormat) -> Self {
        Self::single(format)
    }
}

impl<'de> Deserialize<'de> for MultiSuffixFormat {
    /// Accept both the legacy single-object shape and the explicit part list.
    ///
    /// A bare object is read as one part, which is exactly how a stored
    /// single-algorithm plan was written before multi-algorithm support existed.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MultiSuffixVisitor;

        impl<'de> Visitor<'de> for MultiSuffixVisitor {
            type Value = MultiSuffixFormat;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a suffix format object or a {\"parts\": [...]} object")
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let raw = LegacyOrParts::deserialize(de::value::MapAccessDeserializer::new(map))?;
                match raw {
                    LegacyOrParts::Legacy(format) => Ok(MultiSuffixFormat::single(format)),
                    LegacyOrParts::Parts { parts } => {
                        MultiSuffixFormat::new(parts).map_err(de::Error::custom)
                    }
                }
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let parts =
                    Vec::<SuffixFormat>::deserialize(de::value::SeqAccessDeserializer::new(seq))?;
                MultiSuffixFormat::new(parts).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(MultiSuffixVisitor)
    }
}

/// Either shape accepted by [`MultiSuffixFormat`]'s deserializer.
///
/// `parts` is matched first; when the key is absent the same map is read as a
/// single [`SuffixFormat`], so a legacy document needs no migration.
#[derive(Deserialize)]
#[serde(untagged)]
enum LegacyOrParts {
    /// Explicit `{"parts": [...]}`.
    Parts {
        /// Ordered parts.
        parts: Vec<SuffixFormat>,
    },
    /// Legacy single-algorithm object.
    Legacy(SuffixFormat),
}

/// Failure modes for suffix formatting and configuration validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SuffixError {
    /// The configured suffix length exceeds the algorithm's digest width.
    #[error("suffix length {0} exceeds the digest width available for this algorithm")]
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
    #[error("formatted name needs {utf16_len} UTF-16 code units, limit is {max_utf16_len}")]
    NameTooLong {
        /// Length of the candidate name in UTF-16 code units.
        utf16_len: usize,
        /// Maximum number of UTF-16 code units allowed by the caller.
        max_utf16_len: usize,
    },
    /// The filename passed to the formatter is empty.
    #[error("filename is empty")]
    EmptyName,
    /// A suffix format was requested with no algorithms.
    #[error("a suffix format needs at least one algorithm")]
    EmptySuffixFormat,
    /// More algorithms were requested than one filename suffix group may hold.
    #[error("a suffix group holds at most {max} algorithms, got {count}")]
    TooManySuffixParts {
        /// Number of parts supplied by the caller.
        count: usize,
        /// Maximum number of parts allowed.
        max: usize,
    },
    /// The same algorithm was requested twice in one suffix group.
    #[error("algorithm {0} appears twice in one suffix group")]
    DuplicateSuffixAlgorithm(AlgorithmId),
}

/// One suffix parsed out of a filename.
///
/// A single-algorithm token leaves [`ParsedSuffix::parts`] empty and serializes
/// exactly as before, so stored JSON and older clients see no change. A combined
/// group fills [`ParsedSuffix::parts`] with one entry per algorithm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedSuffix {
    /// Raw bracket content exactly as found in the filename, case preserved
    /// (for example `ABCDE` or `blake3-ABCDE`).
    pub token: String,
    /// Lowercase hex portion of the token (the digest prefix).
    pub hex: String,
    /// Algorithm inferred from a recognized `prefix:hex` token or from a bare
    /// hex token whose length maps to exactly one registered algorithm.
    pub algorithm: Option<AlgorithmId>,
    /// True when the bare hex length maps to more than one algorithm, so
    /// [`ParsedSuffix::algorithm`] stays [`None`].
    pub ambiguous: bool,
    /// Individual parts of a combined `a+b` group, in filename order.
    ///
    /// Empty for a single-algorithm token; skipped when serializing so a
    /// single-algorithm suffix produces exactly the JSON it always did.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ParsedSuffix>,
}

impl ParsedSuffix {
    /// Whether this token combines more than one algorithm.
    pub fn is_multi(&self) -> bool {
        !self.parts.is_empty()
    }

    /// Algorithms named by the token, in filename order.
    ///
    /// A single token reports its inferred algorithm when unambiguous, so a
    /// caller can decide whether verification is possible at all.
    pub fn algorithms(&self) -> Vec<AlgorithmId> {
        if self.parts.is_empty() {
            return self.algorithm.into_iter().collect();
        }
        self.parts
            .iter()
            .filter_map(|part| part.algorithm)
            .collect()
    }
}

/// Result of inspecting the bracket group immediately before a filename
/// extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuffixParseResult {
    /// No bracket group immediately before the extension.
    NotSuffixed,
    /// A valid `[token]` or `[prefix:hex]` suffix was found.
    Parsed(ParsedSuffix),
    /// A bracket group exists but its content is neither hex of a length the
    /// parser accepts
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
        format!(
            "{}{SUFFIX_PREFIX_SEPARATOR}{token}",
            format.algorithm.spec().suffix_prefix
        )
    } else {
        token
    };
    let suffix = format!("[{token}]");
    let candidate = format!("{stem}{suffix}{extension}");
    let candidate_len = utf16_len(&candidate);
    if candidate_len <= max_utf16_len && candidate.len() <= MAX_NAME_BYTES {
        return Ok(candidate);
    }

    let fixed = format!("{suffix}{extension}");
    let truncated_stem = truncate_stem_for_limits(&stem, &fixed, max_utf16_len);
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

/// Format `name` with a multi-algorithm suffix group, preserving the extension.
///
/// A one-part format is delegated to [`format_filename`] and is byte-for-byte
/// identical to it. A multi-part format renders every token inside the same
/// bracket group, joined by [`MULTI_SUFFIX_SEPARATOR`]:
///
/// ```text
/// movie.mkv -> movie[blake3-ABCDEF+md5-123456].mkv
/// ```
///
/// Multi-algorithm groups are **always** prefixed with the algorithm name,
/// whatever [`SuffixFormat::prefix_enabled`] says on each part: a bare hex token
/// carries no algorithm identity, so a combined group of unprefixed tokens could
/// never be parsed back or verified. Single-algorithm output keeps honouring
/// [`SuffixFormat::prefix_enabled`] exactly as before.
///
/// Stem truncation, the UTF-16 limit and the empty/unparseable name checks behave
/// as in [`format_filename`], applied to the combined suffix.
pub fn format_filename_multi(
    name: &str,
    digests: &[(AlgorithmId, &str)],
    format: &MultiSuffixFormat,
    max_utf16_len: usize,
) -> Result<String, SuffixError> {
    if name.is_empty() {
        return Err(SuffixError::EmptyName);
    }
    format.validate()?;
    if let Some(only) = format.as_single() {
        let digest = digest_for(digests, only.algorithm)?;
        return format_filename(name, digest, &only, max_utf16_len);
    }
    let tokens = render_tokens(digests, format)?;
    render_group(name, &tokens, max_utf16_len)
}

/// Format `name` with a multi-algorithm suffix group, keeping at most
/// [`DEFAULT_MAX_UTF16_LEN`] UTF-16 code units.
///
/// Convenience wrapper over [`format_filename_multi`] using the platform default
/// filename limit.
pub fn format_filename_multi_default(
    name: &str,
    digests: &[(AlgorithmId, &str)],
    format: &MultiSuffixFormat,
) -> Result<String, SuffixError> {
    format_filename_multi(name, digests, format, DEFAULT_MAX_UTF16_LEN)
}

/// Split an already parsed suffix token into its individual parts.
///
/// A token without [`MULTI_SUFFIX_SEPARATOR`] yields exactly one part, so callers
/// that only know about single suffixes keep working unchanged. Returns an empty
/// vector when any part is not a valid suffix part.
pub fn split_suffix_token(token: &str) -> Vec<ParsedSuffix> {
    let parts: Vec<ParsedSuffix> = token
        .split(MULTI_SUFFIX_SEPARATOR)
        .filter_map(parse_suffix_token)
        .collect();
    if parts.len() != token.split(MULTI_SUFFIX_SEPARATOR).count() {
        return Vec::new();
    }
    parts
}

/// Look up the digest for `algorithm` in a caller-supplied list.
fn digest_for<'a>(
    digests: &'a [(AlgorithmId, &'a str)],
    algorithm: AlgorithmId,
) -> Result<&'a str, SuffixError> {
    match digests.iter().find(|(id, _)| *id == algorithm) {
        Some((_, digest)) => Ok(digest),
        // Reported as "too short" with zero actual characters: the caller has no
        // digest for this algorithm at all, which the planner maps to MissingHash.
        None => Err(SuffixError::DigestTooShort {
            required: algorithm.hex_len(),
            actual: 0,
        }),
    }
}

/// Render every token of a multi-algorithm group, always with its prefix.
fn render_tokens(
    digests: &[(AlgorithmId, &str)],
    format: &MultiSuffixFormat,
) -> Result<Vec<String>, SuffixError> {
    let mut tokens = Vec::with_capacity(format.len());
    for part in format.parts() {
        let digest = digest_for(digests, part.algorithm)?;
        tokens.push(render_prefixed_token(digest, part)?);
    }
    Ok(tokens)
}

/// Render one `<prefix>:<hex>` token for a multi-algorithm group.
///
/// The prefix is mandatory here: see [`format_filename_multi`] for why an
/// unprefixed token cannot survive a round trip inside a combined group.
fn render_prefixed_token(digest_hex: &str, part: &SuffixFormat) -> Result<String, SuffixError> {
    if !is_hex(digest_hex) {
        return Err(SuffixError::InvalidDigest);
    }
    let required = usize::from(part.length);
    if digest_hex.len() < required {
        return Err(SuffixError::DigestTooShort {
            required,
            actual: digest_hex.len(),
        });
    }
    let hex = &digest_hex[..required];
    let token = match part.case {
        HexCase::Upper => hex.to_ascii_uppercase(),
        HexCase::Lower => hex.to_ascii_lowercase(),
    };
    Ok(format!(
        "{}{SUFFIX_PREFIX_SEPARATOR}{token}",
        part.algorithm.spec().suffix_prefix
    ))
}

/// Wrap a rendered token list into `stem[token1+token2]extension`.
///
/// The name checks and the UTF-16 truncation mirror [`format_filename`] so a
/// multi-algorithm suffix never behaves differently from a single one at the
/// platform limit.
/// Build the longest stem that keeps the whole name within both limits.
///
/// The suffix and extension are fixed cost and are never truncated: a truncated
/// digest token would not parse back, and a truncated extension changes the
/// file type. Only the stem gives way, and it is cut on character boundaries so
/// no multi-byte character is split.
fn truncate_stem_for_limits(stem: &str, fixed: &str, max_utf16_len: usize) -> String {
    let fixed_utf16 = utf16_len(fixed);
    let fixed_bytes = fixed.len();
    let keep_units = max_utf16_len.saturating_sub(fixed_utf16);
    let keep_bytes = MAX_NAME_BYTES.saturating_sub(fixed_bytes);
    let mut truncated = String::new();
    let mut used_units = 0usize;
    let mut used_bytes = 0usize;
    for character in stem.chars() {
        let width = character.len_utf16();
        let bytes = character.len_utf8();
        if used_units + width > keep_units || used_bytes + bytes > keep_bytes {
            break;
        }
        truncated.push(character);
        used_units += width;
        used_bytes += bytes;
    }
    truncated
}

fn render_group(
    name: &str,
    tokens: &[String],
    max_utf16_len: usize,
) -> Result<String, SuffixError> {
    match parse_filename(name) {
        SuffixParseResult::Parsed(_) => return Err(SuffixError::AlreadySuffixed),
        SuffixParseResult::Unparseable => return Err(SuffixError::UnparseableName),
        SuffixParseResult::NotSuffixed => {}
    }
    let (stem, extension) = split_name(name);
    let suffix = format!("[{}]", tokens.join(&MULTI_SUFFIX_SEPARATOR.to_string()));
    let candidate = format!("{stem}{suffix}{extension}");
    let candidate_len = utf16_len(&candidate);
    if candidate_len <= max_utf16_len && candidate.len() <= MAX_NAME_BYTES {
        return Ok(candidate);
    }
    let fixed = format!("{suffix}{extension}");
    let truncated_stem = truncate_stem_for_limits(&stem, &fixed, max_utf16_len);
    if truncated_stem.is_empty() {
        return Err(SuffixError::NameTooLong {
            utf16_len: candidate_len,
            max_utf16_len,
        });
    }
    Ok(format!("{truncated_stem}{suffix}{extension}"))
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
/// The table is part of the product contract: up to 100 files use 6 hex
/// characters, up to 1,000 use 8, up to 100,000 use 10 and larger batches use
/// 12. The first entry matches [`SuffixFormat::default`] so the UI never shows a
/// recommended value that contradicts the selected one.
pub fn recommended_length(count: usize) -> u8 {
    if count <= 100 {
        6
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
    // Parsing still needs at least one character: an empty token is not a
    // suffix, and accepting it would make every `name[]` look parseable. The
    // zero-length *writing* case never produces a token to parse back.
    const MIN_PARSE_LENGTH: usize = 1;
    (MIN_PARSE_LENGTH..=max_parse_token_length()).contains(&value.len()) && is_hex(value)
}

/// A multi-algorithm group whose parts split cleanly yields one [`ParsedSuffix`]
/// carrying the parts, with [`ParsedSuffix::algorithm`] set only when every part
/// names the same algorithm (which a validated format never does).
fn parse_multi_suffix_token(content: &str) -> Option<ParsedSuffix> {
    let parts = split_suffix_token(content);
    if parts.len() < 2 {
        return None;
    }
    // A combined group is only attributable when every part names its algorithm;
    // an unprefixed part would silently drop algorithm identity.
    if parts.iter().any(|part| part.algorithm.is_none()) {
        return None;
    }
    let ambiguous = parts.iter().any(|part| part.ambiguous);
    Some(ParsedSuffix {
        token: content.to_string(),
        hex: parts
            .iter()
            .map(|part| part.hex.as_str())
            .collect::<Vec<&str>>()
            .join(""),
        algorithm: None,
        ambiguous,
        parts,
    })
}

fn parse_suffix_token(content: &str) -> Option<ParsedSuffix> {
    if content.contains(MULTI_SUFFIX_SEPARATOR) {
        return parse_multi_suffix_token(content);
    }
    // Both separators are accepted: the current one is what this release
    // writes, the legacy one is what earlier releases wrote and what already
    // exists in users' directories.
    let split = content
        .split_once(SUFFIX_PREFIX_SEPARATOR)
        .or_else(|| content.split_once(LEGACY_SUFFIX_PREFIX_SEPARATOR));
    if let Some((prefix, hex)) = split {
        let spec = spec_by_suffix_prefix(prefix)?;
        if !is_hex_token(hex) {
            return None;
        }
        return Some(ParsedSuffix {
            token: content.to_string(),
            hex: hex.to_ascii_lowercase(),
            algorithm: Some(spec.id),
            ambiguous: false,
            parts: Vec::new(),
        });
    }
    if !is_hex_token(content) {
        return None;
    }
    // Only suffix-capable algorithms can have produced this token, so infer
    // within that subset; otherwise shared widths report false ambiguity.
    let inferred = AlgorithmId::infer_suffix_by_hex_len(content.len());
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
        parts: Vec::new(),
    })
}

/// How an existing suffix on disk relates to the digests computed now.
///
/// The rename planner used to treat every already-suffixed name as a skip, so a
/// user could not tell "this file is already tagged correctly" from "this file
/// was tagged by a different tool, or changed since". These variants separate
/// those cases so the UI can ask the right question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistingSuffixVerdict {
    /// No bracket group before the extension.
    Absent,
    /// The suffix names an algorithm and its hex matches the current digest.
    Matches {
        /// Algorithm the suffix names.
        algorithm: AlgorithmId,
        /// Hex characters the suffix carries.
        hex: String,
    },
    /// The suffix names an algorithm, but its hex disagrees with the digest
    /// computed now. The file changed since it was tagged, or was tagged from
    /// different bytes.
    Mismatch {
        /// Algorithm the suffix names.
        algorithm: AlgorithmId,
        /// Hex characters found in the suffix.
        found: String,
        /// Hex characters the current digest starts with.
        current: String,
    },
    /// The suffix carries no algorithm prefix and its width maps to several
    /// algorithms, so which one produced it is undecidable from the name.
    ///
    /// [matched] lists the algorithms whose current digest does agree, which is
    /// the only evidence available without re-reading history.
    Ambiguous {
        /// Hex characters found in the suffix.
        hex: String,
        /// Algorithms whose current digest starts with [hex].
        matched: Vec<AlgorithmId>,
    },
    /// The suffix carries no algorithm prefix, but its width maps to exactly one
    /// algorithm, so the comparison is still decidable.
    Inferred {
        /// Algorithm the width implies.
        algorithm: AlgorithmId,
        /// Whether its current digest agrees with the suffix.
        agrees: bool,
    },
    /// A bracket group exists but is not a suffix this tool understands.
    Unparseable,
}

/// Read the current digest of one algorithm out of a digest collection.
///
/// Implemented for the two map types that appear in this crate so a caller can
/// pass whatever it already holds: the planner works with the `IndexMap` on a
/// file entry, while tests build plain `HashMap`s.
pub trait DigestLookup {
    /// Digest hex for [algorithm], or `None` when it was not computed.
    fn digest(&self, algorithm: AlgorithmId) -> Option<&str>;
}

impl DigestLookup for std::collections::HashMap<AlgorithmId, String> {
    fn digest(&self, algorithm: AlgorithmId) -> Option<&str> {
        self.get(&algorithm).map(String::as_str)
    }
}

impl DigestLookup for indexmap::IndexMap<AlgorithmId, String> {
    fn digest(&self, algorithm: AlgorithmId) -> Option<&str> {
        self.get(&algorithm).map(String::as_str)
    }
}

/// Compare the suffix already on [name] with the digests in [hashes].
///
/// [hashes] is any digest source: a `HashMap` from an ad-hoc computation, or the
/// `IndexMap` a [`FileEntry`] carries. The lookup is abstracted rather than
/// taking one concrete map type so neither caller has to copy its digests into
/// the other shape just to ask this question.
/// Judge a combined `a-xx+b-yy` suffix part by part.
///
/// Every part names its algorithm, so each is decidable on its own. The group
/// matches only when all parts agree; if any part disagrees the whole label is
/// stale, because the name as a whole claims a set of values that is no longer
/// true. A part whose algorithm was not computed makes the group unverifiable
/// rather than matching: silence is not agreement.
fn compare_multi_suffix<M: DigestLookup>(parsed: &ParsedSuffix, hashes: &M) -> ExistingSuffixVerdict {
    let mut checked = 0usize;
    for part in &parsed.parts {
        let Some(algorithm) = part.algorithm else {
            return ExistingSuffixVerdict::Unparseable;
        };
        let Some(current) = hashes.digest(algorithm) else {
            return ExistingSuffixVerdict::Unparseable;
        };
        let hex = part.hex.to_ascii_lowercase();
        let current_prefix: String = current
            .chars()
            .take(hex.chars().count())
            .collect::<String>()
            .to_ascii_lowercase();
        if current_prefix != hex {
            return ExistingSuffixVerdict::Mismatch {
                algorithm,
                found: hex,
                current: current_prefix,
            };
        }
        checked += 1;
    }
    if checked == 0 {
        return ExistingSuffixVerdict::Unparseable;
    }
    // Report the first part as the representative algorithm: the verdict type
    // names one, and for a fully matching group any of them is accurate. Every
    // part was already proven to carry an algorithm above, so the lookup cannot
    // fail here.
    let Some(first) = parsed.parts.first() else {
        return ExistingSuffixVerdict::Unparseable;
    };
    let Some(algorithm) = first.algorithm else {
        return ExistingSuffixVerdict::Unparseable;
    };
    ExistingSuffixVerdict::Matches {
        algorithm,
        hex: first.hex.to_ascii_lowercase(),
    }
}

pub fn compare_existing_suffix<M: DigestLookup>(name: &str, hashes: &M) -> ExistingSuffixVerdict {
    let parsed = match parse_filename(name) {
        SuffixParseResult::NotSuffixed => return ExistingSuffixVerdict::Absent,
        SuffixParseResult::Unparseable => return ExistingSuffixVerdict::Unparseable,
        SuffixParseResult::Parsed(parsed) => parsed,
    };
    // A combined group carries one token per algorithm, so comparing the
    // concatenated hex would always fail. Each part is judged on its own and
    // the group is only "matching" when every part that can be checked agrees.
    if parsed.is_multi() {
        return compare_multi_suffix(&parsed, hashes);
    }
    let hex = parsed.hex.to_ascii_lowercase();
    // An explicit prefix is authoritative: the suffix itself says which
    // algorithm produced it, so no inference is needed or wanted.
    if let Some(algorithm) = parsed.algorithm {
        let Some(current) = hashes.digest(algorithm) else {
            // The caller did not compute this algorithm, so nothing can be
            // claimed about the suffix.
            return ExistingSuffixVerdict::Unparseable;
        };
        let current_prefix: String = current
            .chars()
            .take(hex.chars().count())
            .collect::<String>()
            .to_ascii_lowercase();
        if current_prefix == hex {
            return ExistingSuffixVerdict::Matches { algorithm, hex };
        }
        return ExistingSuffixVerdict::Mismatch {
            algorithm,
            found: hex,
            current: current_prefix,
        };
    }
    // No prefix. Widths that map to one algorithm stay decidable; wider ones
    // are reported with every algorithm that agrees.
    let width = hex.chars().count();
    let mut candidates = AlgorithmId::infer_suffix_by_hex_len(width);
    if candidates.is_empty() {
        // A truncated token usually matches no full digest width — the default
        // suffix is six characters, and no algorithm hashes to six. Treating
        // that as "no candidates" made every default-length bare suffix
        // unverifiable. Any algorithm whose digest is at least this wide could
        // have produced it, so those are the candidates.
        candidates = AlgorithmId::suffix_capable()
            .filter(|spec| spec.hex_len >= width)
            .map(|spec| spec.id)
            .collect();
    }
    if candidates.len() == 1 {
        let algorithm = candidates[0];
        let agrees = hashes.digest(algorithm).is_some_and(|current| {
            current
                .chars()
                .take(hex.chars().count())
                .collect::<String>()
                .to_ascii_lowercase()
                == hex
        });
        return ExistingSuffixVerdict::Inferred { algorithm, agrees };
    }
    let matched: Vec<AlgorithmId> = candidates
        .into_iter()
        .filter(|algorithm| {
            hashes.digest(*algorithm).is_some_and(|current| {
                current
                    .chars()
                    .take(hex.chars().count())
                    .collect::<String>()
                    .to_ascii_lowercase()
                    == hex
            })
        })
        .collect();
    ExistingSuffixVerdict::Ambiguous { hex, matched }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn hash_map(entries: &[(AlgorithmId, &str)]) -> std::collections::HashMap<AlgorithmId, String> {
        entries
            .iter()
            .map(|(algorithm, digest)| (*algorithm, (*digest).to_string()))
            .collect()
    }

    #[test]
    fn a_legacy_colon_suffix_still_parses() {
        // Names written by earlier releases use `:`. They exist on disk, so a
        // parse failure here would make every one of them look untagged and be
        // renamed a second time.
        match parse_filename("movie[blake3:ABCDEF].mkv") {
            SuffixParseResult::Parsed(parsed) => {
                assert_eq!(parsed.algorithm, Some(AlgorithmId::Blake3));
                assert_eq!(parsed.hex, "abcdef");
            }
            other => panic!("legacy suffix must parse, got {other:?}"),
        }
    }

    #[test]
    fn the_current_separator_is_a_dash() {
        // `:` is illegal on NTFS, exFAT, FAT32 and most SMB shares, and macOS
        // renders it as `/` in Finder.
        let format = SuffixFormat {
            length: 6,
            case: HexCase::Upper,
            algorithm: AlgorithmId::Blake3,
            prefix_enabled: true,
        };
        let formatted = format_filename("movie.mkv", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
            .expect("format");
        assert_eq!(formatted, "movie[blake3-ABCDEF].mkv");
        assert!(!formatted.contains(':'), "no colon may reach a filename");
    }

    #[test]
    fn a_multi_algorithm_suffix_is_compared_part_by_part() {
        // Comparing the concatenated hex of `[blake3-ab+md5-cd]` never matches,
        // which made every combined suffix look unverifiable.
        let blake3 = "abcdef0123456789".repeat(4);
        let md5 = "0123456789abcdef0123456789abcdef".to_string();
        let hashes = hash_map(&[
            (AlgorithmId::Blake3, blake3.as_str()),
            (AlgorithmId::Md5, md5.as_str()),
        ]);
        assert_eq!(
            compare_existing_suffix("movie[blake3-abcdef+md5-012345].mkv", &hashes),
            ExistingSuffixVerdict::Matches {
                algorithm: AlgorithmId::Blake3,
                hex: "abcdef".to_string(),
            }
        );
    }

    #[test]
    fn a_multi_algorithm_suffix_with_one_bad_part_is_a_mismatch() {
        let blake3 = "abcdef0123456789".repeat(4);
        let md5 = "0123456789abcdef0123456789abcdef".to_string();
        let hashes = hash_map(&[
            (AlgorithmId::Blake3, blake3.as_str()),
            (AlgorithmId::Md5, md5.as_str()),
        ]);
        // blake3 agrees, md5 does not: the group as a whole is stale.
        match compare_existing_suffix("movie[blake3-abcdef+md5-ffffff].mkv", &hashes) {
            ExistingSuffixVerdict::Mismatch { algorithm, .. } => {
                assert_eq!(algorithm, AlgorithmId::Md5);
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    #[test]
    fn a_six_character_bare_suffix_is_still_checkable() {
        // Six is the default suffix length and matches no digest width, so the
        // width-based candidate list was empty and every default-length suffix
        // came back unverifiable.
        let blake3 = "abcdef0123456789".repeat(4);
        let hashes = hash_map(&[(AlgorithmId::Blake3, blake3.as_str())]);
        match compare_existing_suffix("movie[ABCDEF].mkv", &hashes) {
            ExistingSuffixVerdict::Ambiguous { matched, .. } => {
                assert!(
                    matched.contains(&AlgorithmId::Blake3),
                    "blake3 agrees with the token and must be reported: {matched:?}"
                );
            }
            other => panic!("expected ambiguity with a match, got {other:?}"),
        }
    }

    #[test]
    fn a_stem_is_truncated_to_the_byte_limit_without_splitting_characters() {
        // 100 CJK characters are 300 bytes: they fit the UTF-16 limit but not
        // the 255-byte limit Linux and macOS enforce.
        let stem = "中".repeat(100);
        let format = SuffixFormat {
            length: 6,
            case: HexCase::Upper,
            algorithm: AlgorithmId::Blake3,
            prefix_enabled: false,
        };
        let formatted = format_filename(
            &format!("{stem}.mkv"),
            &digest(),
            &format,
            DEFAULT_MAX_UTF16_LEN,
        )
        .expect("format");
        assert!(
            formatted.len() <= MAX_NAME_BYTES,
            "name is {} bytes, over the {MAX_NAME_BYTES} limit",
            formatted.len()
        );
        // The suffix and extension survive intact; only the stem gave way.
        assert!(formatted.ends_with("[ABCDEF].mkv"));
        assert!(formatted.starts_with("中"), "no character was split");
    }

    #[test]
    fn compare_reports_absent_when_there_is_no_suffix() {
        let hashes = hash_map(&[(AlgorithmId::Sha256, "abcd1234")]);
        assert_eq!(
            compare_existing_suffix("movie.mkv", &hashes),
            ExistingSuffixVerdict::Absent
        );
    }

    #[test]
    fn compare_accepts_a_prefixed_suffix_that_matches() {
        let hashes = hash_map(&[(AlgorithmId::Sha256, "abcd1234ffff")]);
        assert_eq!(
            compare_existing_suffix("movie[sha256:ABCD1234].mkv", &hashes),
            ExistingSuffixVerdict::Matches {
                algorithm: AlgorithmId::Sha256,
                hex: "abcd1234".to_string(),
            }
        );
    }

    #[test]
    fn compare_flags_a_prefixed_suffix_that_disagrees() {
        // The suffix says sha256 but the file now hashes to something else:
        // exactly the "content changed since it was tagged" case.
        let hashes = hash_map(&[(AlgorithmId::Sha256, "99998888ffff")]);
        assert_eq!(
            compare_existing_suffix("movie[sha256:ABCD1234].mkv", &hashes),
            ExistingSuffixVerdict::Mismatch {
                algorithm: AlgorithmId::Sha256,
                found: "abcd1234".to_string(),
                current: "99998888".to_string(),
            }
        );
    }

    #[test]
    fn compare_infers_when_the_width_names_one_algorithm() {
        // A 4-character token matches no registered digest width, so it cannot
        // be attributed to an algorithm at all; the verdict must not invent one.
        let hashes = hash_map(&[(AlgorithmId::Md5, "abcd1234")]);
        match compare_existing_suffix("movie[ABCD].mkv", &hashes) {
            ExistingSuffixVerdict::Ambiguous { hex, .. } => {
                assert_eq!(hex, "abcd");
            }
            ExistingSuffixVerdict::Inferred { agrees, .. } => {
                // Either shape is acceptable as long as the prefix agrees.
                assert!(agrees);
            }
            other => panic!("unexpected verdict: {other:?}"),
        }
    }

    #[test]
    fn compare_lists_every_algorithm_that_agrees_with_a_bare_token() {
        // Xxh3_64 and Xxh64 both print 16 hex characters, so a bare token of
        // that width belongs to neither by name alone. Both digests start with
        // the same 16 characters here, so both must be reported as matching.
        let shared = "abcd1234abcd1234";
        let hashes = hash_map(&[
            (AlgorithmId::Xxh3_64, "abcd1234abcd1234"),
            (AlgorithmId::Xxh64, "abcd1234abcd1234"),
        ]);
        let verdict = compare_existing_suffix(&format!("movie[{shared}].mkv"), &hashes);
        match verdict {
            ExistingSuffixVerdict::Ambiguous { matched, .. } => {
                assert!(matched.contains(&AlgorithmId::Xxh3_64));
                assert!(matched.contains(&AlgorithmId::Xxh64));
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn compare_never_claims_a_match_for_an_algorithm_it_was_not_given() {
        // No digests at all: the suffix cannot be verified, and inventing a
        // verdict would be worse than admitting the gap.
        let hashes = hash_map(&[]);
        let verdict = compare_existing_suffix("movie[sha256:ABCD1234].mkv", &hashes);
        assert_eq!(verdict, ExistingSuffixVerdict::Unparseable);
    }
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

        let multi_dot =
            format_filename("archive.tar.gz", &digest(), &format, DEFAULT_MAX_UTF16_LEN)
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
        assert_eq!(formatted, "movie[blake3-ABCDE].mkv");
    }

    #[test]
    fn format_rejects_already_suffixed_or_unparseable_names() {
        let format = upper_format(5);
        assert_eq!(
            format_filename(
                "movie[ABCDE].mkv",
                &digest(),
                &format,
                DEFAULT_MAX_UTF16_LEN
            ),
            Err(SuffixError::AlreadySuffixed)
        );
        assert_eq!(
            format_filename(
                "movie[blake3:ABCDE].mkv",
                &digest(),
                &format,
                DEFAULT_MAX_UTF16_LEN
            ),
            Err(SuffixError::AlreadySuffixed)
        );
        assert_eq!(
            format_filename(
                "movie[1080p].mkv",
                &digest(),
                &format,
                DEFAULT_MAX_UTF16_LEN
            ),
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
    fn multi_suffix_renders_prefixed_tokens_in_one_group() {
        let format = MultiSuffixFormat::new(vec![
            SuffixFormat {
                length: 6,
                case: HexCase::Upper,
                algorithm: AlgorithmId::Blake3,
                prefix_enabled: false,
            },
            SuffixFormat {
                length: 6,
                case: HexCase::Upper,
                algorithm: AlgorithmId::Md5,
                prefix_enabled: false,
            },
        ])
        .expect("two parts");
        let blake3 = digest();
        let md5 = "0123456789abcdef0123456789abcdef";
        let digests = [
            (AlgorithmId::Blake3, blake3.as_str()),
            (AlgorithmId::Md5, md5),
        ];
        let formatted =
            format_filename_multi("movie.mkv", &digests, &format, DEFAULT_MAX_UTF16_LEN)
                .expect("multi formatting must succeed");
        // Multi-algorithm groups always carry the algorithm name, even though
        // neither part asked for a prefix.
        assert_eq!(formatted, "movie[blake3-ABCDEF+md5-012345].mkv");
    }

    #[test]
    fn multi_suffix_round_trips_through_the_parser() {
        let format = MultiSuffixFormat::new(vec![
            SuffixFormat {
                length: 4,
                case: HexCase::Lower,
                algorithm: AlgorithmId::Blake3,
                prefix_enabled: false,
            },
            SuffixFormat {
                length: 4,
                case: HexCase::Lower,
                algorithm: AlgorithmId::Sha256,
                prefix_enabled: false,
            },
        ])
        .expect("two parts");
        let blake3 = digest();
        let sha256 = "f".repeat(64);
        let digests = [
            (AlgorithmId::Blake3, blake3.as_str()),
            (AlgorithmId::Sha256, sha256.as_str()),
        ];
        let formatted =
            format_filename_multi("movie.mkv", &digests, &format, DEFAULT_MAX_UTF16_LEN)
                .expect("multi formatting must succeed");
        assert_eq!(formatted, "movie[blake3-abcd+sha256-ffff].mkv");
        match parse_filename(&formatted) {
            SuffixParseResult::Parsed(parsed) => {
                assert!(parsed.is_multi());
                assert_eq!(parsed.parts.len(), 2);
                assert_eq!(parsed.parts[0].algorithm, Some(AlgorithmId::Blake3));
                assert_eq!(parsed.parts[0].hex, "abcd");
                assert_eq!(parsed.parts[1].algorithm, Some(AlgorithmId::Sha256));
                assert_eq!(
                    parsed.algorithms(),
                    vec![AlgorithmId::Blake3, AlgorithmId::Sha256]
                );
            }
            other => panic!("expected a parsed multi suffix, got {other:?}"),
        }
    }

    #[test]
    fn single_part_multi_format_is_byte_identical_to_the_legacy_formatter() {
        let single = upper_format(6);
        let multi = MultiSuffixFormat::single(single);
        let digests = [(AlgorithmId::Blake3, digest())];
        let digests: Vec<(AlgorithmId, &str)> = digests
            .iter()
            .map(|(id, hex)| (*id, hex.as_str()))
            .collect();
        let legacy = format_filename("movie.mkv", &digest(), &single, DEFAULT_MAX_UTF16_LEN)
            .expect("legacy format");
        let formatted = format_filename_multi("movie.mkv", &digests, &multi, DEFAULT_MAX_UTF16_LEN)
            .expect("multi format");
        assert_eq!(legacy, formatted);
        assert_eq!(legacy, "movie[ABCDEF].mkv");
    }

    #[test]
    fn unprefixed_combined_group_is_unparseable() {
        // A hand-made name with bare hex parts must not be mistaken for a
        // verified multi-algorithm suffix: algorithm identity is missing.
        assert_eq!(
            parse_filename("movie[ABCDEF+012345].mkv"),
            SuffixParseResult::Unparseable
        );
    }

    #[test]
    fn multi_format_rejects_empty_duplicate_and_oversized_part_lists() {
        assert_eq!(
            MultiSuffixFormat::new(Vec::new()),
            Err(SuffixError::EmptySuffixFormat)
        );
        assert_eq!(
            MultiSuffixFormat::new(vec![
                SuffixFormat {
                    algorithm: AlgorithmId::Blake3,
                    ..upper_format(6)
                },
                SuffixFormat {
                    algorithm: AlgorithmId::Blake3,
                    ..upper_format(6)
                },
            ])
            .expect("two parts")
            .validate(),
            Err(SuffixError::DuplicateSuffixAlgorithm(AlgorithmId::Blake3))
        );
        let many: Vec<SuffixFormat> = AlgorithmId::suffix_capable()
            .take(MAX_SUFFIX_PARTS + 1)
            .map(|spec| SuffixFormat {
                algorithm: spec.id,
                ..upper_format(6)
            })
            .collect();
        assert_eq!(many.len(), MAX_SUFFIX_PARTS + 1);
        assert!(matches!(
            MultiSuffixFormat::new(many),
            Err(SuffixError::TooManySuffixParts { .. })
        ));
    }

    #[test]
    fn multi_format_deserializes_legacy_and_explicit_shapes() {
        // The legacy single-object document is what stored plans contain.
        let legacy: MultiSuffixFormat = serde_json::from_str(
            r#"{"length":6,"case":"upper","algorithm":"blake3","prefix_enabled":false}"#,
        )
        .expect("legacy shape must parse");
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy.algorithms(), vec![AlgorithmId::Blake3]);
        let only = legacy.as_single().expect("single part");
        assert_eq!(only.length, 6);

        let explicit: MultiSuffixFormat = serde_json::from_str(
            r#"{"parts":[
                {"length":6,"case":"upper","algorithm":"blake3","prefix_enabled":true},
                {"length":4,"case":"lower","algorithm":"md5","prefix_enabled":true}
            ]}"#,
        )
        .expect("explicit shape must parse");
        assert!(explicit.is_multi());
        assert_eq!(explicit.len(), 2);
        assert_eq!(
            explicit.algorithms(),
            vec![AlgorithmId::Blake3, AlgorithmId::Md5]
        );
    }

    #[test]
    fn default_length_is_six() {
        assert_eq!(SuffixFormat::default().length, 6);
        assert_eq!(recommended_length(1), 6);
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
    fn validate_accepts_zero_and_rejects_beyond_the_digest_width() {
        // Zero is a legitimate suffix: it writes only the separator, so a
        // batch can be tagged without embedding any digest.
        assert!(upper_format(0).validate().is_ok());
        // The ceiling is the algorithm's own digest width. upper_format uses
        // BLAKE3, whose digest is 64 hex characters.
        let blake3_width =
            u8::try_from(default_suffix_algorithm().spec().hex_len).expect("digest width fits u8");
        assert!(upper_format(blake3_width).validate().is_ok());
        for length in [blake3_width + 1, 255] {
            assert_eq!(
                upper_format(length).validate(),
                Err(SuffixError::InvalidLength(length)),
                "length {length}"
            );
        }
        // A shorter algorithm must reject a length that only BLAKE3 could fill.
        let md5 = SuffixFormat {
            algorithm: AlgorithmId::Md5,
            ..upper_format(0)
        };
        assert!(
            md5.clone().validate().is_ok(),
            "zero length is valid for every algorithm"
        );
        let md5_too_long = SuffixFormat {
            length: u8::try_from(AlgorithmId::Md5.spec().hex_len).expect("width") + 1,
            ..md5
        };
        assert!(matches!(
            md5_too_long.validate(),
            Err(SuffixError::InvalidLength(_))
        ));
        // Only suffix-capable algorithms may be written into a filename; the
        // rest must be rejected so a filename never carries an unidentifiable
        // digest (e.g. a variable-length or non-hex-safe encoding).
        for algorithm in AlgorithmId::ALL {
            let format = SuffixFormat {
                algorithm,
                ..upper_format(8)
            };
            if algorithm.spec().suffix_usable {
                assert!(format.validate().is_ok(), "algorithm {algorithm}");
            } else {
                assert_eq!(
                    format.validate(),
                    Err(SuffixError::AlgorithmNotSuffixUsable(algorithm)),
                    "{algorithm} must not be usable in a filename suffix"
                );
            }
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
        assert_eq!(recommended_length(0), 6);
        assert_eq!(recommended_length(1), 6);
        assert_eq!(recommended_length(100), 6);
        assert_eq!(recommended_length(101), 8);
        assert_eq!(recommended_length(1_000), 8);
        assert_eq!(recommended_length(1_001), 10);
        assert_eq!(recommended_length(100_000), 10);
        assert_eq!(recommended_length(100_001), 12);
    }
}
