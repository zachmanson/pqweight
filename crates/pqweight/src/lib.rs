//! Bitcoin transaction weight, and how it changes under post-quantum signatures.

mod aggregate;
mod fee;
mod migration;
mod parser;

pub use aggregate::{
    AggregateCounts, AggregateError, AggregateFeeTotals, AggregateResult, AggregateTotals,
    BreakdownKind, BreakdownRow, ExposureRow, aggregate,
};
pub use fee::{FeeRate, FeeRateError, fee};
pub use migration::{
    BaselineSpendType, InputResult, KeyExposure, Migration, MultisigThreshold, ParameterSet,
    UnmappedReason, migrate,
};
pub use parser::ParseError;

/// The crate version, so the CLI can report which library it was built against.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The measured size of a transaction, in bytes and weight units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionWeight {
    /// Consensus size measure: 3 x `stripped_size` + `total_size`.
    pub weight: u64,
    /// `weight` divided by 4, rounded up.
    pub vsize: u64,
    /// Serialization without segwit marker, flag and witness data.
    pub stripped_size: u64,
    /// Full serialization, including witness data.
    pub total_size: u64,
}

/// Decodes a hex string into raw bytes.
///
/// Not part of the public API: `aggregate()` is the only new seam this slice
/// adds (see the ticket's Seams decision), so this stays crate-internal
/// rather than becoming a second one. The CLI keeps its own copy.
fn decode_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("invalid hex: odd number of digits".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| format!("invalid hex at position {i}"))
        })
        .collect()
}

/// Measures the **weight** of a raw serialized transaction.
///
/// # Errors
///
/// Returns a [`ParseError`] if `bytes` is not a complete transaction.
pub fn transaction_weight(bytes: &[u8]) -> Result<TransactionWeight, ParseError> {
    let stripped_size = parser::measure(bytes)?;
    let total_size = bytes.len() as u64;
    let weight = 3 * stripped_size + total_size;
    Ok(TransactionWeight {
        weight,
        vsize: weight.div_ceil(4),
        stripped_size,
        total_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!version().is_empty());
    }
}
