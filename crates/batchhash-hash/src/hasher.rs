//! Single-pass multi-hash engine.
//!
//! compute_hashes reads a byte stream exactly once and feeds every selected
//! hasher from the same chunk. Digests are returned in canonical registry
//! order (AlgorithmId::ALL) as lowercase hex strings.
//!
//! File input always streams. An earlier revision memory-mapped large files for
//! a single-algorithm BLAKE3 run and called update_rayon; that shortcut was
//! removed because the documented "files must not change during a batch"
//! precondition cannot be enforced by a safe API that maps the file. Every input
//! is now read in chunks, so the bytes are read exactly once and the engine never
//! aliases memory that another process could mutate.
//!
//! Blake3Policy::Auto and Blake3Policy::Disabled therefore select the same code
//! path today; the policy is kept for API and contract compatibility.
//!
//! This module contains no unsafe code; every operation is safe Rust and returns
//! HashError instead of panicking.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use batchhash_types::AlgorithmId;
use indexmap::IndexMap;
use sha2::Digest;
use sha3::digest::{ExtendableOutput, Update as XofUpdate, XofReader};

/// Default read buffer size: 4 MiB.
pub const DEFAULT_BUFFER_SIZE: usize = 4 * 1024 * 1024;
/// Minimum read buffer size: 64 KiB.
pub const MIN_BUFFER_SIZE: usize = 64 * 1024;
/// Maximum read buffer size: 64 MiB.
pub const MAX_BUFFER_SIZE: usize = 64 * 1024 * 1024;

/// BLAKE3 internal parallelism policy for one hashing pass.
///
/// Both variants currently select single-pass streaming: the memory-mapped
/// parallel path was removed (see the module documentation). The policy is kept
/// so the public options contract and the bridge hardware report stay stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blake3Policy {
    /// Permit library-level parallelism. Equivalent to Disabled for file input.
    #[default]
    Auto,
    /// Never use the parallel or multithreaded BLAKE3 path.
    Disabled,
}

/// Options for one hashing pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashOptions {
    /// Algorithms to compute. Duplicates are ignored; output is canonical registry order.
    pub algorithms: Vec<AlgorithmId>,
    /// Requested read buffer size in bytes, clamped between MIN_BUFFER_SIZE and MAX_BUFFER_SIZE.
    pub buffer_size: usize,
    /// BLAKE3 internal parallelism policy.
    pub blake3: Blake3Policy,
    /// When true, optional parallel/multithreaded hash paths are disabled.
    pub forced_soft: bool,
}

impl Default for HashOptions {
    fn default() -> Self {
        Self {
            algorithms: AlgorithmId::ALL.to_vec(),
            buffer_size: DEFAULT_BUFFER_SIZE,
            blake3: Blake3Policy::Auto,
            forced_soft: false,
        }
    }
}

impl HashOptions {
    /// Create options for the given algorithms with all other settings at their defaults.
    pub fn new(algorithms: Vec<AlgorithmId>) -> Self {
        Self {
            algorithms,
            ..Self::default()
        }
    }

    /// Set the requested read buffer size (clamped when used).
    pub fn with_buffer_size(mut self, buffer_size: usize) -> Self {
        self.buffer_size = buffer_size;
        self
    }

    /// Set the BLAKE3 parallelism policy.
    pub fn with_blake3(mut self, blake3: Blake3Policy) -> Self {
        self.blake3 = blake3;
        self
    }

    /// Force the engine onto non-parallel code paths.
    pub fn with_forced_soft(mut self, forced_soft: bool) -> Self {
        self.forced_soft = forced_soft;
        self
    }

    /// Buffer size actually used, clamped to the supported range.
    pub fn effective_buffer_size(&self) -> usize {
        self.buffer_size.clamp(MIN_BUFFER_SIZE, MAX_BUFFER_SIZE)
    }

    /// Selected algorithms in canonical registry order, deduplicated.
    pub fn canonical_algorithms(&self) -> Vec<AlgorithmId> {
        AlgorithmId::ALL
            .iter()
            .copied()
            .filter(|algorithm| self.algorithms.contains(algorithm))
            .collect()
    }
}

/// Hook invoked once for every non-empty chunk read from an input stream.
///
/// The slice is the exact bytes fed to every hasher in that iteration. The hook
/// runs on the worker thread that owns the read loop, so implementations must be
/// cheap and must not re-enter the hashing API.
pub trait HashSink {
    /// Observe one chunk that was just fed to all hashers.
    fn on_chunk(&mut self, chunk: &[u8]);
}

impl<F> HashSink for F
where
    F: FnMut(&[u8]),
{
    fn on_chunk(&mut self, chunk: &[u8]) {
        (*self)(chunk);
    }
}

/// Errors returned by the hash engine.
#[derive(Debug, thiserror::Error)]
pub enum HashError {
    /// The underlying reader or file operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    /// The caller is not allowed to read the requested input.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// The requested combination is not supported on this platform.
    #[error("unsupported hashing operation: {0}")]
    Unsupported(String),
    /// No algorithm was selected.
    #[error("no hash algorithms were selected")]
    NoAlgorithms,
    /// The operation was cancelled at a chunk boundary.
    #[error("hashing was cancelled")]
    Cancelled,
}

// The digest states vary in size (BLAKE3 is the largest), so every variant is
// boxed to keep HasherImpl small and uniformly sized.
enum HasherImpl {
    Md5(Box<md5::Md5>),
    Sha1(Box<sha1::Sha1>),
    Sha224(Box<sha2::Sha224>),
    Sha256(Box<sha2::Sha256>),
    Sha384(Box<sha2::Sha384>),
    Sha512(Box<sha2::Sha512>),
    Sha3_256(Box<sha3::Sha3_256>),
    Sha3_512(Box<sha3::Sha3_512>),
    Blake2b(Box<blake2::Blake2b512>),
    Blake2s(Box<blake2::Blake2s256>),
    Blake3(Box<blake3::Hasher>),
    Xxh3_64(Box<xxhash_rust::xxh3::Xxh3>),
    Xxh3_128(Box<xxhash_rust::xxh3::Xxh3>),
    Xxh64(Box<xxhash_rust::xxh64::Xxh64>),
    Crc32(Box<crc32fast::Hasher>),
    Crc32c(Box<crc32c::Crc32cHasher>),
    Md2(Box<md2::Md2>),
    Md4(Box<md4::Md4>),
    Sha512_224(Box<sha2::Sha512_224>),
    Sha512_256(Box<sha2::Sha512_256>),
    Sha3_224(Box<sha3::Sha3_224>),
    Sha3_384(Box<sha3::Sha3_384>),
    Keccak224(Box<sha3::Keccak224>),
    Keccak256(Box<sha3::Keccak256>),
    Keccak384(Box<sha3::Keccak384>),
    Keccak512(Box<sha3::Keccak512>),
    Shake128(Box<sha3::Shake128>),
    Shake256(Box<sha3::Shake256>),
    Ripemd128(Box<ripemd::Ripemd128>),
    Ripemd160(Box<ripemd::Ripemd160>),
    Ripemd256(Box<ripemd::Ripemd256>),
    Ripemd320(Box<ripemd::Ripemd320>),
    Sm3(Box<sm3::Sm3>),
    Gost94(Box<gost94::Gost94CryptoPro>),
    Streebog256(Box<streebog::Streebog256>),
    Streebog512(Box<streebog::Streebog512>),
    Whirlpool(Box<whirlpool::Whirlpool>),
    Tiger(Box<tiger::Tiger>),
    Skein256(Box<skein::Skein256>),
    Skein512(Box<skein::Skein512>),
    Groestl256(Box<groestl::Groestl256>),
    Groestl512(Box<groestl::Groestl512>),
    Jh256(Box<jh::Jh256>),
    Jh512(Box<jh::Jh512>),
}

impl HasherImpl {
    fn new(algorithm: AlgorithmId) -> Self {
        match algorithm {
            AlgorithmId::Md5 => Self::Md5(Box::new(md5::Md5::new())),
            AlgorithmId::Sha1 => Self::Sha1(Box::new(sha1::Sha1::new())),
            AlgorithmId::Sha224 => Self::Sha224(Box::new(sha2::Sha224::new())),
            AlgorithmId::Sha256 => Self::Sha256(Box::new(sha2::Sha256::new())),
            AlgorithmId::Sha384 => Self::Sha384(Box::new(sha2::Sha384::new())),
            AlgorithmId::Sha512 => Self::Sha512(Box::new(sha2::Sha512::new())),
            AlgorithmId::Sha3_256 => Self::Sha3_256(Box::new(sha3::Sha3_256::new())),
            AlgorithmId::Sha3_512 => Self::Sha3_512(Box::new(sha3::Sha3_512::new())),
            AlgorithmId::Blake2b => Self::Blake2b(Box::new(blake2::Blake2b512::new())),
            AlgorithmId::Blake2s => Self::Blake2s(Box::new(blake2::Blake2s256::new())),
            AlgorithmId::Blake3 => Self::Blake3(Box::new(blake3::Hasher::new())),
            AlgorithmId::Xxh3_64 => Self::Xxh3_64(Box::new(xxhash_rust::xxh3::Xxh3::new())),
            AlgorithmId::Xxh3_128 => Self::Xxh3_128(Box::new(xxhash_rust::xxh3::Xxh3::new())),
            AlgorithmId::Xxh64 => Self::Xxh64(Box::new(xxhash_rust::xxh64::Xxh64::new(0))),
            AlgorithmId::Crc32 => Self::Crc32(Box::new(crc32fast::Hasher::new())),
            AlgorithmId::Crc32c => Self::Crc32c(Box::<crc32c::Crc32cHasher>::default()),
            AlgorithmId::Md2 => Self::Md2(Box::new(md2::Md2::new())),
            AlgorithmId::Md4 => Self::Md4(Box::new(md4::Md4::new())),
            AlgorithmId::Sha512_224 => Self::Sha512_224(Box::new(sha2::Sha512_224::new())),
            AlgorithmId::Sha512_256 => Self::Sha512_256(Box::new(sha2::Sha512_256::new())),
            AlgorithmId::Sha3_224 => Self::Sha3_224(Box::new(sha3::Sha3_224::new())),
            AlgorithmId::Sha3_384 => Self::Sha3_384(Box::new(sha3::Sha3_384::new())),
            AlgorithmId::Keccak224 => Self::Keccak224(Box::new(sha3::Keccak224::new())),
            AlgorithmId::Keccak256 => Self::Keccak256(Box::new(sha3::Keccak256::new())),
            AlgorithmId::Keccak384 => Self::Keccak384(Box::new(sha3::Keccak384::new())),
            AlgorithmId::Keccak512 => Self::Keccak512(Box::new(sha3::Keccak512::new())),
            AlgorithmId::Shake128 => Self::Shake128(Box::default()),
            AlgorithmId::Shake256 => Self::Shake256(Box::default()),
            AlgorithmId::Ripemd128 => Self::Ripemd128(Box::new(ripemd::Ripemd128::new())),
            AlgorithmId::Ripemd160 => Self::Ripemd160(Box::new(ripemd::Ripemd160::new())),
            AlgorithmId::Ripemd256 => Self::Ripemd256(Box::new(ripemd::Ripemd256::new())),
            AlgorithmId::Ripemd320 => Self::Ripemd320(Box::new(ripemd::Ripemd320::new())),
            AlgorithmId::Sm3 => Self::Sm3(Box::new(sm3::Sm3::new())),
            AlgorithmId::Gost94 => Self::Gost94(Box::new(gost94::Gost94CryptoPro::new())),
            AlgorithmId::Streebog256 => Self::Streebog256(Box::new(streebog::Streebog256::new())),
            AlgorithmId::Streebog512 => Self::Streebog512(Box::new(streebog::Streebog512::new())),
            AlgorithmId::Whirlpool => Self::Whirlpool(Box::new(whirlpool::Whirlpool::new())),
            AlgorithmId::Tiger => Self::Tiger(Box::new(tiger::Tiger::new())),
            AlgorithmId::Skein256 => Self::Skein256(Box::new(skein::Skein256::new())),
            AlgorithmId::Skein512 => Self::Skein512(Box::new(skein::Skein512::new())),
            AlgorithmId::Groestl256 => Self::Groestl256(Box::new(groestl::Groestl256::new())),
            AlgorithmId::Groestl512 => Self::Groestl512(Box::new(groestl::Groestl512::new())),
            AlgorithmId::Jh256 => Self::Jh256(Box::new(jh::Jh256::new())),
            AlgorithmId::Jh512 => Self::Jh512(Box::new(jh::Jh512::new())),
        }
    }

    fn update(&mut self, chunk: &[u8]) {
        match self {
            Self::Md5(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha1(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha224(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha384(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha3_256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha3_512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Blake2b(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Blake2s(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Blake3(hasher) => {
                hasher.update(chunk);
            }
            Self::Xxh3_64(hasher) => {
                hasher.update(chunk);
            }
            Self::Xxh3_128(hasher) => {
                hasher.update(chunk);
            }
            Self::Xxh64(hasher) => {
                hasher.update(chunk);
            }
            Self::Crc32(hasher) => {
                hasher.update(chunk);
            }
            Self::Crc32c(hasher) => std::hash::Hasher::write(&mut **hasher, chunk),
            Self::Md2(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Md4(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha512_224(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha512_256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha3_224(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sha3_384(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Keccak224(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Keccak256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Keccak384(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Keccak512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Shake128(hasher) => {
                XofUpdate::update(&mut **hasher, chunk);
            }
            Self::Shake256(hasher) => {
                XofUpdate::update(&mut **hasher, chunk);
            }
            Self::Ripemd128(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Ripemd160(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Ripemd256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Ripemd320(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Sm3(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Gost94(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Streebog256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Streebog512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Whirlpool(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Tiger(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Skein256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Skein512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Groestl256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Groestl512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Jh256(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
            Self::Jh512(hasher) => {
                Digest::update(&mut **hasher, chunk);
            }
        }
    }

    fn finalize_hex(self) -> String {
        match self {
            Self::Md5(hasher) => finalize_digest(*hasher),
            Self::Sha1(hasher) => finalize_digest(*hasher),
            Self::Sha224(hasher) => finalize_digest(*hasher),
            Self::Sha256(hasher) => finalize_digest(*hasher),
            Self::Sha384(hasher) => finalize_digest(*hasher),
            Self::Sha512(hasher) => finalize_digest(*hasher),
            Self::Sha3_256(hasher) => finalize_digest(*hasher),
            Self::Sha3_512(hasher) => finalize_digest(*hasher),
            Self::Blake2b(hasher) => finalize_digest(*hasher),
            Self::Blake2s(hasher) => finalize_digest(*hasher),
            Self::Blake3(hasher) => {
                let hasher = *hasher;
                hasher.finalize().to_hex().to_string()
            }
            Self::Xxh3_64(hasher) => {
                let hasher = *hasher;
                format!("{:016x}", hasher.digest())
            }
            Self::Xxh3_128(hasher) => {
                let hasher = *hasher;
                format!("{:032x}", hasher.digest128())
            }
            Self::Xxh64(hasher) => {
                let hasher = *hasher;
                format!("{:016x}", hasher.digest())
            }
            Self::Crc32(hasher) => {
                let hasher = *hasher;
                format!("{:08x}", hasher.finalize())
            }
            Self::Crc32c(hasher) => {
                let hasher = *hasher;
                let checksum = std::hash::Hasher::finish(&hasher);
                format!("{:08x}", checksum as u32)
            }
            Self::Md2(hasher) => finalize_digest(*hasher),
            Self::Md4(hasher) => finalize_digest(*hasher),
            Self::Sha512_224(hasher) => finalize_digest(*hasher),
            Self::Sha512_256(hasher) => finalize_digest(*hasher),
            Self::Sha3_224(hasher) => finalize_digest(*hasher),
            Self::Sha3_384(hasher) => finalize_digest(*hasher),
            Self::Keccak224(hasher) => finalize_digest(*hasher),
            Self::Keccak256(hasher) => finalize_digest(*hasher),
            Self::Keccak384(hasher) => finalize_digest(*hasher),
            Self::Keccak512(hasher) => finalize_digest(*hasher),
            Self::Shake128(hasher) => {
                let hasher = *hasher;
                let mut reader = hasher.finalize_xof();
                let mut out = [0u8; 16];
                XofReader::read(&mut reader, &mut out);
                digest_to_hex(&out)
            }
            Self::Shake256(hasher) => {
                let hasher = *hasher;
                let mut reader = hasher.finalize_xof();
                let mut out = [0u8; 32];
                XofReader::read(&mut reader, &mut out);
                digest_to_hex(&out)
            }
            Self::Ripemd128(hasher) => finalize_digest(*hasher),
            Self::Ripemd160(hasher) => finalize_digest(*hasher),
            Self::Ripemd256(hasher) => finalize_digest(*hasher),
            Self::Ripemd320(hasher) => finalize_digest(*hasher),
            Self::Sm3(hasher) => finalize_digest(*hasher),
            Self::Gost94(hasher) => finalize_digest(*hasher),
            Self::Streebog256(hasher) => finalize_digest(*hasher),
            Self::Streebog512(hasher) => finalize_digest(*hasher),
            Self::Whirlpool(hasher) => finalize_digest(*hasher),
            Self::Tiger(hasher) => finalize_digest(*hasher),
            Self::Skein256(hasher) => finalize_digest(*hasher),
            Self::Skein512(hasher) => finalize_digest(*hasher),
            Self::Groestl256(hasher) => finalize_digest(*hasher),
            Self::Groestl512(hasher) => finalize_digest(*hasher),
            Self::Jh256(hasher) => finalize_digest(*hasher),
            Self::Jh512(hasher) => finalize_digest(*hasher),
        }
    }
}

fn finalize_digest<D: Digest>(hasher: D) -> String {
    let digest = hasher.finalize();
    digest_to_hex(&digest)
}

fn digest_to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let high = *byte >> 4;
        let low = *byte & 0x0f;
        hex.push(DIGITS[high as usize] as char);
        hex.push(DIGITS[low as usize] as char);
    }
    hex
}

fn create_hashers(options: &HashOptions) -> Result<Vec<(AlgorithmId, HasherImpl)>, HashError> {
    let algorithms = options.canonical_algorithms();
    if algorithms.is_empty() {
        return Err(HashError::NoAlgorithms);
    }
    Ok(algorithms
        .into_iter()
        .map(|algorithm| (algorithm, HasherImpl::new(algorithm)))
        .collect())
}

/// Compute all selected hashes from a reader with a single I/O pass.
///
/// The stream is read exactly once; every chunk feeds every selected hasher.
/// The result contains one lowercase hex digest per selected algorithm in
/// canonical registry order. An empty algorithm list returns HashError::NoAlgorithms.
pub fn compute_hashes(
    reader: &mut dyn Read,
    options: &HashOptions,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    compute_hashes_streaming(reader, options, None, None)
}

/// Like compute_hashes, but also reports every read chunk to the supplied sink.
///
/// This is the single-pass hook used by the scheduler for progress accounting and
/// by tests that count I/O passes.
pub fn compute_hashes_with_sink(
    reader: &mut dyn Read,
    options: &HashOptions,
    sink: &mut dyn HashSink,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    compute_hashes_streaming(reader, options, Some(sink), None)
}

/// Convenience wrapper: hash a reader with default options for the given algorithms.
pub fn hash_reader(
    reader: &mut dyn Read,
    algorithms: &[AlgorithmId],
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    compute_hashes(reader, &HashOptions::new(algorithms.to_vec()))
}

/// Compute hashes for a file path.
///
/// The file is always streamed in one pass: the parallel BLAKE3 memory-map path
/// was removed because a safe API cannot enforce the stability precondition that
/// keeping a mapping assumes. BLAKE3 output is identical to the streaming
/// implementation, because both feed the same byte stream to the same hasher.
pub fn compute_hashes_file(
    path: &Path,
    options: &HashOptions,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    compute_hashes_path(path, options, None, None)
}

pub(crate) fn compute_hashes_streaming(
    reader: &mut dyn Read,
    options: &HashOptions,
    mut sink: Option<&mut dyn HashSink>,
    cancel: Option<&AtomicBool>,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    let mut hashers = create_hashers(options)?;
    let mut buffer = vec![0u8; options.effective_buffer_size()];
    loop {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(HashError::Cancelled);
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(HashError::Io(error)),
        };
        let chunk = &buffer[..read];
        for (_, hasher) in hashers.iter_mut() {
            hasher.update(chunk);
        }
        if let Some(hook) = sink.as_deref_mut() {
            hook.on_chunk(chunk);
        }
    }
    let mut hashes = IndexMap::new();
    for (algorithm, hasher) in hashers {
        hashes.insert(algorithm, hasher.finalize_hex());
    }
    Ok(hashes)
}

pub(crate) fn compute_hashes_path(
    path: &Path,
    options: &HashOptions,
    sink: Option<&mut dyn HashSink>,
    cancel: Option<&AtomicBool>,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    let mut file = File::open(path)?;
    compute_hashes_streaming(&mut file, options, sink, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn buffer_size_defaults_and_clamps() {
        assert_eq!(
            HashOptions::default().effective_buffer_size(),
            DEFAULT_BUFFER_SIZE
        );
        let small = HashOptions::default().with_buffer_size(1);
        assert_eq!(small.effective_buffer_size(), MIN_BUFFER_SIZE);
        let huge = HashOptions::default().with_buffer_size(usize::MAX);
        assert_eq!(huge.effective_buffer_size(), MAX_BUFFER_SIZE);
        let exact = HashOptions::default().with_buffer_size(1024 * 1024);
        assert_eq!(exact.effective_buffer_size(), 1024 * 1024);
    }

    #[test]
    fn algorithms_are_deduplicated_in_registry_order() {
        let options = HashOptions::new(vec![
            AlgorithmId::Sha256,
            AlgorithmId::Md5,
            AlgorithmId::Sha256,
        ]);
        assert_eq!(
            options.canonical_algorithms(),
            vec![AlgorithmId::Md5, AlgorithmId::Sha256]
        );
    }

    #[test]
    fn empty_algorithm_list_is_rejected() {
        let options = HashOptions::new(Vec::new());
        let mut reader = Cursor::new(b"data".to_vec());
        let result = compute_hashes(&mut reader, &options);
        assert!(matches!(result, Err(HashError::NoAlgorithms)));
    }

    #[test]
    fn blake3_policy_variants_produce_identical_digests() {
        let data = vec![9u8; 4096];
        let auto = HashOptions::new(vec![AlgorithmId::Blake3]);
        let disabled =
            HashOptions::new(vec![AlgorithmId::Blake3]).with_blake3(Blake3Policy::Disabled);
        let forced = HashOptions::new(vec![AlgorithmId::Blake3]).with_forced_soft(true);
        let mut auto_reader = Cursor::new(data.clone());
        let mut disabled_reader = Cursor::new(data.clone());
        let mut forced_reader = Cursor::new(data);
        let auto_hashes =
            compute_hashes(&mut auto_reader, &auto).expect("auto hashing must succeed");
        let disabled_hashes =
            compute_hashes(&mut disabled_reader, &disabled).expect("disabled hashing must succeed");
        let forced_hashes =
            compute_hashes(&mut forced_reader, &forced).expect("forced soft hashing must succeed");
        assert_eq!(auto_hashes, disabled_hashes);
        assert_eq!(auto_hashes, forced_hashes);
    }

    #[test]
    fn file_hashing_streams_in_chunks_through_the_sink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("input.bin");
        let data = vec![5u8; MIN_BUFFER_SIZE * 2 + 7];
        std::fs::write(&path, &data).expect("fixture");
        let options = HashOptions::new(vec![AlgorithmId::Blake3]).with_buffer_size(MIN_BUFFER_SIZE);
        let mut chunk_lengths = Vec::new();
        let mut sink = |chunk: &[u8]| chunk_lengths.push(chunk.len());
        let hashes = compute_hashes_path(&path, &options, Some(&mut sink), None)
            .expect("file hashing must succeed");
        // The file is read in buffer-sized chunks, never as one mapping.
        assert_eq!(chunk_lengths, vec![MIN_BUFFER_SIZE, MIN_BUFFER_SIZE, 7]);
        let mut reference_reader = Cursor::new(data);
        let reference = compute_hashes_streaming(&mut reference_reader, &options, None, None)
            .expect("streaming reference");
        assert_eq!(hashes, reference);
    }

    #[test]
    fn file_hashing_observes_cancellation_before_any_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("input.bin");
        std::fs::write(&path, vec![1u8; 32]).expect("fixture");
        let cancel = AtomicBool::new(true);
        let result = compute_hashes_path(
            &path,
            &HashOptions::new(vec![AlgorithmId::Blake3]),
            None,
            Some(&cancel),
        );
        assert!(matches!(result, Err(HashError::Cancelled)));
    }

    #[test]
    fn cancellation_is_checked_at_chunk_boundaries() {
        let options = HashOptions::new(vec![AlgorithmId::Md5]).with_buffer_size(MIN_BUFFER_SIZE);
        let data = vec![7u8; MIN_BUFFER_SIZE * 3];
        let mut reader = Cursor::new(data);
        let cancel = AtomicBool::new(false);
        let mut sink = |_chunk: &[u8]| cancel.store(true, Ordering::Relaxed);
        let result =
            compute_hashes_streaming(&mut reader, &options, Some(&mut sink), Some(&cancel));
        assert!(matches!(result, Err(HashError::Cancelled)));
    }

    #[test]
    fn sink_observes_each_chunk_exactly_once() {
        let options = HashOptions::new(vec![AlgorithmId::Md5]).with_buffer_size(MIN_BUFFER_SIZE);
        let data = vec![3u8; MIN_BUFFER_SIZE + 10];
        let mut reader = Cursor::new(data);
        let mut chunk_lengths = Vec::new();
        let mut sink = |chunk: &[u8]| chunk_lengths.push(chunk.len());
        let result = compute_hashes_streaming(&mut reader, &options, Some(&mut sink), None)
            .expect("hashing must succeed");
        assert_eq!(result.len(), 1);
        assert_eq!(chunk_lengths, vec![MIN_BUFFER_SIZE, 10]);
    }
}
