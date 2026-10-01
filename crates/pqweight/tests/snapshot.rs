//! The snapshot parser against the snapshot Fixture: a regtest `dumptxoutset`
//! file whose Oracle is Bitcoin Core's `gettxoutsetinfo` totals and `gettxout`
//! for every coin (recorded by `scripts/record-snapshot-fixture.ps1`).

mod common;

use std::fs::{self, File};

use common::{decode_hex, fixtures_dir};
use std::collections::HashMap;

use pqweight::{Coin, CoinScript, SnapshotError, read_snapshot};

fn oracle() -> serde_json::Value {
    let path = fixtures_dir().join("snapshot/regtest-utxo.json");
    let meta: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    meta["oracle"].clone()
}

fn snapshot_file() -> File {
    File::open(fixtures_dir().join("snapshot/regtest-utxo.dat")).unwrap()
}

/// A hash as Core displays it: the serialized bytes reversed, in hex.
fn display_hex(hash: &[u8; 32]) -> String {
    use std::fmt::Write;
    hash.iter().rev().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").expect("writing to a String never fails");
        hex
    })
}

#[test]
fn regtest_snapshot_header_matches_the_oracle() {
    let oracle = oracle();
    let snapshot = read_snapshot(snapshot_file()).unwrap();

    assert_eq!(snapshot.header.network_magic, [0xfa, 0xbf, 0xb5, 0xda]);
    assert_eq!(
        display_hex(&snapshot.header.base_block_hash),
        oracle["base_hash"].as_str().unwrap()
    );
    assert_eq!(
        snapshot.header.coins_count,
        oracle["coins_count"].as_u64().unwrap()
    );
}

/// What the Oracle's scriptPubKey must look like for `script`. Uncompressed P2PK
/// stores only the x-coordinate and y's parity (ADR 0003: no point
/// decompression), so for it the Oracle's key is checked against those two.
fn assert_script_matches(script: &CoinScript, oracle_script: &[u8], label: &str) {
    match script {
        CoinScript::P2pkh(hash) => {
            assert_eq!(
                oracle_script,
                [&[0x76, 0xa9, 0x14][..], hash, &[0x88, 0xac]].concat(),
                "{label}"
            );
        }
        CoinScript::P2sh(hash) => {
            assert_eq!(
                oracle_script,
                [&[0xa9, 0x14][..], hash, &[0x87]].concat(),
                "{label}"
            );
        }
        CoinScript::P2pkCompressed(key) => {
            assert_eq!(
                oracle_script,
                [&[0x21][..], key, &[0xac]].concat(),
                "{label}"
            );
        }
        CoinScript::P2pkUncompressed { x, odd_y } => {
            assert_eq!(oracle_script.len(), 67, "{label}");
            assert_eq!(oracle_script[..2], [0x41, 0x04], "{label}");
            assert_eq!(oracle_script[2..34], x[..], "{label}");
            assert_eq!(oracle_script[65] & 1 == 1, *odd_y, "{label}: y parity");
            assert_eq!(oracle_script[66], 0xac, "{label}");
        }
        CoinScript::Raw(bytes) => assert_eq!(oracle_script, &bytes[..], "{label}"),
    }
}

#[test]
fn every_coin_in_the_regtest_snapshot_matches_gettxout() {
    let oracle = oracle();
    let coins: Vec<Coin> = read_snapshot(snapshot_file())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(coins.len() as u64, oracle["coins_count"].as_u64().unwrap());
    assert_eq!(
        coins.iter().map(|coin| coin.value).sum::<u64>(),
        oracle["total_amount"].as_u64().unwrap()
    );

    let by_outpoint: HashMap<(String, u32), &Coin> = coins
        .iter()
        .map(|coin| ((display_hex(&coin.txid), coin.vout), coin))
        .collect();
    let oracle_coins = oracle["coins"].as_array().unwrap();
    assert_eq!(by_outpoint.len(), oracle_coins.len(), "duplicate outpoints");
    for expected in oracle_coins {
        let txid = expected["txid"].as_str().unwrap().to_string();
        let vout = u32::try_from(expected["vout"].as_u64().unwrap()).unwrap();
        let label = format!("{txid}:{vout}");
        let coin = by_outpoint
            .get(&(txid, vout))
            .unwrap_or_else(|| panic!("{label} missing from the snapshot"));
        assert_eq!(
            u64::from(coin.height),
            expected["height"].as_u64().unwrap(),
            "{label}"
        );
        assert_eq!(
            coin.coinbase,
            expected["coinbase"].as_bool().unwrap(),
            "{label}"
        );
        assert_eq!(coin.value, expected["value"].as_u64().unwrap(), "{label}");
        let oracle_script = decode_hex(expected["script"].as_str().unwrap());
        assert_script_matches(&coin.script, &oracle_script, &label);
    }
}

#[test]
fn regtest_snapshot_stores_each_compressed_script_form() {
    let coins: Vec<Coin> = read_snapshot(snapshot_file())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let has = |matches: fn(&CoinScript) -> bool| coins.iter().any(|coin| matches(&coin.script));

    assert!(has(|script| matches!(script, CoinScript::P2pkh(_))));
    assert!(has(|script| matches!(script, CoinScript::P2sh(_))));
    assert!(has(|script| matches!(
        script,
        CoinScript::P2pkCompressed(_)
    )));
    assert!(has(|script| matches!(
        script,
        CoinScript::P2pkUncompressed { odd_y: false, .. }
    )));
    assert!(has(|script| matches!(
        script,
        CoinScript::P2pkUncompressed { odd_y: true, .. }
    )));
    assert!(has(|script| matches!(script, CoinScript::Raw(_))));
}

fn snapshot_bytes() -> Vec<u8> {
    fs::read(fixtures_dir().join("snapshot/regtest-utxo.dat")).unwrap()
}

#[test]
fn file_without_the_snapshot_magic_is_rejected() {
    let mut bytes = snapshot_bytes();
    bytes[0] = b'x';
    assert!(matches!(
        read_snapshot(&bytes[..]),
        Err(SnapshotError::BadMagic)
    ));
}

#[test]
fn snapshot_version_other_than_2_is_rejected() {
    let mut bytes = snapshot_bytes();
    bytes[5] = 3;
    assert!(matches!(
        read_snapshot(&bytes[..]),
        Err(SnapshotError::UnsupportedVersion(3))
    ));
}

#[test]
fn header_cut_short_is_truncated() {
    let bytes = snapshot_bytes();
    assert!(matches!(
        read_snapshot(&bytes[..50]),
        Err(SnapshotError::Truncated { .. })
    ));
}

#[test]
fn body_cut_short_yields_coins_then_one_error_then_ends() {
    let bytes = snapshot_bytes();
    let cut = &bytes[..bytes.len() / 2];
    let results: Vec<_> = read_snapshot(cut).unwrap().collect();

    let (last, coins) = results.split_last().unwrap();
    assert!(!coins.is_empty());
    assert!(coins.iter().all(Result::is_ok));
    assert!(matches!(last, Err(SnapshotError::Truncated { .. })));
}

#[test]
fn every_prefix_of_the_snapshot_reads_without_panicking() {
    let bytes = snapshot_bytes();
    for len in 0..bytes.len() {
        if let Ok(snapshot) = read_snapshot(&bytes[..len]) {
            let results: Vec<_> = snapshot.collect();
            assert!(
                results.last().is_some_and(Result::is_err),
                "prefix of {len} bytes"
            );
        }
    }
}
