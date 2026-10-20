//! Proof that multi-algorithm hashing performs exactly one I/O pass.

use std::io::{self, Cursor, Read};

use batchhash_hash::{compute_hashes, compute_hashes_with_sink, HashOptions};
use batchhash_types::AlgorithmId;

const ALGORITHMS: [AlgorithmId; 6] = [
    AlgorithmId::Md5,
    AlgorithmId::Sha256,
    AlgorithmId::Sha3_256,
    AlgorithmId::Blake2s,
    AlgorithmId::Blake3,
    AlgorithmId::Crc32,
];

const BUFFER_SIZE: usize = 64 * 1024;

struct CountingReader {
    inner: Cursor<Vec<u8>>,
    read_calls: usize,
    bytes_read: usize,
}

impl CountingReader {
    fn new(data: Vec<u8>) -> Self {
        Self {
            inner: Cursor::new(data),
            read_calls: 0,
            bytes_read: 0,
        }
    }
}

impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if read > 0 {
            self.read_calls += 1;
            self.bytes_read += read;
        }
        Ok(read)
    }
}

fn patterned_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| (index.wrapping_mul(31) % 251) as u8)
        .collect()
}

#[test]
fn multi_algorithm_hashing_reads_the_stream_once() {
    let data = patterned_data(200_000);
    let options = HashOptions::new(ALGORITHMS.to_vec()).with_buffer_size(BUFFER_SIZE);
    let mut reader = CountingReader::new(data.clone());

    let hashes = compute_hashes(&mut reader, &options).expect("hashing must succeed");

    let expected_calls = data.len().div_ceil(BUFFER_SIZE);
    assert_eq!(reader.bytes_read, data.len(), "all bytes must be read");
    assert_eq!(
        reader.read_calls, expected_calls,
        "one pass over the stream, not one pass per algorithm"
    );
    assert_eq!(hashes.len(), ALGORITHMS.len());

    for algorithm in ALGORITHMS {
        let individual_options = HashOptions::new(vec![algorithm]);
        let mut individual_reader = Cursor::new(data.clone());
        let individual = compute_hashes(&mut individual_reader, &individual_options)
            .expect("individual hashing must succeed");
        assert_eq!(
            hashes.get(&algorithm),
            individual.get(&algorithm),
            "{algorithm} must match an individual run"
        );
    }
}

#[test]
fn sink_observes_every_read_chunk_once() {
    let data = patterned_data(130_000);
    let options = HashOptions::new(vec![AlgorithmId::Blake3]).with_buffer_size(BUFFER_SIZE);
    let mut reader = CountingReader::new(data.clone());
    let mut chunk_count = 0usize;
    let mut chunk_bytes = 0usize;
    let mut sink = |chunk: &[u8]| {
        chunk_count += 1;
        chunk_bytes += chunk.len();
    };
    let hashes = compute_hashes_with_sink(&mut reader, &options, &mut sink)
        .expect("hashing must succeed");

    assert_eq!(chunk_bytes, data.len());
    assert_eq!(chunk_count, reader.read_calls);
    assert_eq!(chunk_count, data.len().div_ceil(BUFFER_SIZE));
    assert_eq!(hashes.len(), 1);
}

#[test]
fn empty_input_is_hashed_without_any_read() {
    let options = HashOptions::new(vec![AlgorithmId::Sha256, AlgorithmId::Blake3]);
    let mut reader = CountingReader::new(Vec::new());
    let hashes = compute_hashes(&mut reader, &options).expect("hashing must succeed");
    assert_eq!(reader.read_calls, 0);
    assert_eq!(reader.bytes_read, 0);
    assert_eq!(
        hashes.get(&AlgorithmId::Sha256).map(String::as_str),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
    assert_eq!(
        hashes.get(&AlgorithmId::Blake3).map(String::as_str),
        Some("af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")
    );
}
