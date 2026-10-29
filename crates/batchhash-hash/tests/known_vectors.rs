//! Known-answer tests: NIST and reference digest vectors for every algorithm.

use std::io::Cursor;

use batchhash_hash::{compute_hashes, HashOptions};
use batchhash_types::AlgorithmId;

fn digest(algorithm: AlgorithmId, input: &[u8]) -> String {
    let options = HashOptions::new(vec![algorithm]);
    let mut reader = Cursor::new(input.to_vec());
    let hashes = compute_hashes(&mut reader, &options).expect("hashing must succeed");
    hashes
        .get(&algorithm)
        .cloned()
        .expect("selected algorithm must be present")
}

#[test]
fn nist_and_reference_digests_match() {
    const CASES: &[(AlgorithmId, &str, &str)] = &[
        (AlgorithmId::Md5, "", "d41d8cd98f00b204e9800998ecf8427e"),
        (AlgorithmId::Md5, "abc", "900150983cd24fb0d6963f7d28e17f72"),
        (AlgorithmId::Sha1, "", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
        (AlgorithmId::Sha1, "abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
        (AlgorithmId::Sha224, "", "d14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f"),
        (AlgorithmId::Sha224, "abc", "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7"),
        (AlgorithmId::Sha256, "", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (AlgorithmId::Sha256, "abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (AlgorithmId::Sha384, "", "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"),
        (AlgorithmId::Sha384, "abc", "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"),
        (AlgorithmId::Sha512, "", "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"),
        (AlgorithmId::Sha512, "abc", "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"),
        (AlgorithmId::Sha3_256, "", "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"),
        (AlgorithmId::Sha3_256, "abc", "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"),
        (AlgorithmId::Sha3_512, "", "a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26"),
        (AlgorithmId::Sha3_512, "abc", "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0"),
        (AlgorithmId::Blake2b, "", "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"),
        (AlgorithmId::Blake2b, "abc", "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"),
        (AlgorithmId::Blake2s, "", "69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9"),
        (AlgorithmId::Blake2s, "abc", "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982"),
        (AlgorithmId::Blake3, "", "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"),
        (AlgorithmId::Blake3, "abc", "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"),
        (AlgorithmId::Xxh3_64, "", "2d06800538d394c2"),
        (AlgorithmId::Xxh3_64, "abc", "78af5f94892f3950"),
        (AlgorithmId::Xxh3_128, "", "99aa06d3014798d86001c324468d497f"),
        (AlgorithmId::Xxh3_128, "abc", "06b05ab6733a618578af5f94892f3950"),
        (AlgorithmId::Xxh64, "", "ef46db3751d8e999"),
        (AlgorithmId::Xxh64, "abc", "44bc2cf5ad770999"),
        (AlgorithmId::Crc32, "", "00000000"),
        (AlgorithmId::Crc32, "abc", "352441c2"),
        (AlgorithmId::Crc32, "123456789", "cbf43926"),
        (AlgorithmId::Crc32c, "", "00000000"),
        (AlgorithmId::Crc32c, "abc", "364b3fb7"),
        (AlgorithmId::Crc32c, "Hello world!", "7b98e751"),
        (AlgorithmId::Crc32c, "123456789", "e3069283"),
    ];
    for (algorithm, input, expected) in CASES {
        let actual = digest(*algorithm, input.as_bytes());
        assert_eq!(actual, *expected, "{algorithm} digest for {input:?}");
        assert_eq!(
            actual.len(),
            algorithm.hex_len(),
            "{algorithm} digest length"
        );
        assert!(
            actual
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "{algorithm} digest must be lowercase hex: {actual}"
        );
    }
}

const PRIME32: u64 = 2_654_435_761;
const PRIME64: u64 = 11_400_714_785_074_694_797;

fn sanity_buffer(len: usize) -> Vec<u8> {
    let mut byte_gen = PRIME32;
    let mut buffer = Vec::with_capacity(len);
    for _ in 0..len {
        buffer.push((byte_gen >> 56) as u8);
        byte_gen = byte_gen.wrapping_mul(PRIME64);
    }
    buffer
}

#[test]
fn xxhash_reference_sanity_vectors() {
    const XXH64_CASES: &[(usize, &str)] = &[
        (0, "ef46db3751d8e999"),
        (1, "e934a84adb052768"),
        (4, "9136a0dca57457ee"),
        (14, "8282dcc4994e35c8"),
        (222, "b641ae8cb691c174"),
    ];
    for (len, expected) in XXH64_CASES {
        assert_eq!(
            digest(AlgorithmId::Xxh64, &sanity_buffer(*len)),
            *expected,
            "xxh64 sanity vector for {len} bytes"
        );
    }

    const XXH3_64_CASES: &[(usize, &str)] = &[
        (0, "2d06800538d394c2"),
        (1, "c44bdff4074eecdb"),
        (6, "27b56a84cd2d7325"),
        (12, "a713daf0dfbb77e7"),
        (24, "a3fe70bf9d3510eb"),
        (48, "397da259ecba1f11"),
        (80, "bcdefbbb2c47c90a"),
        (195, "cd94217ee362ec3a"),
        (403, "cdeb804d65c6dea4"),
        (512, "617e49599013cb6b"),
        (2048, "dd59e2c3a5f038e0"),
        (2240, "6e73a90539cf2948"),
        (2367, "cb37aeb9e5d361ed"),
    ];
    for (len, expected) in XXH3_64_CASES {
        assert_eq!(
            digest(AlgorithmId::Xxh3_64, &sanity_buffer(*len)),
            *expected,
            "xxh3-64 sanity vector for {len} bytes"
        );
    }

    const XXH3_128_CASES: &[(usize, &str, &str)] = &[
        (0, "6001c324468d497f", "99aa06d3014798d8"),
        (1, "c44bdff4074eecdb", "a6cd5e9392000f6a"),
        (6, "3e7039bdda43cfc6", "082afe0b8162d12a"),
        (12, "061a192713f69ad9", "6e3efd8fc7802b18"),
        (24, "1e7044d28b1b901d", "0ce966e4678d3761"),
        (48, "f942219aed80f67b", "a002ac4e5478227e"),
        (81, "5e8bafb9f95fb803", "4952f58181ab0042"),
        (222, "f1aebd597cec6b3a", "337e09641b948717"),
        (403, "cdeb804d65c6dea4", "1b6de21e332dd73d"),
        (512, "617e49599013cb6b", "18d2d110dcc9bca1"),
        (2048, "dd59e2c3a5f038e0", "f736557fd47073a5"),
        (2240, "6e73a90539cf2948", "ccb134fbfa7ce49d"),
        (2367, "cb37aeb9e5d361ed", "e89c0f6ff369b427"),
    ];
    for (len, low, high) in XXH3_128_CASES {
        let expected = format!("{high}{low}");
        assert_eq!(
            digest(AlgorithmId::Xxh3_128, &sanity_buffer(*len)),
            expected,
            "xxh3-128 sanity vector for {len} bytes"
        );
    }
}
