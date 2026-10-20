//! Single-pass multi-hash engine.
//!
//! compute_hashes reads a byte stream exactly once and feeds every selected
//! hasher from the same chunk. Digests are returned in canonical registry
//! order (AlgorithmId::ALL) as lowercase hex strings.
//!
//! BLAKE3 has an optional parallel path: for a single-algorithm BLAKE3 run on a
//! large file, the engine may memory-map the file and call update_rayon. That
//! path is enabled only when Blake3Policy::Auto is selected and the file is at
//! least HashOptions::mmap_threshold bytes long; multi-algorithm runs always use
//! single-pass streaming so the input is read exactly once.
//!
//! No unsafe code lives here except the single documented memory-map call; every
//! other operation is safe Rust and returns HashError instead of panicking.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use batchhash_types::AlgorithmId;
use indexmap::IndexMap;
use sha2::Digest;

/// Default read buffer size: 4 MiB.
pub const DEFAULT_BUFFER_SIZE: usize = 4 * 1024 * 1024;
/// Minimum read buffer size: 64 KiB.
pub const MIN_BUFFER_SIZE: usize = 64 * 1024;
/// Maximum read buffer size: 64 MiB.
pub const MAX_BUFFER_SIZE: usize = 64 * 1024 * 1024;

/// BLAKE3 internal parallelism policy for one hashing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blake3Policy {
    /// Allow the parallel BLAKE3 path when it is safe (single algorithm, large file).
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
    /// File size threshold for the parallel BLAKE3 memory-map path. None disables it.
    pub mmap_threshold: Option<u64>,
}

impl Default for HashOptions {
    fn default() -> Self {
        Self {
            algorithms: AlgorithmId::ALL.to_vec(),
            buffer_size: DEFAULT_BUFFER_SIZE,
            blake3: Blake3Policy::Auto,
            forced_soft: false,
            mmap_threshold: None,
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

    /// Set the file size threshold for the parallel BLAKE3 memory-map path.
    pub fn with_mmap_threshold(mut self, mmap_threshold: Option<u64>) -> Self {
        self.mmap_threshold = mmap_threshold;
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

fn is_blake3_only(options: &HashOptions) -> bool {
    !options.algorithms.is_empty()
        && options
            .algorithms
            .iter()
            .all(|algorithm| *algorithm == AlgorithmId::Blake3)
}

fn blake3_parallel_eligible(options: &HashOptions, file_len: u64) -> bool {
    !options.forced_soft
        && options.blake3 == Blake3Policy::Auto
        && is_blake3_only(options)
        && options
            .mmap_threshold
            .is_some_and(|threshold| file_len >= threshold)
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

enum HasherImpl {
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha224(sha2::Sha224),
    Sha256(sha2::Sha256),
    Sha384(sha2::Sha384),
    Sha512(sha2::Sha512),
    Sha3_256(sha3::Sha3_256),
    Sha3_512(sha3::Sha3_512),
    Blake2b(blake2::Blake2b512),
    Blake2s(blake2::Blake2s256),
    Blake3(blake3::Hasher),
    Xxh3_64(xxhash_rust::xxh3::Xxh3),
    Xxh3_128(xxhash_rust::xxh3::Xxh3),
    Xxh64(xxhash_rust::xxh64::Xxh64),
    Crc32(crc32fast::Hasher),
    Crc32c(crc32c::Crc32cHasher),
}

impl HasherImpl {
    fn new(algorithm: AlgorithmId) -> Self {
        match algorithm {
            AlgorithmId::Md5 => Self::Md5(md5::Md5::new()),
            AlgorithmId::Sha1 => Self::Sha1(sha1::Sha1::new()),
            AlgorithmId::Sha224 => Self::Sha224(sha2::Sha224::new()),
            AlgorithmId::Sha256 => Self::Sha256(sha2::Sha256::new()),
            AlgorithmId::Sha384 => Self::Sha384(sha2::Sha384::new()),
            AlgorithmId::Sha512 => Self::Sha512(sha2::Sha512::new()),
            AlgorithmId::Sha3_256 => Self::Sha3_256(sha3::Sha3_256::new()),
            AlgorithmId::Sha3_512 => Self::Sha3_512(sha3::Sha3_512::new()),
            AlgorithmId::Blake2b => Self::Blake2b(blake2::Blake2b512::new()),
            AlgorithmId::Blake2s => Self::Blake2s(blake2::Blake2s256::new()),
            AlgorithmId::Blake3 => Self::Blake3(blake3::Hasher::new()),
            AlgorithmId::Xxh3_64 => Self::Xxh3_64(xxhash_rust::xxh3::Xxh3::new()),
            AlgorithmId::Xxh3_128 => Self::Xxh3_128(xxhash_rust::xxh3::Xxh3::new()),
            AlgorithmId::Xxh64 => Self::Xxh64(xxhash_rust::xxh64::Xxh64::new(0)),
            AlgorithmId::Crc32 => Self::Crc32(crc32fast::Hasher::new()),
            AlgorithmId::Crc32c => Self::Crc32c(crc32c::Crc32cHasher::default()),
        }
    }

    fn update(&mut self, chunk: &[u8]) {
        match self {
            Self::Md5(hasher) => hasher.update(chunk),
            Self::Sha1(hasher) => hasher.update(chunk),
            Self::Sha224(hasher) => hasher.update(chunk),
            Self::Sha256(hasher) => hasher.update(chunk),
            Self::Sha384(hasher) => hasher.update(chunk),
            Self::Sha512(hasher) => hasher.update(chunk),
            Self::Sha3_256(hasher) => hasher.update(chunk),
            Self::Sha3_512(hasher) => hasher.update(chunk),
            Self::Blake2b(hasher) => hasher.update(chunk),
            Self::Blake2s(hasher) => hasher.update(chunk),
            Self::Blake3(hasher) => {
                hasher.update(chunk);
            }
            Self::Xxh3_64(hasher) => hasher.update(chunk),
            Self::Xxh3_128(hasher) => hasher.update(chunk),
            Self::Xxh64(hasher) => hasher.update(chunk),
            Self::Crc32(hasher) => hasher.update(chunk),
            Self::Crc32c(hasher) => std::hash::Hasher::write(hasher, chunk),
        }
    }

    fn finalize_hex(self) -> String {
        match self {
            Self::Md5(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha1(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha224(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha256(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha384(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha512(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha3_256(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Sha3_512(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Blake2b(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Blake2s(hasher) => digest_to_hex(&hasher.finalize()),
            Self::Blake3(hasher) => hasher.finalize().to_hex().to_string(),
            Self::Xxh3_64(hasher) => format!("{:016x}", hasher.digest()),
            Self::Xxh3_128(hasher) => format!("{:032x}", hasher.digest128()),
            Self::Xxh64(hasher) => format!("{:016x}", hasher.digest()),
            Self::Crc32(hasher) => format!("{:08x}", hasher.finalize()),
            Self::Crc32c(hasher) => {
                let checksum = std::hash::Hasher::finish(&hasher);
                format!("{:08x}", checksum as u32)
            }
        }
    }
}

fn digest_to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(DIGITS[(byte >> 4) as usize] as char);
        hex.push(DIGITS[(byte & 0x0f) as usize] as char);
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
/// Uses the parallel BLAKE3 memory-map path only when all of these hold: BLAKE3
/// is the only selected algorithm, Blake3Policy::Auto is selected, mmap_threshold is set
/// and the file is at least that large, and forced_soft is false. Otherwise the
/// file is streamed in one pass.
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

fn compute_hashes_path(
    path: &Path,
    options: &HashOptions,
    sink: Option<&mut dyn HashSink>,
    cancel: Option<&AtomicBool>,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    if file_len > 0 && blake3_parallel_eligible(options, file_len) {
        return compute_hashes_mmap_blake3(&file, sink, cancel);
    }
    compute_hashes_streaming(&mut file, options, sink, cancel)
}

fn compute_hashes_mmap_blake3(
    file: &File,
    mut sink: Option<&mut dyn HashSink>,
    cancel: Option<&AtomicBool>,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(HashError::Cancelled);
    }
    // SAFETY: Mmap::map is unsafe because mutating or truncating the mapped file
    // while the mapping is alive is undefined behavior. Batch hashing requires
    // stable inputs and documents that files must not change during a batch; the
    // mapping is read-only, private to this call and dropped before returning.
    let mapped = unsafe { memmap2::Mmap::map(file)? };
    if let Some(hook) = sink.as_deref_mut() {
        hook.on_chunk(&mapped);
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update_rayon(&mapped);
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(HashError::Cancelled);
    }
    let mut hashes = IndexMap::new();
    hashes.insert(AlgorithmId::Blake3, hasher.finalize().to_hex().to_string());
    Ok(hashes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn buffer_size_defaults_and_clamps() {
        assert_eq!(HashOptions::default().effective_buffer_size(), DEFAULT_BUFFER_SIZE);
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
    fn blake3_parallel_policy_is_restrictive() {
        let blake3_only = HashOptions::new(vec![AlgorithmId::Blake3]);
        assert!(!blake3_parallel_eligible(&blake3_only, 1 << 20));

        let with_threshold =
            HashOptions::new(vec![AlgorithmId::Blake3]).with_mmap_threshold(Some(1024));
        assert!(blake3_parallel_eligible(&with_threshold, 1024));
        assert!(!blake3_parallel_eligible(&with_threshold, 1023));

        let multi = HashOptions::new(vec![AlgorithmId::Blake3, AlgorithmId::Sha256])
            .with_mmap_threshold(Some(1024));
        assert!(!blake3_parallel_eligible(&multi, 2048));

        let disabled = HashOptions::new(vec![AlgorithmId::Blake3])
            .with_mmap_threshold(Some(1024))
            .with_blake3(Blake3Policy::Disabled);
        assert!(!blake3_parallel_eligible(&disabled, 2048));

        let forced = HashOptions::new(vec![AlgorithmId::Blake3])
            .with_mmap_threshold(Some(1024))
            .with_forced_soft(true);
        assert!(!blake3_parallel_eligible(&forced, 2048));
    }

    #[test]
    fn cancellation_is_checked_at_chunk_boundaries() {
        let options = HashOptions::new(vec![AlgorithmId::Md5])
            .with_buffer_size(MIN_BUFFER_SIZE);
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
        let options = HashOptions::new(vec![AlgorithmId::Md5])
            .with_buffer_size(MIN_BUFFER_SIZE);
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
