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

/// Separator placed between the tokens of a multi-algorithm suffix group.
///
/// It is deliberately a character that can never appear in a hex token or in an
/// algorithm prefix, so a combined group stays unambiguous when parsed back.
pub const MULTI_SUFFIX_SEPARATOR: char = '+';

/// Maximum number of algorithms that may be combined into one filename suffix.
///
/// The cap exists so a filename cannot be inflated without bound: sixteen tokens
/// of up to 16 hex characters plus separators still fit every platform limit
/// once the stem is truncated.
pub const MAX_SUFFIX_PARTS: usize = 16;

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

/// One or more algorithm tokens written into a single filename suffix group.
///
/// A one-part format reproduces the historical behaviour byte for byte
/// (`movie[ABCDEF].mkv`), and a multi-part format joins the tokens with
/// [`MULTI_SUFFIX_SEPARATOR`] inside one bracket group
/// (`movie[blake3:ABCDEF+md5:123456].mkv`). Keeping the tokens in one group is
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

/// Format `name` with a multi-algorithm suffix group, preserving the extension.
///
/// A one-part format is delegated to [`format_filename`] and is byte-for-byte
/// identical to it. A multi-part format renders every token inside the same
/// bracket group, joined by [`MULTI_SUFFIX_SEPARATOR`]:
///
/// ```text
/// movie.mkv -> movie[blake3:ABCDEF+md5:123456].mkv
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
    Ok(format!("{}:{token}", part.algorithm.spec().suffix_prefix))
}

/// Wrap a rendered token list into `stem[token1+token2]extension`.
///
/// The name checks and the UTF-16 truncation mirror [`format_filename`] so a
/// multi-algorithm suffix never behaves differently from a single one at the
/// platform limit.
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
    if candidate_len <= max_utf16_len {
        return Ok(candidate);
    }
    let fixed_len = utf16_len(&suffix) + utf16_len(extension);
    let keep_units = max_utf16_len.saturating_sub(fixed_len);
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
    let min = usize::from(MIN_SUFFIX_LENGTH);
    (min..=max_parse_token_length()).contains(&value.len()) && is_hex(value)
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
        assert_eq!(formatted, "movie[blake3:ABCDE].mkv");
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
        assert_eq!(formatted, "movie[blake3:ABCDEF+md5:012345].mkv");
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
        assert_eq!(formatted, "movie[blake3:abcd+sha256:ffff].mkv");
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
