//! HashOptions behaviour: clamping, policy, per-file hashing and equality.

use std::fs;
use std::io::Cursor;

use batchhash_hash::{
    compute_hashes, compute_hashes_file, compute_hashes_with_sink, hash_reader, Blake3Policy,
    HashError, HashOptions,
};
use batchhash_types::AlgorithmId;

const SMALL_BUFFER: usize = 64 * 1024;

fn patterned_data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| (index.wrapping_mul(31) % 251) as u8)
        .collect()
}

#[test]
fn buffer_size_is_clamped_to_supported_range() {
    assert_eq!(
        HashOptions::default().effective_buffer_size(),
        4 * 1024 * 1024
    );
    assert_eq!(
        HashOptions::new(vec![AlgorithmId::Md5])
            .with_buffer_size(0)
            .effective_buffer_size(),
        64 * 1024
    );
    assert_eq!(
        HashOptions::new(vec![AlgorithmId::Md5])
            .with_buffer_size(usize::MAX)
            .effective_buffer_size(),
        64 * 1024 * 1024
    );
    assert_eq!(
        HashOptions::new(vec![AlgorithmId::Md5])
            .with_buffer_size(123_456)
            .effective_buffer_size(),
        123_456
    );
}

#[test]
fn every_algorithm_matches_individual_runs() {
    let data = patterned_data(300_000);
    let algorithms = AlgorithmId::ALL.to_vec();
    let mut reader = Cursor::new(data.clone());
    let combined = compute_hashes(&mut reader, &HashOptions::new(algorithms.clone()))
        .expect("combined hashing must succeed");
    assert_eq!(combined.len(), algorithms.len());
    for algorithm in algorithms {
        let mut individual_reader = Cursor::new(data.clone());
        let individual = compute_hashes(&mut individual_reader, &HashOptions::new(vec![algorithm]))
            .expect("individual hashing must succeed");
        assert_eq!(
            combined.get(&algorithm),
            individual.get(&algorithm),
            "{algorithm} must match an individual run"
        );
        assert_eq!(
            combined.get(&algorithm).map(String::len),
            Some(algorithm.hex_len())
        );
    }
    // Canonical registry order is preserved in the result.
    let keys: Vec<AlgorithmId> = combined.keys().copied().collect();
    assert_eq!(keys, AlgorithmId::ALL.to_vec());
}

#[test]
fn forced_soft_and_auto_produce_identical_hashes() {
    let data = patterned_data(200_000);
    let auto = HashOptions::new(AlgorithmId::ALL.to_vec());
    let forced_soft = HashOptions::new(AlgorithmId::ALL.to_vec()).with_forced_soft(true);
    let auto_hashes =
        compute_hashes(&mut Cursor::new(data.clone()), &auto).expect("auto hashing must succeed");
    let soft_hashes = compute_hashes(&mut Cursor::new(data), &forced_soft)
        .expect("forced soft hashing must succeed");
    assert_eq!(auto_hashes, soft_hashes);
}

#[test]
fn hash_reader_is_a_compute_hashes_convenience() {
    let data = patterned_data(10_000);
    let mut reader = Cursor::new(data.clone());
    let convenience = hash_reader(&mut reader, &[AlgorithmId::Sha1, AlgorithmId::Xxh3_64])
        .expect("convenience hashing must succeed");
    let mut other_reader = Cursor::new(data);
    let reference = compute_hashes(
        &mut other_reader,
        &HashOptions::new(vec![AlgorithmId::Sha1, AlgorithmId::Xxh3_64]),
    )
    .expect("reference hashing must succeed");
    assert_eq!(convenience, reference);
}

#[test]
fn empty_algorithm_selection_is_rejected() {
    let mut reader = Cursor::new(b"data".to_vec());
    let error = compute_hashes(&mut reader, &HashOptions::new(Vec::new()))
        .expect_err("no algorithms must be an error");
    assert!(matches!(error, HashError::NoAlgorithms));
}

#[test]
fn file_hashing_matches_reader_hashing_for_every_policy() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let path = dir.path().join("large.bin");
    let data = patterned_data(300_000);
    fs::write(&path, data.as_slice()).expect("file must be written");

    let streaming_options = HashOptions::new(vec![AlgorithmId::Blake3]);
    let streaming = compute_hashes(&mut Cursor::new(data.clone()), &streaming_options)
        .expect("streaming hashing must succeed");

    // File input always streams, so every policy must reproduce the same digest.
    assert_eq!(
        compute_hashes_file(&path, &streaming_options).expect("file hashing must succeed"),
        streaming
    );

    let disabled = HashOptions::new(vec![AlgorithmId::Blake3]).with_blake3(Blake3Policy::Disabled);
    assert_eq!(
        compute_hashes_file(&path, &disabled).expect("disabled policy must stream"),
        streaming
    );

    let forced = HashOptions::new(vec![AlgorithmId::Blake3]).with_forced_soft(true);
    assert_eq!(
        compute_hashes_file(&path, &forced).expect("forced soft policy must stream"),
        streaming
    );

    let multi = HashOptions::new(vec![AlgorithmId::Blake3, AlgorithmId::Sha256]);
    let multi_streaming = compute_hashes(&mut Cursor::new(data), &multi)
        .expect("multi-algorithm streaming must succeed");
    assert_eq!(
        compute_hashes_file(&path, &multi).expect("multi-algorithm file hashing must succeed"),
        multi_streaming
    );
}

/// Progress contract: every chunk is fed to all hashers before the sink observes
/// it, so bytes-done can only reach the file size once the whole input has been
/// hashed chunk by chunk (never before the digest work started).
#[test]
fn chunks_are_hashed_before_the_sink_observes_them() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let path = dir.path().join("chunked.bin");
    let data = patterned_data(SMALL_BUFFER * 2 + 11);
    fs::write(&path, data.as_slice()).expect("file must be written");

    let options =
        HashOptions::new(vec![AlgorithmId::Blake3, AlgorithmId::Md5]).with_buffer_size(SMALL_BUFFER);
    let mut chunks = Vec::new();
    let mut sink = |chunk: &[u8]| chunks.push(chunk.len());
    let mut reader = Cursor::new(data.clone());
    let from_reader = compute_hashes_with_sink(&mut reader, &options, &mut sink)
        .expect("reader hashing must succeed");
    assert_eq!(chunks, vec![SMALL_BUFFER, SMALL_BUFFER, 11]);
    assert_eq!(chunks.iter().sum::<usize>(), data.len());

    // File input streams the same way: one pass, buffer-sized chunks.
    let mut file_chunks = Vec::new();
    let mut file_sink = |chunk: &[u8]| file_chunks.push(chunk.len());
    let mut file_reader = fs::File::open(&path).expect("open fixture");
    let from_file = compute_hashes_with_sink(&mut file_reader, &options, &mut file_sink)
        .expect("file hashing must succeed");
    assert_eq!(file_chunks, chunks);
    assert_eq!(from_file, from_reader);
}

#[test]
fn empty_file_hashes_empty_digests() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let path = dir.path().join("empty.bin");
    fs::write(&path, b"").expect("file must be written");
    let options = HashOptions::new(vec![AlgorithmId::Blake3, AlgorithmId::Md5]);
    let hashes = compute_hashes_file(&path, &options).expect("empty file hashing must succeed");
    assert_eq!(
        hashes.get(&AlgorithmId::Blake3).map(String::as_str),
        Some("af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")
    );
    assert_eq!(
        hashes.get(&AlgorithmId::Md5).map(String::as_str),
        Some("d41d8cd98f00b204e9800998ecf8427e")
    );
}
