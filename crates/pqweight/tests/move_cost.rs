//! Move cost of Exposed coins (ticket 22). Expected weights are the worked
//! numbers in `docs/migration-templates.md`, "Move layouts".

mod common;

use std::fs::File;

use common::fixtures_dir;
use pqweight::{
    Coin, CoinScript, ExposedType, MoveCost, MoveCostTotals, MultisigThreshold, move_cost,
    read_snapshot,
};

fn coin(script: CoinScript, value: u64) -> Coin {
    Coin {
        txid: [0; 32],
        vout: 0,
        height: 1,
        coinbase: false,
        value,
        script,
    }
}

fn p2pk_compressed(value: u64) -> Coin {
    let mut key = [0x11; 33];
    key[0] = 0x02;
    coin(CoinScript::P2pkCompressed(key), value)
}

fn p2pk_uncompressed(value: u64) -> Coin {
    coin(
        CoinScript::P2pkUncompressed {
            x: [0x22; 32],
            odd_y: true,
        },
        value,
    )
}

fn p2tr(value: u64) -> Coin {
    let mut script = vec![0x51, 0x20];
    script.extend([0x33; 32]);
    coin(CoinScript::Raw(script), value)
}

/// `OP_m <n compressed keys> OP_n OP_CHECKMULTISIG`, m and n from 1 to 16.
fn bare_multisig(m: u8, n: u8, value: u64) -> Coin {
    let mut script = vec![0x50 + m];
    for _ in 0..n {
        script.push(33);
        script.push(0x02);
        script.extend([0x44; 32]);
    }
    script.extend([0x50 + n, 0xae]);
    coin(CoinScript::Raw(script), value)
}

fn totals(coins: u64, value: u64, floor_weight: u64, ceiling_weight: u64) -> MoveCostTotals {
    MoveCostTotals {
        coins,
        value,
        floor_weight,
        ceiling_weight,
    }
}

fn multisig(m: u8, n: u8) -> ExposedType {
    ExposedType::BareMultisig(MultisigThreshold { m, n })
}

/// The (all coins, coins of at least 546 sats) pair for `exposed_type`.
fn row(cost: &MoveCost, exposed_type: ExposedType) -> (MoveCostTotals, MoveCostTotals) {
    let row = cost
        .rows
        .iter()
        .find(|row| row.exposed_type == exposed_type)
        .unwrap_or_else(|| panic!("no {exposed_type:?} row"));
    (row.all, row.above_dust)
}

#[test]
fn p2pk_coin_moves_for_456_floor_and_668_ceiling_either_key_form() {
    let cost = move_cost(&[p2pk_compressed(100_000), p2pk_uncompressed(200_000)]);

    let one = |value| totals(1, value, 456, 668);
    assert_eq!(
        row(&cost, ExposedType::P2pkCompressed),
        (one(100_000), one(100_000))
    );
    assert_eq!(
        row(&cost, ExposedType::P2pkUncompressed),
        (one(200_000), one(200_000))
    );
}

#[test]
fn p2tr_coin_moves_for_230_floor_and_444_ceiling() {
    let cost = move_cost(&[p2tr(10_000)]);

    let one = totals(1, 10_000, 230, 444);
    assert_eq!(row(&cost, ExposedType::P2tr), (one, one));
}

#[test]
fn bare_multisig_costs_depend_on_m_with_one_row_per_threshold() {
    let cost = move_cost(&[
        bare_multisig(1, 1, 1_000),
        bare_multisig(1, 3, 1_000),
        bare_multisig(2, 3, 1_000),
        bare_multisig(3, 3, 1_000),
        bare_multisig(16, 16, 1_000),
    ]);

    let one = |floor, ceiling| totals(1, 1_000, floor, ceiling);
    assert_eq!(row(&cost, multisig(1, 1)).0, one(460, 672));
    assert_eq!(row(&cost, multisig(1, 3)).0, one(460, 672));
    assert_eq!(row(&cost, multisig(2, 3)).0, one(752, 964));
    assert_eq!(row(&cost, multisig(3, 3)).0, one(1_044, 1_256));
    // 1 + 73 x 16 = 1,169 scriptSig bytes need a 3-byte compact size.
    assert_eq!(row(&cost, multisig(16, 16)).0, one(4_848, 5_060));
}

#[test]
fn coins_of_one_type_add_up() {
    let cost = move_cost(&[p2pk_compressed(1_000), p2pk_compressed(2_000)]);

    assert_eq!(
        row(&cost, ExposedType::P2pkCompressed).0,
        totals(2, 3_000, 912, 1_336)
    );
}

#[test]
fn coins_below_546_sats_count_only_in_all() {
    let cost = move_cost(&[p2tr(545), p2tr(546)]);

    assert_eq!(
        row(&cost, ExposedType::P2tr),
        (totals(2, 1_091, 460, 888), totals(1, 546, 230, 444))
    );
}

#[test]
fn coins_that_are_not_exposed_have_no_row_but_count_as_scanned() {
    let mut invalid_p2pk = vec![0x41, 0x04];
    invalid_p2pk.extend([0; 64]);
    invalid_p2pk.push(0xac);
    let mut p2wpkh = vec![0x00, 0x14];
    p2wpkh.extend([0x55; 20]);
    let coins = [
        coin(CoinScript::P2pkh([0; 20]), 1),
        coin(CoinScript::P2sh([0; 20]), 2),
        coin(CoinScript::Raw(p2wpkh), 3),
        coin(CoinScript::Raw(invalid_p2pk), 4),
        p2tr(5),
    ];

    let cost = move_cost(&coins);

    assert_eq!(cost.rows.len(), 1, "{:?}", cost.rows);
    assert_eq!(cost.scanned_coins, 5);
    assert_eq!(cost.scanned_value, 15);
}

#[test]
fn total_sums_every_row() {
    let cost = move_cost(&[
        p2pk_compressed(1_000),
        bare_multisig(1, 2, 500),
        p2tr(2_000),
    ]);

    let (all, above_dust) = cost.total();
    assert_eq!(all, totals(3, 3_500, 456 + 460 + 230, 668 + 672 + 444));
    assert_eq!(above_dust, totals(2, 3_000, 456 + 230, 668 + 444));
}

#[test]
fn blocks_are_weight_over_four_million() {
    let six_million = totals(1, 0, 6_000_000, 10_000_000);

    assert!((six_million.floor_blocks() - 1.5).abs() < f64::EPSILON);
    assert!((six_million.ceiling_blocks() - 2.5).abs() < f64::EPSILON);
}

#[test]
fn rows_come_in_a_fixed_order() {
    let cost = move_cost(&[
        p2tr(1_000),
        bare_multisig(2, 3, 1_000),
        bare_multisig(1, 3, 1_000),
        p2pk_uncompressed(1_000),
        p2pk_compressed(1_000),
    ]);

    let order: Vec<ExposedType> = cost.rows.iter().map(|row| row.exposed_type).collect();
    assert_eq!(
        order,
        [
            ExposedType::P2pkCompressed,
            ExposedType::P2pkUncompressed,
            multisig(1, 3),
            multisig(2, 3),
            ExposedType::P2tr,
        ]
    );
}

/// The snapshot Fixture's Exposed coins, counted by hand from the Oracle's
/// scriptPubKeys in `regtest-utxo.json`.
#[test]
fn regtest_snapshot_move_cost() {
    let file = File::open(fixtures_dir().join("snapshot/regtest-utxo.dat")).unwrap();
    let coins: Vec<Coin> = read_snapshot(file)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let cost = move_cost(&coins);

    let same = |t: MoveCostTotals| (t, t);
    assert_eq!(
        row(&cost, ExposedType::P2pkCompressed),
        same(totals(1, 100_000_000, 456, 668))
    );
    assert_eq!(
        row(&cost, ExposedType::P2pkUncompressed),
        same(totals(2, 12_345_678 + 5_000, 912, 1_336))
    );
    assert_eq!(row(&cost, multisig(1, 1)), same(totals(1, 7_000, 460, 672)));
    assert_eq!(row(&cost, multisig(1, 2)), same(totals(1, 8_000, 460, 672)));
    assert_eq!(row(&cost, multisig(2, 2)), same(totals(1, 9_000, 752, 964)));
    assert_eq!(
        row(&cost, multisig(2, 3)),
        same(totals(1, 10_000, 752, 964))
    );
    assert_eq!(
        row(&cost, multisig(3, 3)),
        same(totals(1, 11_000, 1_044, 1_256))
    );
    assert_eq!(
        row(&cost, ExposedType::P2tr),
        (
            totals(3, 2_100_000_000 + 546 + 545, 690, 1_332),
            totals(2, 2_100_000_000 + 546, 460, 888)
        )
    );
    assert_eq!(cost.rows.len(), 8);
    assert_eq!(cost.scanned_coins, 118);
}
