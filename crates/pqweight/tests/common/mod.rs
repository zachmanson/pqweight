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
