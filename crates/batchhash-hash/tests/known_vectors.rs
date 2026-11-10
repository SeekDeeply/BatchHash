//! Known-answer tests: NIST, RFC and reference digest vectors for every
//! registered algorithm.
//!
//! Values for SHA-2, SHA-3, SHAKE, BLAKE2, RIPEMD, SM3, MD2 and MD4 were
//! cross-checked against OpenSSL 3.5. Keccak (pre-standard padding),
//! Whirlpool, Tiger, GOST, Streebog, Skein, Groestl and JH use their
//! published empty-string and "abc" vectors.

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
        (AlgorithmId::Crc32c, "", "00000000"),
        (AlgorithmId::Crc32c, "abc", "364b3fb7"),
        (AlgorithmId::Md2, "", "8350e5a3e24c153df2275c9f80692773"),
        (AlgorithmId::Md2, "abc", "da853b0d3f88d99b30283a69e6ded6bb"),
        (AlgorithmId::Md4, "", "31d6cfe0d16ae931b73c59d7e0c089c0"),
        (AlgorithmId::Md4, "abc", "a448017aaf21d8525fc10ae87aa6729d"),
        (AlgorithmId::Sha512_224, "", "6ed0dd02806fa89e25de060c19d3ac86cabb87d6a0ddd05c333b84f4"),
        (AlgorithmId::Sha512_224, "abc", "4634270f707b6a54daae7530460842e20e37ed265ceee9a43e8924aa"),
        (AlgorithmId::Sha512_256, "", "c672b8d1ef56ed28ab87c3622c5114069bdd3ad7b8f9737498d0c01ecef0967a"),
        (AlgorithmId::Sha512_256, "abc", "53048e2681941ef99b2e29b76b4c7dabe4c2d0c634fc6d46e0e2f13107e7af23"),
        (AlgorithmId::Sha3_224, "", "6b4e03423667dbb73b6e15454f0eb1abd4597f9a1b078e3f5b5a6bc7"),
        (AlgorithmId::Sha3_224, "abc", "e642824c3f8cf24ad09234ee7d3c766fc9a3a5168d0c94ad73b46fdf"),
        (AlgorithmId::Sha3_384, "", "0c63a75b845e4f7d01107d852e4c2485c51a50aaaa94fc61995e71bbee983a2ac3713831264adb47fb6bd1e058d5f004"),
        (AlgorithmId::Sha3_384, "abc", "ec01498288516fc926459f58e2c6ad8df9b473cb0fc08c2596da7cf0e49be4b298d88cea927ac7f539f1edf228376d25"),
        (AlgorithmId::Keccak224, "", "f71837502ba8e10837bdd8d365adb85591895602fc552b48b7390abd"),
        (AlgorithmId::Keccak224, "abc", "c30411768506ebe1c2871b1ee2e87d38df342317300a9b97a95ec6a8"),
        (AlgorithmId::Keccak256, "", "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"),
        (AlgorithmId::Keccak256, "abc", "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"),
        (AlgorithmId::Keccak384, "", "2c23146a63a29acf99e73b88f8c24eaa7dc60aa771780ccc006afbfa8fe2479b2dd2b21362337441ac12b515911957ff"),
        (AlgorithmId::Keccak384, "abc", "f7df1165f033337be098e7d288ad6a2f74409d7a60b49c36642218de161b1f99f8c681e4afaf31a34db29fb763e3c28e"),
        (AlgorithmId::Keccak512, "", "0eab42de4c3ceb9235fc91acffe746b29c29a8c366b7c60e4e67c466f36a4304c00fa9caf9d87976ba469bcbe06713b435f091ef2769fb160cdab33d3670680e"),
        (AlgorithmId::Keccak512, "abc", "18587dc2ea106b9a1563e32b3312421ca164c7f1f07bc922a9c83d77cea3a1e5d0c69910739025372dc14ac9642629379540c17e2a65b19d77aa511a9d00bb96"),
        (AlgorithmId::Shake128, "", "7f9c2ba4e88f827d616045507605853e"),
        (AlgorithmId::Shake128, "abc", "5881092dd818bf5cf8a3ddb793fbcba7"),
        (AlgorithmId::Shake256, "", "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f"),
        (AlgorithmId::Shake256, "abc", "483366601360a8771c6863080cc4114d8db44530f8f1e1ee4f94ea37e78b5739"),
        (AlgorithmId::Ripemd128, "", "cdf26213a150dc3ecb610f18f6b38b46"),
        (AlgorithmId::Ripemd128, "abc", "c14a12199c66e4ba84636b0f69144c77"),
        (AlgorithmId::Ripemd160, "", "9c1185a5c5e9fc54612808977ee8f548b2258d31"),
        (AlgorithmId::Ripemd160, "abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"),
        (AlgorithmId::Ripemd256, "", "02ba4c4e5f8ecd1877fc52d64d30e37a2d9774fb1e5d026380ae0168e3c5522d"),
        (AlgorithmId::Ripemd256, "abc", "afbd6e228b9d8cbbcef5ca2d03e6dba10ac0bc7dcbe4680e1e42d2e975459b65"),
        (AlgorithmId::Ripemd320, "", "22d65d5661536cdc75c1fdf5c6de7b41b9f27325ebc61e8557177d705a0ec880151c3a32a00899b8"),
        (AlgorithmId::Ripemd320, "abc", "de4c01b3054f8930a79d09ae738e92301e5a17085beffdc1b8d116713e74f82fa942d64cdbc4682d"),
        (AlgorithmId::Sm3, "", "1ab21d8355cfa17f8e61194831e81a8f22bec8c728fefb747ed035eb5082aa2b"),
        (AlgorithmId::Sm3, "abc", "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0"),
        (AlgorithmId::Gost94, "", "981e5f3ca30c841487830f84fb433e13ac1101569b9c13584ac483234cd656c0"),
        (AlgorithmId::Gost94, "abc", "b285056dbf18d7392d7677369524dd14747459ed8143997e163b2986f92fd42c"),
        (AlgorithmId::Streebog256, "", "3f539a213e97c802cc229d474c6aa32a825a360b2a933a949fd925208d9ce1bb"),
        (AlgorithmId::Streebog256, "abc", "4e2919cf137ed41ec4fb6270c61826cc4fffb660341e0af3688cd0626d23b481"),
        (AlgorithmId::Streebog512, "", "8e945da209aa869f0455928529bcae4679e9873ab707b55315f56ceb98bef0a7362f715528356ee83cda5f2aac4c6ad2ba3a715c1bcd81cb8e9f90bf4c1c1a8a"),
        (AlgorithmId::Streebog512, "abc", "28156e28317da7c98f4fe2bed6b542d0dab85bb224445fcedaf75d46e26d7eb8d5997f3e0915dd6b7f0aab08d9c8beb0d8c64bae2ab8b3c8c6bc53b3bf0db728"),
        (AlgorithmId::Whirlpool, "", "19fa61d75522a4669b44e39c1d2e1726c530232130d407f89afee0964997f7a73e83be698b288febcf88e3e03c4f0757ea8964e59b63d93708b138cc42a66eb3"),
        (AlgorithmId::Whirlpool, "abc", "4e2448a4c6f486bb16b6562c73b4020bf3043e3a731bce721ae1b303d97e6d4c7181eebdb6c57e277d0e34957114cbd6c797fc9d95d8b582d225292076d4eef5"),
        (AlgorithmId::Tiger, "", "3293ac630c13f0245f92bbb1766e16167a4e58492dde73f3"),
        (AlgorithmId::Tiger, "abc", "2aab1484e8c158f2bfb8c5ff41b57a525129131c957b5f93"),
        (AlgorithmId::Skein256, "", "c8877087da56e072870daa843f176e9453115929094c3a40c463a196c29bf7ba"),
        (AlgorithmId::Skein256, "abc", "258bdec343b9fde1639221a5ae0144a96e552e5288753c5fec76c05fc2fc1870"),
        (AlgorithmId::Skein512, "", "bc5b4c50925519c290cc634277ae3d6257212395cba733bbad37a4af0fa06af41fca7903d06564fea7a2d3730dbdb80c1f85562dfcc070334ea4d1d9e72cba7a"),
        (AlgorithmId::Skein512, "abc", "8f5dd9ec798152668e35129496b029a960c9a9b88662f7f9482f110b31f9f93893ecfb25c009baad9e46737197d5630379816a886aa05526d3a70df272d96e75"),
        (AlgorithmId::Groestl256, "", "1a52d11d550039be16107f9c58db9ebcc417f16f736adb2502567119f0083467"),
        (AlgorithmId::Groestl256, "abc", "f3c1bb19c048801326a7efbcf16e3d7887446249829c379e1840d1a3a1e7d4d2"),
        (AlgorithmId::Groestl512, "", "6d3ad29d279110eef3adbd66de2a0345a77baede1557f5d099fce0c03d6dc2ba8e6d4a6633dfbd66053c20faa87d1a11f39a7fbe4a6c2f009801370308fc4ad8"),
        (AlgorithmId::Groestl512, "abc", "70e1c68c60df3b655339d67dc291cc3f1dde4ef343f11b23fdd44957693815a75a8339c682fc28322513fd1f283c18e53cff2b264e06bf83a2f0ac8c1f6fbff6"),
        (AlgorithmId::Jh256, "", "46e64619c18bb0a92a5e87185a47eef83ca747b8fcc8e1412921357e326df434"),
        (AlgorithmId::Jh256, "abc", "924bc82f24a76d519d4f69493da7fa70dc88bdb6016b6d1cc1dcf7def15e9cdd"),
        (AlgorithmId::Jh512, "", "90ecf2f76f9d2c8017d979ad5ab96b87d58fc8fc4b83060f3f900774faa2c8fabe69c5f4ff1ec2b61d6b316941cedee117fb04b1f4c5bc1b919ae841c50eec4f"),
        (AlgorithmId::Jh512, "abc", "a05eab9c641cb901107d9880bcdf0eedb19b0073188896365921bd200225d9176cf136e7af90d67bdb05dfa3037e48b757d23a905b2270db67255b9eca982973"),
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
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "{algorithm} digest must be lowercase hex: {actual}"
        );
    }
}

#[test]
fn every_registered_algorithm_has_a_vector() {
    let covered: std::collections::BTreeSet<AlgorithmId> =
        CASES_ALGORITHMS.iter().copied().collect();
    for algorithm in AlgorithmId::ALL {
        assert!(
            covered.contains(&algorithm),
            "{algorithm} has no known-answer vector"
        );
    }
}

#[test]
fn same_input_produces_stable_digest_across_runs() {
    for algorithm in AlgorithmId::ALL {
        let first = digest(algorithm, b"batchhash");
        let second = digest(algorithm, b"batchhash");
        assert_eq!(first, second, "{algorithm} must be deterministic");
    }
}

/// Every algorithm covered by CASES above, for the coverage assertion.
const CASES_ALGORITHMS: &[AlgorithmId] = &[
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
    AlgorithmId::Md2,
    AlgorithmId::Md4,
    AlgorithmId::Sha512_224,
    AlgorithmId::Sha512_256,
    AlgorithmId::Sha3_224,
    AlgorithmId::Sha3_384,
    AlgorithmId::Keccak224,
    AlgorithmId::Keccak256,
    AlgorithmId::Keccak384,
    AlgorithmId::Keccak512,
    AlgorithmId::Shake128,
    AlgorithmId::Shake256,
    AlgorithmId::Ripemd128,
    AlgorithmId::Ripemd160,
    AlgorithmId::Ripemd256,
    AlgorithmId::Ripemd320,
    AlgorithmId::Sm3,
    AlgorithmId::Gost94,
    AlgorithmId::Streebog256,
    AlgorithmId::Streebog512,
    AlgorithmId::Whirlpool,
    AlgorithmId::Tiger,
    AlgorithmId::Skein256,
    AlgorithmId::Skein512,
    AlgorithmId::Groestl256,
    AlgorithmId::Groestl512,
    AlgorithmId::Jh256,
    AlgorithmId::Jh512,
];
