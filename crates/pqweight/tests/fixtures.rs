//! Every committed Fixture must measure the same in three independent ways: our
//! parser, `rust-bitcoin` (dev-dependency only) and the Oracle (Bitcoin Core).
//!
//! A Fixture is `<name>.hex` (raw transaction) plus `<name>.json` (the Oracle's
//! weight and vsize, recorded by `scripts/record-fixtures.ps1`). Adding a Fixture
//! means dropping two files into `tests/fixtures/`; no test code changes.

mod common;

use std::fs;

use common::{decode_hex, fixture_hex_paths};
use pqweight::transaction_weight;

#[test]
fn every_fixture_matches_the_oracle() {
    let hex_files = fixture_hex_paths();

    let mut failures = Vec::new();
    for hex_path in &hex_files {
        let name = hex_path.file_stem().unwrap().to_string_lossy();
        let bytes = decode_hex(fs::read_to_string(hex_path).unwrap().trim());
        let oracle: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(hex_path.with_extension("json")).unwrap())
                .unwrap();
        let oracle_weight = oracle["oracle"]["weight"].as_u64().expect("oracle weight");
        let oracle_vsize = oracle["oracle"]["vsize"].as_u64().expect("oracle vsize");

        let ours = match transaction_weight(&bytes) {
            Ok(ours) => (ours.weight, ours.vsize),
            Err(err) => {
                failures.push(format!("{name}: our parser failed: {err:?}"));
                continue;
            }
        };
        let rust_bitcoin = match bitcoin::consensus::deserialize::<bitcoin::Transaction>(&bytes) {
            Ok(tx) => (tx.weight().to_wu(), tx.vsize() as u64),
            Err(err) => {
                failures.push(format!("{name}: rust-bitcoin failed to parse: {err}"));
                continue;
            }
        };
        let oracle = (oracle_weight, oracle_vsize);

        if ours != oracle || rust_bitcoin != oracle {
            failures.push(format!(
                "{name}: (weight, vsize) ours={ours:?} rust-bitcoin={rust_bitcoin:?} oracle={oracle:?}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} fixtures disagree across ours, rust-bitcoin and the Oracle:\n{}",
        failures.len(),
        hex_files.len(),
        failures.join("\n")
    );
}
