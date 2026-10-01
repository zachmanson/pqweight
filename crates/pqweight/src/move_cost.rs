//! **Move cost** of **Exposed coins**: the weight of spending unspent coins
//! with today's signatures into PQ outputs (ticket 22). The layouts are the
//! stated assumptions in `docs/migration-templates.md`, "Move layouts".

use crate::migration::{MultisigThreshold, bare_multisig_threshold};
use crate::snapshot::{Coin, CoinScript};

/// Consensus limit on a block's weight, the unit "blocks" are counted in.
const MAX_BLOCK_WEIGHT: u64 = 4_000_000;

/// Coins below this many sats are left out of the `above_dust` totals:
/// inscription postage and data outputs nobody will pay to move.
const DUST_LIMIT: u64 = 546;

/// Outpoint (36) and sequence (4), which every input has.
const OUTPOINT_AND_SEQUENCE: u64 = 40;

/// A 72-byte DER signature (sighash byte included) and its 1-byte push opcode.
const DER_SIGNATURE_PUSH: u64 = 73;

/// Version, input count, output count and locktime of a 1-in, 1-out
/// transaction, at 4 WU per byte.
const TRANSACTION_OVERHEAD_WEIGHT: u64 = 4 * (4 + 1 + 1 + 4);

/// A BIP-360 style PQ output: 8-byte value, 1-byte length, 34-byte script.
const PQ_OUTPUT_WEIGHT: u64 = 4 * (8 + 1 + 34);

/// The segwit marker and flag, at 1 WU each.
const SEGWIT_MARKER_AND_FLAG_WEIGHT: u64 = 2;

/// The kind of **Exposed coin**: an unspent output whose public key sits in
/// the output itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExposedType {
    P2pkCompressed,
    P2pkUncompressed,
    BareMultisig(MultisigThreshold),
    P2tr,
}

impl ExposedType {
    /// The kind of Exposed coin `script` locks, or `None` if it is not one.
    fn of(script: &CoinScript) -> Option<Self> {
        match script {
            CoinScript::P2pkCompressed(_) => Some(Self::P2pkCompressed),
            CoinScript::P2pkUncompressed { .. } => Some(Self::P2pkUncompressed),
            CoinScript::Raw(script) if is_p2tr(script) => Some(Self::P2tr),
            CoinScript::Raw(script) => bare_multisig_threshold(script).map(Self::BareMultisig),
            CoinScript::P2pkh(_) | CoinScript::P2sh(_) => None,
        }
    }

    /// Weight of the input that spends this coin with today's signature.
    fn spend_weight(self) -> u64 {
        match self {
            Self::P2pkCompressed | Self::P2pkUncompressed => {
                4 * (OUTPOINT_AND_SEQUENCE + 1 + DER_SIGNATURE_PUSH)
            }
            Self::BareMultisig(MultisigThreshold { m, .. }) => {
                // OP_0, then m signature pushes.
                let script_sig = 1 + u64::from(m) * DER_SIGNATURE_PUSH;
                4 * (OUTPOINT_AND_SEQUENCE + compact_size_len(script_sig) + script_sig)
            }
            // Empty scriptSig; witness: 1 item of 64 bytes.
            Self::P2tr => 4 * (OUTPOINT_AND_SEQUENCE + 1) + (1 + 1 + 64),
        }
    }

    /// Weight of a transaction moving just this coin into one PQ output.
    fn ceiling_weight(self) -> u64 {
        let marker_and_flag = match self {
            Self::P2tr => SEGWIT_MARKER_AND_FLAG_WEIGHT,
            _ => 0,
        };
        self.spend_weight() + TRANSACTION_OVERHEAD_WEIGHT + PQ_OUTPUT_WEIGHT + marker_and_flag
    }
}

/// `51 20 <32 bytes>`: a segwit v1 output with a 32-byte program.
fn is_p2tr(script: &[u8]) -> bool {
    script.len() == 34 && script[0] == 0x51 && script[1] == 0x20
}

fn compact_size_len(value: u64) -> u64 {
    match value {
        0..=252 => 1,
        253..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

/// Coins, their value and the Move cost of a set of Exposed coins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MoveCostTotals {
    pub coins: u64,
    /// Value in satoshis.
    pub value: u64,
    /// Perfect consolidation: the sum of the coins' spend weights.
    pub floor_weight: u64,
    /// One coin per transaction.
    pub ceiling_weight: u64,
}

impl MoveCostTotals {
    /// Full blocks of moves the floor fills.
    #[must_use]
    pub fn floor_blocks(&self) -> f64 {
        blocks(self.floor_weight)
    }

    /// Full blocks of moves the ceiling fills.
    #[must_use]
    pub fn ceiling_blocks(&self) -> f64 {
        blocks(self.ceiling_weight)
    }

    fn add(&mut self, other: &Self) {
        self.coins += other.coins;
        self.value += other.value;
        self.floor_weight += other.floor_weight;
        self.ceiling_weight += other.ceiling_weight;
    }
}

fn blocks(weight: u64) -> f64 {
    // Display only: weights this large (over 2^52) never occur.
    #[allow(clippy::cast_precision_loss)]
    let ratio = weight as f64 / MAX_BLOCK_WEIGHT as f64;
    ratio
}

/// Move cost of one kind of Exposed coin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveCostRow {
    pub exposed_type: ExposedType,
    /// Every coin of this kind.
    pub all: MoveCostTotals,
    /// Only coins of at least 546 sats.
    pub above_dust: MoveCostTotals,
}

/// Move cost of every Exposed coin seen, one row per [`ExposedType`] present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveCost {
    /// In [`ExposedType`] order: P2PK compressed, P2PK uncompressed, bare
    /// multisig by threshold, P2TR.
    pub rows: Vec<MoveCostRow>,
    /// Every coin seen, Exposed or not.
    pub scanned_coins: u64,
    /// Value of every coin seen, in satoshis.
    pub scanned_value: u64,
}

impl MoveCost {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one coin, adding it to its row if it is an Exposed coin.
    pub fn add(&mut self, coin: &Coin) {
        self.scanned_coins += 1;
        self.scanned_value += coin.value;
        let Some(exposed_type) = ExposedType::of(&coin.script) else {
            return;
        };
        let index = match self
            .rows
            .binary_search_by_key(&exposed_type, |row| row.exposed_type)
        {
            Ok(index) => index,
            Err(index) => {
                self.rows.insert(
                    index,
                    MoveCostRow {
                        exposed_type,
                        all: MoveCostTotals::default(),
                        above_dust: MoveCostTotals::default(),
                    },
                );
                index
            }
        };
        let this_coin = MoveCostTotals {
            coins: 1,
            value: coin.value,
            floor_weight: exposed_type.spend_weight(),
            ceiling_weight: exposed_type.ceiling_weight(),
        };
        let row = &mut self.rows[index];
        row.all.add(&this_coin);
        if coin.value >= DUST_LIMIT {
            row.above_dust.add(&this_coin);
        }
    }

    /// Every row summed: (all coins, coins of at least 546 sats).
    #[must_use]
    pub fn total(&self) -> (MoveCostTotals, MoveCostTotals) {
        let mut all = MoveCostTotals::default();
        let mut above_dust = MoveCostTotals::default();
        for row in &self.rows {
            all.add(&row.all);
            above_dust.add(&row.above_dust);
        }
        (all, above_dust)
    }
}

/// The **Move cost** of `coins`. To stream a snapshot without collecting it,
/// use [`MoveCost::add`] coin by coin instead.
pub fn move_cost<'a>(coins: impl IntoIterator<Item = &'a Coin>) -> MoveCost {
    let mut cost = MoveCost::new();
    for coin in coins {
        cost.add(coin);
    }
    cost
}
