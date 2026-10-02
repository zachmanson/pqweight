//! Helpers shared by the integration tests.

// Each test binary compiles this module separately and uses only some helpers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "odd-length hex");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex digit"))
        .collect()
}

/// A block Fixture from `fixtures/block/`: one raw block and its Oracle values.
pub struct BlockFixture {
    pub hex: String,
    pub bytes: Vec<u8>,
    pub oracle: serde_json::Value,
}

/// Names of every block Fixture (`fixtures/block/<name>.hex`), sorted, so a
/// newly recorded block is picked up without editing a list.
pub fn block_fixture_names() -> Vec<String> {
    let dir = fixtures_dir().join("block");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("block fixtures directory exists")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hex"))
        .map(|path| {
            path.file_stem()
                .expect("fixture has a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no block fixtures in {}", dir.display());
    names
}

pub fn block_fixture(name: &str) -> BlockFixture {
    let dir = fixtures_dir().join("block");
    let hex = std::fs::read_to_string(dir.join(format!("{name}.hex")))
        .expect("block fixture exists")
        .trim()
        .to_string();
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("{name}.json"))).expect("oracle exists"),
    )
    .expect("valid JSON");
    BlockFixture {
        bytes: decode_hex(&hex),
        hex,
        oracle: meta["oracle"].clone(),
    }
}

/// Paths of every `<name>.hex` Fixture, sorted so failures are reproducible.
pub fn fixture_hex_paths() -> Vec<PathBuf> {
    let dir = fixtures_dir();
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures directory exists")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hex"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures found in {}", dir.display());
    paths
}

/// A stand-in ECDSA signature of `len` bytes with BIP-66's DER structure:
/// `30 <len> 02 <r len> <r> 02 <s len> <s>` and a sighash byte, the 7 bytes
/// of framing leaving `len - 7` for `r` and `s`, split as evenly as possible.
/// Below 9 bytes `s` is empty and above 73 the whole is too long, so neither
/// is a signature.
pub fn der_signature(len: usize) -> Vec<u8> {
    let body = len.checked_sub(7).expect("at least 7 bytes of framing");
    let s_len = body / 2;
    let r_len = body - s_len;
    let byte = |n: usize| u8::try_from(n).expect("DER length fits a byte");
    let mut signature = vec![0x30, byte(4 + body), 0x02, byte(r_len)];
    signature.extend(std::iter::repeat_n(1u8, r_len));
    signature.extend_from_slice(&[0x02, byte(s_len)]);
    signature.extend(std::iter::repeat_n(1u8, s_len));
    signature.push(0x01);
    assert_eq!(signature.len(), len);
    signature
}
