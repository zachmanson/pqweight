//! Sums `migrate()` over many transactions: what would this cost the network,
//! and how often does template coverage fail, across a batch rather than one
//! spend.

use crate::migration::{
    BaselineSpendType, InputResult, KeyExposure, ParameterSet, UnmappedReason, migrate,
};
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

/// What a breakdown row groups inputs by: the Baseline spend type of Mapped
/// inputs, or the Unmapped reason of Unmapped ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakdownKind {
    Mapped(BaselineSpendType),
    Unmapped(UnmappedReason),
}

/// Every input of one kind across the batch, from every parsed transaction
/// (fully or partially mapped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakdownRow {
    pub kind: BreakdownKind,
    pub inputs: usize,
    /// Summed **Input weight** today.
    pub baseline_weight: u64,
    /// Summed **Input weight** after migration. `None` for Unmapped rows.
    pub migrated_weight: Option<u64>,
}

/// Every input with one **Key exposure** across the batch: Mapped inputs summed
/// for their **Added weight**, Unmapped ones counted apart since they have none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExposureRow {
    pub key_exposure: KeyExposure,
    pub mapped_inputs: usize,
    /// Summed **Input weight** today of the Mapped inputs.
    pub baseline_weight: u64,
    /// Summed **Input weight** after migration of the Mapped inputs.
    pub migrated_weight: u64,
    pub unmapped_inputs: usize,
    /// Summed **Input weight** today of the Unmapped inputs. When this isn't
    /// zero, the row's Added weight is a lower bound.
    pub unmapped_baseline_weight: u64,
}

impl ExposureRow {
    /// The block space migrating this row's Mapped inputs would add: migrated
    /// minus baseline **Input weight**. Signed, since a template may in
    /// principle shrink an input. Weights never come near 2^63, so the casts
    /// never wrap.
    #[must_use]
    pub fn added_weight(&self) -> i64 {
        self.migrated_weight.cast_signed() - self.baseline_weight.cast_signed()
    }
}

/// The result of running `migrate()` over many transactions and summing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateResult {
    /// Today's weight/vsize, summed over every transaction that parsed.
    pub baseline: AggregateTotals,
    /// Migrated weight/vsize, summed over fully-mapped transactions only.
    /// `None` when no transaction is fully mapped (nothing to sum).
    pub migrated: Option<AggregateTotals>,
    /// Today's weight/vsize of the partially mapped transactions alone: the part
    /// of `baseline` that has no migrated total.
    pub partially_mapped: AggregateTotals,
    /// `None` unless a fee rate was given.
    pub fees: Option<AggregateFeeTotals>,
    pub counts: AggregateCounts,
    /// One row per kind of input seen, in order of first appearance.
    pub breakdown: Vec<BreakdownRow>,
    /// One row per Key exposure, always all four, in the order `Exposed in
    /// output`, `Hashed until spend`, `No key`, `Undetermined`.
    pub exposure: Vec<ExposureRow>,
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
    let mut partially_mapped = AggregateTotals::default();
    let mut baseline_fee = 0u64;
    let mut migrated_fee = 0u64;
    let mut counts = AggregateCounts::default();
    let mut breakdown: Vec<BreakdownRow> = Vec::new();
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

        for input in &tx_migration.inputs {
            add_to_breakdown(&mut breakdown, input);
        }

        counts.unmapped_inputs += tx_migration
            .inputs
            .iter()
            .filter(|input| matches!(input, InputResult::Unmapped { .. }))
            .count();

        if let Some(total) = tx_migration.migrated {
            counts.fully_mapped += 1;
            migrated.weight += total.weight;
            migrated.vsize += total.vsize;
            if let Some(rate) = fee_rate {
                migrated_fee += fee(total.vsize, rate);
            }
        } else {
            counts.partially_mapped += 1;
            partially_mapped.weight += tx_baseline.weight;
            partially_mapped.vsize += tx_baseline.vsize;
        }
    }
    counts.parse_errors = errors.len();

    AggregateResult {
        baseline,
        migrated: (counts.fully_mapped > 0).then_some(migrated),
        partially_mapped,
        fees: fee_rate.map(|_| AggregateFeeTotals {
            baseline: baseline_fee,
            migrated: (counts.fully_mapped > 0).then_some(migrated_fee),
        }),
        counts,
        exposure: exposure_rows(&breakdown),
        breakdown,
        errors,
    }
}

/// The breakdown regrouped by Key exposure, one row for each of the four values
/// even when no input has it.
fn exposure_rows(breakdown: &[BreakdownRow]) -> Vec<ExposureRow> {
    [
        KeyExposure::ExposedInOutput,
        KeyExposure::HashedUntilSpend,
        KeyExposure::NoKey,
        KeyExposure::Undetermined,
    ]
    .into_iter()
    .map(|key_exposure| {
        let mut row = ExposureRow {
            key_exposure,
            mapped_inputs: 0,
            baseline_weight: 0,
            migrated_weight: 0,
            unmapped_inputs: 0,
            unmapped_baseline_weight: 0,
        };
        for kind_row in breakdown {
            match kind_row.kind {
                BreakdownKind::Mapped(spend_type) if spend_type.key_exposure() == key_exposure => {
                    row.mapped_inputs += kind_row.inputs;
                    row.baseline_weight += kind_row.baseline_weight;
                    row.migrated_weight += kind_row.migrated_weight.unwrap_or(0);
                }
                BreakdownKind::Unmapped(reason) if reason.key_exposure() == key_exposure => {
                    row.unmapped_inputs += kind_row.inputs;
                    row.unmapped_baseline_weight += kind_row.baseline_weight;
                }
                BreakdownKind::Mapped(_) | BreakdownKind::Unmapped(_) => {}
            }
        }
        row
    })
    .collect()
}

/// Adds one input to the row for its kind, starting that row if it is the
/// first input of its kind.
fn add_to_breakdown(breakdown: &mut Vec<BreakdownRow>, input: &InputResult) {
    let (kind, migrated_weight) = match *input {
        InputResult::Mapped {
            spend_type,
            template_weight,
            ..
        } => (BreakdownKind::Mapped(spend_type), Some(template_weight)),
        InputResult::Unmapped { reason, .. } => (BreakdownKind::Unmapped(reason), None),
    };
    let index = breakdown
        .iter()
        .position(|row| row.kind == kind)
        .unwrap_or_else(|| {
            breakdown.push(BreakdownRow {
                kind,
                inputs: 0,
                baseline_weight: 0,
                migrated_weight: migrated_weight.map(|_| 0),
            });
            breakdown.len() - 1
        });
    let row = &mut breakdown[index];
    row.inputs += 1;
    row.baseline_weight += input.baseline_weight();
    row.migrated_weight = row.migrated_weight.zip(migrated_weight).map(|(a, b)| a + b);
}
