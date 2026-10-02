//! `aggregate_blocks`: whole blocks in, one line each, verified by
//! `parse_block`, summed like `aggregate` and also reported per block.

mod common;

use common::block_fixture;
use pqweight::{BlockRow, ParameterSet, aggregate, aggregate_blocks};

fn block_line(name: &str) -> String {
    block_fixture(name).hex
}

fn oracle(name: &str) -> serde_json::Value {
    block_fixture(name).oracle
}

#[test]
fn each_block_gets_a_row_with_its_hash_count_and_block_weight() {
    let lines = vec![block_line("regtest-block"), block_line("block-170")];

    let result = aggregate_blocks(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.blocks.len(), 2);
    let regtest = &result.blocks[0];
    assert_eq!(regtest.hash.to_string(), oracle("regtest-block")["hash"]);
    assert_eq!(regtest.transactions, 3);
    assert_eq!(regtest.weight, oracle("regtest-block")["weight"]);
    assert_eq!(regtest.partially_mapped, 0);
    // Block 170: a coinbase and a P2PK spend, which is Unmapped. Weight 1960
    // as mempool.space reports (and block.rs checks against rust-bitcoin).
    let block_170: &BlockRow = &result.blocks[1];
    assert_eq!(block_170.hash.to_string(), oracle("block-170")["hash"]);
    assert_eq!(block_170.transactions, 2);
    assert_eq!(block_170.weight, 1960);
    assert_eq!(block_170.partially_mapped, 1);
    assert_eq!(block_170.migrated_weight, None);
}

#[test]
fn migrated_block_weight_is_the_header_and_count_plus_migrated_transaction_weights() {
    let result = aggregate_blocks(
        vec![block_line("regtest-block")].into_iter(),
        ParameterSet::MlDsa44,
        None,
    );

    // One block, every transaction fully mapped: the migrated total is the sum
    // of their migrated weights. The header is 80 bytes and the count of 3 is
    // one byte, all non-witness, so 4 x 81 WU.
    let migrated_transactions = result.migrated.unwrap().weight;
    assert_eq!(
        result.blocks[0].migrated_weight,
        Some(4 * 81 + migrated_transactions)
    );
}

/// The block's transactions as `rust-bitcoin` splits them, one hex per line:
/// the input `aggregate` takes.
fn transaction_lines(block_hex: &str) -> Vec<String> {
    let bytes = common::decode_hex(block_hex);
    let block: bitcoin::Block = bitcoin::consensus::deserialize(&bytes).unwrap();
    block
        .txdata
        .iter()
        .map(bitcoin::consensus::encode::serialize_hex)
        .collect()
}

#[test]
fn totals_are_the_same_as_aggregating_the_transactions_one_per_line() {
    let blocks = vec![block_line("regtest-block"), block_line("block-170")];
    let transactions: Vec<String> = blocks.iter().flat_map(|b| transaction_lines(b)).collect();
    let rate = Some(pqweight::FeeRate::parse("2.5").unwrap());

    let mut by_block = aggregate_blocks(blocks.into_iter(), ParameterSet::MlDsa44, rate);
    let by_transaction = aggregate(transactions.into_iter(), ParameterSet::MlDsa44, rate);

    assert_eq!(by_block.blocks.len(), 2);
    by_block.blocks.clear();
    assert_eq!(by_block, by_transaction);
}

#[test]
fn a_block_that_fails_verification_is_recorded_and_dropped_whole() {
    let mut corrupted = block_line("regtest-block");
    // Flip the last hex digit: the last transaction's locktime, so the merkle
    // root no longer matches.
    let last = corrupted.pop().unwrap();
    corrupted.push(if last == '0' { '1' } else { '0' });
    let lines = vec![corrupted, String::new(), block_line("block-170")];

    let result = aggregate_blocks(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].line, 1);
    assert!(
        result.errors[0].message.contains("merkle root mismatch"),
        "{}",
        result.errors[0].message
    );
    assert_eq!(result.counts.parse_errors, 1);
    // Only block 170's two transactions are counted.
    assert_eq!(result.counts.parsed, 2);
    assert_eq!(result.blocks.len(), 1);
    assert_eq!(
        result.blocks[0].hash.to_string(),
        oracle("block-170")["hash"]
    );
}
