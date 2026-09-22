//! Sums `migrate()` over many transactions: what would this cost the network,
//! and how often does template coverage fail, across a batch rather than one
//! spend.

use crate::migration::{InputResult, ParameterSet, migrate};
use crate::{FeeRate, TransactionWeight, fee, transaction_weight};

/// Weight and vsize summed across transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AggregateTotals {
    pub weight: u64,
    pub vsize: u64,
}

/// Fee totals at a given rate, each summed **per transaction** (`fee(tx1) +
/// fee(tx2) + ...`), never from one `fee()` call on a summed vsize: each real
/// transaction pays its own fee and its own round-up separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AggregateFeeTotals {
    /// Summed over every transaction that parsed.
    pub baseline: u64,
    /// Summed over fully-mapped transactions only. `None` when none exist.
    pub migrated: Option<u64>,
}

/// A line that failed to parse as a transaction: recorded, not silently
/// dropped, and does not abort the batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateError {
    /// 1-indexed line number in the input.
    pub line: usize,
    pub message: String,
}

/// Transaction counts across the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AggregateCounts {
    pub parsed: usize,
    pub fully_mapped: usize,
    /// Transactions with at least one Unmapped input (including ones where
    /// every input is Unmapped).
    pub partially_mapped: usize,
    /// Total Unmapped input count across every parsed transaction.
    pub unmapped_inputs: usize,
    pub parse_errors: usize,
}

/// The result of running `migrate()` over many transactions and summing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateResult {
    /// Today's weight/vsize, summed over every transaction that parsed.
    pub baseline: AggregateTotals,
    /// Migrated weight/vsize, summed over fully-mapped transactions only.
    /// `None` when no transaction is fully mapped (nothing to sum).
    pub migrated: Option<AggregateTotals>,
    /// `None` unless a fee rate was given.
    pub fees: Option<AggregateFeeTotals>,
    pub counts: AggregateCounts,
    pub errors: Vec<AggregateError>,
}

/// Runs `migrate()` over `lines` (one raw transaction hex string per line,
/// blank lines skipped) and sums the results.
///
/// A line that fails to decode or parse is recorded in
/// [`AggregateResult::errors`] with its line number; the rest of the batch is
/// still aggregated.
pub fn aggregate(
    lines: impl Iterator<Item = String>,
    parameter_set: ParameterSet,
    fee_rate: Option<FeeRate>,
) -> AggregateResult {
    let mut baseline = AggregateTotals::default();
    let mut migrated = AggregateTotals::default();
    let mut baseline_fee = 0u64;
    let mut migrated_fee = 0u64;
    let mut counts = AggregateCounts::default();
    let mut errors = Vec::new();

    for (index, raw_line) in lines.enumerate() {
        let line = index + 1;
        let hex = raw_line.trim();
        if hex.is_empty() {
            continue;
        }

        let bytes = match crate::decode_hex(hex) {
            Ok(bytes) => bytes,
            Err(message) => {
                errors.push(AggregateError { line, message });
                continue;
            }
        };
        let tx_baseline: TransactionWeight = match transaction_weight(&bytes) {
            Ok(weight) => weight,
            Err(err) => {
                errors.push(AggregateError {
                    line,
                    message: err.to_string(),
                });
                continue;
            }
        };
        let tx_migration = match migrate(&bytes, parameter_set) {
            Ok(migration) => migration,
            Err(err) => {
                errors.push(AggregateError {
                    line,
                    message: err.to_string(),
                });
                continue;
            }
        };

        counts.parsed += 1;
        baseline.weight += tx_baseline.weight;
        baseline.vsize += tx_baseline.vsize;
        if let Some(rate) = fee_rate {
            baseline_fee += fee(tx_baseline.vsize, rate);
        }

        counts.unmapped_inputs += tx_migration
            .inputs
            .iter()
            .filter(|input| matches!(input, InputResult::Unmapped))
            .count();

        match tx_migration.migrated {
            Some(total) => {
                counts.fully_mapped += 1;
                migrated.weight += total.weight;
                migrated.vsize += total.vsize;
                if let Some(rate) = fee_rate {
                    migrated_fee += fee(total.vsize, rate);
                }
            }
            None => counts.partially_mapped += 1,
        }
    }
    counts.parse_errors = errors.len();

    AggregateResult {
        baseline,
        migrated: (counts.fully_mapped > 0).then_some(migrated),
        fees: fee_rate.map(|_| AggregateFeeTotals {
            baseline: baseline_fee,
            migrated: (counts.fully_mapped > 0).then_some(migrated_fee),
        }),
        counts,
        errors,
    }
}
