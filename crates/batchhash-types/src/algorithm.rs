//! Algorithm registry: the single source of truth for BatchHash hash algorithms.
//!
//! Every algorithm the product exposes is registered exactly once here. Stable IDs
//! are part of the report schema v1 contract: they are JSON hash keys, CSV column
//! headers, `suffix_algorithm` values and diff `algorithms_compared` entries.
//!
//! Registry order ([`AlgorithmId::ALL`]) is the canonical order used by report
//! serialization and CSV column layout, so it must never be reordered without a
//! schema version bump.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Stable identity of a hash algorithm (report schema v1 contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlgorithmId {
    #[serde(rename = "md5")]
    Md5,
    #[serde(rename = "sha1")]
    Sha1,
    #[serde(rename = "sha224")]
    Sha224,
    #[serde(rename = "sha256")]
    Sha256,
    #[serde(rename = "sha384")]
    Sha384,
    #[serde(rename = "sha512")]
    Sha512,
    #[serde(rename = "sha3-256")]
    Sha3_256,
    #[serde(rename = "sha3-512")]
    Sha3_512,
    #[serde(rename = "blake2b")]
    Blake2b,
    #[serde(rename = "blake2s")]
    Blake2s,
    #[serde(rename = "blake3")]
    Blake3,
    #[serde(rename = "xxh3-64")]
    Xxh3_64,
    #[serde(rename = "xxh3-128")]
    Xxh3_128,
    #[serde(rename = "xxh64")]
    Xxh64,
    #[serde(rename = "crc32")]
    Crc32,
    #[serde(rename = "crc32c")]
    Crc32c,
}

/// Static metadata for one registered algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlgorithmSpec {
    /// Stable report identity.
    pub id: AlgorithmId,
    /// Stable ID string, identical to the serde representation.
    pub stable_id: &'static str,
    /// Human readable display name used by the UI and reports.
    pub display_name: &'static str,
    /// Digest length in lowercase hex characters.
    pub hex_len: usize,
    /// Digest length in bytes.
    pub digest_len: usize,
    /// Whether the algorithm is considered cryptographically secure.
    pub cryptographic: bool,
    /// Whether the digest may be written into a filename suffix.
    pub suffix_usable: bool,
    /// Warning shown when the algorithm is selected for a filename suffix.
    pub suffix_warning: Option<&'static str>,
    /// Optional human readable prefix used to disambiguate suffixes (`prefix:hex`).
    pub suffix_prefix: &'static str,
    /// Whether a bare hex suffix of this length maps to more than one algorithm.
    pub ambiguous_by_length: bool,
    /// Marked as the default algorithm for writing filename suffixes.
    pub default_suffix_algorithm: bool,
}

impl AlgorithmId {
    /// Canonical registry order. Do not reorder: report and CSV serialization
    /// depend on it (schema v1).
    pub const ALL: [AlgorithmId; 16] = [
        AlgorithmId::Md5,
        AlgorithmId::Sha1,
        AlgorithmId::Sha224,
        AlgorithmId::Sha256,
        AlgorithmId::Sha384,
        AlgorithmId::Sha512,
        AlgorithmId::Sha3_256,
        AlgorithmId::Sha3_512,
        AlgorithmId::Blake2b,
        AlgorithmId::Blake2s,
        AlgorithmId::Blake3,
        AlgorithmId::Xxh3_64,
        AlgorithmId::Xxh3_128,
        AlgorithmId::Xxh64,
        AlgorithmId::Crc32,
        AlgorithmId::Crc32c,
    ];

    /// Stable ID string (e.g. `"sha3-256"`).
    pub fn stable_id(self) -> &'static str {
        self.spec().stable_id
    }

    /// Static metadata for this algorithm.
    pub fn spec(self) -> &'static AlgorithmSpec {
        REGISTRY
            .iter()
            .find(|spec| spec.id == self)
            .expect("every AlgorithmId variant must be registered")
    }

    /// Hex digest length.
    pub fn hex_len(self) -> usize {
        self.spec().hex_len
    }

    /// Parse a stable algorithm ID.
    pub fn from_stable_id(value: &str) -> Option<AlgorithmId> {
        let value = value.trim();
        REGISTRY
            .iter()
            .find(|spec| spec.stable_id == value)
            .map(|spec| spec.id)
    }

    /// All algorithms whose hex digest length equals `hex_len`.
    ///
    /// More than one result means a bare suffix/report column of that length is
    /// ambiguous and the caller must ask the user or require a prefix.
    pub fn infer_by_hex_len(hex_len: usize) -> Vec<AlgorithmId> {
        REGISTRY
            .iter()
            .filter(|spec| spec.hex_len == hex_len)
            .map(|spec| spec.id)
            .collect()
    }

    /// All algorithms registered as suffix-capable, in canonical order.
    pub fn suffix_capable() -> impl Iterator<Item = &'static AlgorithmSpec> {
        REGISTRY.iter().filter(|spec| spec.suffix_usable)
    }
}

impl fmt::Display for AlgorithmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.stable_id())
    }
}

impl FromStr for AlgorithmId {
    type Err = AlgorithmParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AlgorithmId::from_stable_id(s).ok_or_else(|| AlgorithmParseError(s.to_string()))
    }
}

/// Returned when a string is not a registered stable algorithm ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown algorithm id: {0}")]
pub struct AlgorithmParseError(pub String);

/// The algorithm registry, in canonical order.
pub static REGISTRY: [AlgorithmSpec; 16] = [
    AlgorithmSpec {
        id: AlgorithmId::Md5,
        stable_id: "md5",
        display_name: "MD5",
        hex_len: 32,
        digest_len: 16,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("MD5 is cryptographically broken; a truncated suffix is not a security digest."),
        suffix_prefix: "md5",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha1,
        stable_id: "sha1",
        display_name: "SHA-1",
        hex_len: 40,
        digest_len: 20,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("SHA-1 is cryptographically broken; a truncated suffix is not a security digest."),
        suffix_prefix: "sha1",
        ambiguous_by_length: false,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha224,
        stable_id: "sha224",
        display_name: "SHA-224",
        hex_len: 56,
        digest_len: 28,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "sha224",
        ambiguous_by_length: false,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha256,
        stable_id: "sha256",
        display_name: "SHA-256",
        hex_len: 64,
        digest_len: 32,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "sha256",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha384,
        stable_id: "sha384",
        display_name: "SHA-384",
        hex_len: 96,
        digest_len: 48,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "sha384",
        ambiguous_by_length: false,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha512,
        stable_id: "sha512",
        display_name: "SHA-512",
        hex_len: 128,
        digest_len: 64,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "sha512",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha3_256,
        stable_id: "sha3-256",
        display_name: "SHA3-256",
        hex_len: 64,
        digest_len: 32,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "sha3_256",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Sha3_512,
        stable_id: "sha3-512",
        display_name: "SHA3-512",
        hex_len: 128,
        digest_len: 64,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "sha3_512",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Blake2b,
        stable_id: "blake2b",
        display_name: "BLAKE2b-512",
        hex_len: 128,
        digest_len: 64,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "blake2b",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Blake2s,
        stable_id: "blake2s",
        display_name: "BLAKE2s-256",
        hex_len: 64,
        digest_len: 32,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "blake2s",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Blake3,
        stable_id: "blake3",
        display_name: "BLAKE3",
        hex_len: 64,
        digest_len: 32,
        cryptographic: true,
        suffix_usable: true,
        suffix_warning: None,
        suffix_prefix: "blake3",
        ambiguous_by_length: true,
        default_suffix_algorithm: true,
    },
    AlgorithmSpec {
        id: AlgorithmId::Xxh3_64,
        stable_id: "xxh3-64",
        display_name: "XXH3-64",
        hex_len: 16,
        digest_len: 8,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("XXH3 is non-cryptographic; a suffix is a convenience token, not a security digest."),
        suffix_prefix: "xxh3_64",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Xxh3_128,
        stable_id: "xxh3-128",
        display_name: "XXH3-128",
        hex_len: 32,
        digest_len: 16,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("XXH3 is non-cryptographic; a suffix is a convenience token, not a security digest."),
        suffix_prefix: "xxh3_128",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Xxh64,
        stable_id: "xxh64",
        display_name: "XXH64",
        hex_len: 16,
        digest_len: 8,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("XXH64 is non-cryptographic; a suffix is a convenience token, not a security digest."),
        suffix_prefix: "xxh64",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Crc32,
        stable_id: "crc32",
        display_name: "CRC32",
        hex_len: 8,
        digest_len: 4,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("CRC32 is an error-detection checksum, not a content digest; a suffix is convenience only."),
        suffix_prefix: "crc32",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
    AlgorithmSpec {
        id: AlgorithmId::Crc32c,
        stable_id: "crc32c",
        display_name: "CRC32C",
        hex_len: 8,
        digest_len: 4,
        cryptographic: false,
        suffix_usable: true,
        suffix_warning: Some("CRC32C is an error-detection checksum, not a content digest; a suffix is convenience only."),
        suffix_prefix: "crc32c",
        ambiguous_by_length: true,
        default_suffix_algorithm: false,
    },
];

/// Look up a spec by its stable ID string.
pub fn spec_by_stable_id(stable_id: &str) -> Option<&'static AlgorithmSpec> {
    REGISTRY.iter().find(|spec| spec.stable_id == stable_id)
}

/// Look up a spec by its optional suffix prefix (case-insensitive).
pub fn spec_by_suffix_prefix(prefix: &str) -> Option<&'static AlgorithmSpec> {
    REGISTRY
        .iter()
        .find(|spec| spec.suffix_prefix.eq_ignore_ascii_case(prefix))
}

/// The default algorithm used when the user writes filename suffixes.
pub fn default_suffix_algorithm() -> AlgorithmId {
    REGISTRY
        .iter()
        .find(|spec| spec.default_suffix_algorithm)
        .map(|spec| spec.id)
        .unwrap_or(AlgorithmId::Blake3)
}

/// Validate that a string is a lowercase-or-uppercase hex digest of `hex_len` chars.
pub fn is_hex_digest_of_len(value: &str, hex_len: usize) -> bool {
    value.len() == hex_len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Validate an arbitrary hex string (any even length, ASCII hex only).
pub fn is_hex(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Normalize a hex digest to lowercase, returning `None` for non-hex input.
pub fn normalize_hex(value: &str) -> Option<String> {
    if is_hex(value) {
        Some(value.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_all_sixteen_algorithms() {
        assert_eq!(REGISTRY.len(), 16);
        assert_eq!(AlgorithmId::ALL.len(), 16);
        let mut ids: Vec<&str> = REGISTRY.iter().map(|spec| spec.stable_id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 16, "stable IDs must be unique");
    }

    #[test]
    fn registry_order_matches_all_array() {
        let from_registry: Vec<AlgorithmId> = REGISTRY.iter().map(|spec| spec.id).collect();
        assert_eq!(from_registry, AlgorithmId::ALL.to_vec());
    }

    #[test]
    fn stable_ids_round_trip_through_serde() {
        for id in AlgorithmId::ALL {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.stable_id()));
            let back: AlgorithmId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, id);
            assert_eq!(id.to_string(), id.stable_id());
            assert_eq!(id.stable_id().parse::<AlgorithmId>().unwrap(), id);
        }
    }

    #[test]
    fn digest_lengths_are_consistent() {
        for spec in &REGISTRY {
            assert_eq!(
                spec.hex_len,
                spec.digest_len * 2,
                "{} length",
                spec.stable_id
            );
        }
        assert_eq!(AlgorithmId::Md5.hex_len(), 32);
        assert_eq!(AlgorithmId::Sha1.hex_len(), 40);
        assert_eq!(AlgorithmId::Sha224.hex_len(), 56);
        assert_eq!(AlgorithmId::Sha256.hex_len(), 64);
        assert_eq!(AlgorithmId::Sha384.hex_len(), 96);
        assert_eq!(AlgorithmId::Sha512.hex_len(), 128);
        assert_eq!(AlgorithmId::Blake3.hex_len(), 64);
        assert_eq!(AlgorithmId::Xxh3_64.hex_len(), 16);
        assert_eq!(AlgorithmId::Crc32.hex_len(), 8);
    }

    #[test]
    fn cryptographic_flags_are_as_documented() {
        for id in AlgorithmId::ALL {
            let expected = !matches!(
                id,
                AlgorithmId::Md5
                    | AlgorithmId::Sha1
                    | AlgorithmId::Xxh3_64
                    | AlgorithmId::Xxh3_128
                    | AlgorithmId::Xxh64
                    | AlgorithmId::Crc32
                    | AlgorithmId::Crc32c
            );
            assert_eq!(id.spec().cryptographic, expected, "{id}");
            assert!(id.spec().suffix_usable);
            if !expected {
                assert!(id.spec().suffix_warning.is_some(), "{id} needs warning");
            }
        }
    }

    #[test]
    fn suffix_prefixes_are_unique_and_registered() {
        for spec in &REGISTRY {
            assert!(spec.suffix_usable);
            assert!(!spec.suffix_prefix.is_empty());
            assert_eq!(
                spec_by_suffix_prefix(spec.suffix_prefix).map(|s| s.id),
                Some(spec.id)
            );
            assert_eq!(AlgorithmId::from_stable_id(spec.stable_id), Some(spec.id));
        }
    }

    #[test]
    fn length_inference_reports_ambiguity() {
        assert_eq!(AlgorithmId::infer_by_hex_len(40), vec![AlgorithmId::Sha1]);
        assert_eq!(
            AlgorithmId::infer_by_hex_len(32),
            vec![AlgorithmId::Md5, AlgorithmId::Xxh3_128]
        );
        assert_eq!(
            AlgorithmId::infer_by_hex_len(8),
            vec![AlgorithmId::Crc32, AlgorithmId::Crc32c]
        );
        assert_eq!(
            AlgorithmId::infer_by_hex_len(64),
            vec![
                AlgorithmId::Sha256,
                AlgorithmId::Sha3_256,
                AlgorithmId::Blake2s,
                AlgorithmId::Blake3
            ]
        );
        assert!(AlgorithmId::infer_by_hex_len(63).is_empty());
    }

    #[test]
    fn hex_helpers_are_strict() {
        assert!(is_hex("00FF"));
        assert!(!is_hex(""));
        assert!(!is_hex("0g"));
        assert!(is_hex_digest_of_len("ABCD", 4));
        assert!(!is_hex_digest_of_len("ABCD", 5));
        assert_eq!(normalize_hex("A1B2").as_deref(), Some("a1b2"));
        assert_eq!(normalize_hex("zz"), None);
    }

    #[test]
    fn default_suffix_algorithm_is_blake3() {
        assert_eq!(default_suffix_algorithm(), AlgorithmId::Blake3);
    }
}
