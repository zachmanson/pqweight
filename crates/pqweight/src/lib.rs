//! Bitcoin transaction weight, and how it changes under post-quantum signatures.

mod fee;
mod migration;
mod parser;

pub use fee::{FeeRate, FeeRateError, fee};
pub use migration::{BaselineSpendType, InputResult, Migration, ParameterSet, migrate};
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
