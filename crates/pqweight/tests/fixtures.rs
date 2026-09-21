//! Every committed Fixture must measure the same as the Oracle (Bitcoin Core).
//!
//! A Fixture is `<name>.hex` (raw transaction) plus `<name>.json` (the Oracle's
//! weight and vsize, recorded by `scripts/record-fixtures.ps1`). Adding a Fixture
//! means dropping two files into `tests/fixtures/`; no test code changes.

use std::fs;
use std::path::Path;

use pqweight::transaction_weight;

fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "odd-length hex");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex digit"))
        .collect()
}

#[test]
fn every_fixture_matches_the_oracle() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut hex_files: Vec<_> = fs::read_dir(&dir)
        .expect("fixtures directory exists")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hex"))
        .collect();
    hex_files.sort();
    assert!(
        !hex_files.is_empty(),
        "no fixtures found in {}",
        dir.display()
    );

    let mut failures = Vec::new();
    for hex_path in &hex_files {
        let name = hex_path.file_stem().unwrap().to_string_lossy();
        let bytes = decode_hex(fs::read_to_string(hex_path).unwrap().trim());
        let oracle: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(hex_path.with_extension("json")).unwrap())
                .unwrap();
        let oracle_weight = oracle["oracle"]["weight"].as_u64().expect("oracle weight");
        let oracle_vsize = oracle["oracle"]["vsize"].as_u64().expect("oracle vsize");

        match transaction_weight(&bytes) {
            Ok(ours) if ours.weight == oracle_weight && ours.vsize == oracle_vsize => {}
            Ok(ours) => failures.push(format!(
                "{name}: weight ours={} oracle={}, vsize ours={} oracle={}",
                ours.weight, oracle_weight, ours.vsize, oracle_vsize
            )),
            Err(err) => failures.push(format!("{name}: our parser failed: {err:?}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} fixtures disagree with the Oracle:\n{}",
        failures.len(),
        hex_files.len(),
        failures.join("\n")
    );
}
