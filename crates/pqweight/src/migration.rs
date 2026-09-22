//! Models a transaction's spends under post-quantum signatures.
//!
//! Bitcoin has no PQ opcode, so every number here comes from a **Migration
//! template**: a stated assumption about what a PQ witness would contain
//! (see ADR-0002).

use crate::parser::{self, ParsedInput};
use crate::{ParseError, TransactionWeight};

/// A specific configuration of a **Signature scheme** with fixed sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterSet {
    /// ML-DSA-44, FIPS 204.
    MlDsa44,
    /// Falcon-512. Falcon signatures are variable-length; this uses the
    /// fixed 666-byte padded signature size (`CRYPTO_BYTES`) from the Falcon
    /// round-3 spec, a stated assumption (ADR-0002) that must appear in the
    /// Assumptions block wherever it is used. Public key 897 bytes
    /// (`CRYPTO_PUBLICKEYBYTES`).
    Falcon512,
    /// SLH-DSA-128s, FIPS 205 (the "small", slower-signing 128-bit parameter
    /// set): signature 7856 bytes, public key 32 bytes.
    SlhDsa128s,
}

impl ParameterSet {
    /// Signature size in bytes.
    fn signature_size(self) -> u64 {
        match self {
            Self::MlDsa44 => 2420,
            Self::Falcon512 => 666,
            Self::SlhDsa128s => 7856,
        }
    }

    /// Public key size in bytes.
    fn public_key_size(self) -> u64 {
        match self {
            Self::MlDsa44 => 1312,
            Self::Falcon512 => 897,
            Self::SlhDsa128s => 32,
        }
    }
}

/// The kind of spend a real input performs today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineSpendType {
    P2wpkh,
    P2trKeyPath,
    P2shP2wpkh,
    P2pkh,
    /// Empty scriptSig, empty witness: nothing to migrate.
    PayToAnchor,
}

/// The outcome for one input: its **Migration template** weight, or **Unmapped**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputResult {
    Mapped {
        spend_type: BaselineSpendType,
        /// Weight of the whole input after migration: its non-witness bytes at 4 WU
        /// each plus its witness bytes at 1 WU each.
        template_weight: u64,
    },
    Unmapped,
}

/// The outcome of migrating a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// One result per input, in transaction order.
    pub inputs: Vec<InputResult>,
    /// The migrated transaction's size. `None` unless every input is mapped.
    pub migrated: Option<TransactionWeight>,
    /// Whether the migrated total exceeds Bitcoin Core's 400,000 WU relay
    /// limit. This is policy, not consensus (ADR-0002). `None` when there is
    /// no migrated total to check.
    pub exceeds_relay_limit: Option<bool>,
    /// The stated facts this migration relies on (ADR-0002): Bitcoin has no PQ
    /// opcode, so every template is an assumption, not an enforced rule.
    pub assumptions: Vec<&'static str>,
}

/// Bitcoin Core's `MAX_STANDARD_TX_WEIGHT`: relay policy, not a consensus rule.
const RELAY_WEIGHT_LIMIT: u64 = 400_000;

/// Assumptions every migration makes, regardless of parameter set.
const BASE_ASSUMPTIONS: [&str; 3] = [
    "PQ outputs commit to a hash of the public key, as today's outputs do",
    "the 520-byte witness item limit and sigop accounting are changed by a soft fork",
    "outputs are unchanged by migration",
];

/// The assumptions that apply to `parameter_set`, in a fixed order: the base
/// assumptions every migration states, plus any specific to this parameter set.
fn assumptions(parameter_set: ParameterSet) -> Vec<&'static str> {
    let mut assumptions: Vec<&'static str> = BASE_ASSUMPTIONS.to_vec();
    if parameter_set == ParameterSet::Falcon512 {
        assumptions.push(
            "Falcon-512 signatures are padded to a fixed 666 bytes; real signatures are variable-length",
        );
    }
    assumptions
}

/// Segwit marker and flag.
const MARKER_AND_FLAG_SIZE: u64 = 2;

/// Migrates every input of a raw serialized transaction to `parameter_set`.
///
/// # Errors
///
/// Returns a [`ParseError`] if `bytes` is not a complete transaction.
pub fn migrate(bytes: &[u8], parameter_set: ParameterSet) -> Result<Migration, ParseError> {
    let tx = parser::parse(bytes)?;

    let mut inputs = Vec::new();
    let mut baseline_input_stripped_size = 0;
    let mut migrated_input_stripped_size = 0;
    let mut migrated_witness_total = 0;
    for input in &tx.inputs {
        baseline_input_stripped_size += input_stripped_size(input.script_sig);
        inputs.push(match classify(input) {
            Some(spend_type) => {
                let non_witness_size =
                    input_stripped_size(migrated_script_sig(spend_type, input.script_sig));
                let witness_size = migrated_witness_size(spend_type, parameter_set);
                migrated_input_stripped_size += non_witness_size;
                migrated_witness_total += witness_size;
                InputResult::Mapped {
                    spend_type,
                    template_weight: 4 * non_witness_size + witness_size,
                }
            }
            None => InputResult::Unmapped,
        });
    }

    let migrated = inputs
        .iter()
        .all(|result| matches!(result, InputResult::Mapped { .. }))
        .then(|| {
            let stripped_size =
                tx.stripped_size - baseline_input_stripped_size + migrated_input_stripped_size;
            let total_size = stripped_size + MARKER_AND_FLAG_SIZE + migrated_witness_total;
            let weight = 3 * stripped_size + total_size;
            TransactionWeight {
                weight,
                vsize: weight.div_ceil(4),
                stripped_size,
                total_size,
            }
        });

    let exceeds_relay_limit = migrated.as_ref().map(|w| w.weight > RELAY_WEIGHT_LIMIT);

    Ok(Migration {
        inputs,
        migrated,
        exceeds_relay_limit,
        assumptions: assumptions(parameter_set),
    })
}

/// The scriptSig a template leaves after migration: empty for the witness-only
/// templates, unchanged for P2SH-P2WPKH, which still needs a scriptSig that
/// hashes to its P2SH output.
fn migrated_script_sig(spend_type: BaselineSpendType, original: &[u8]) -> &[u8] {
    match spend_type {
        BaselineSpendType::P2wpkh
        | BaselineSpendType::P2trKeyPath
        | BaselineSpendType::P2pkh
        | BaselineSpendType::PayToAnchor => &[],
        BaselineSpendType::P2shP2wpkh => original,
    }
}

/// Bytes of the migrated witness for a mapped input: the PQ template for every
/// spend type but pay-to-anchor, which has nothing to migrate and keeps its
/// single item-count byte.
fn migrated_witness_size(spend_type: BaselineSpendType, parameter_set: ParameterSet) -> u64 {
    match spend_type {
        BaselineSpendType::P2wpkh
        | BaselineSpendType::P2trKeyPath
        | BaselineSpendType::P2shP2wpkh
        | BaselineSpendType::P2pkh => template_witness_size(parameter_set),
        BaselineSpendType::PayToAnchor => 1,
    }
}

fn classify(input: &ParsedInput<'_>) -> Option<BaselineSpendType> {
    let witness = input.witness.as_deref().unwrap_or(&[]);
    if is_p2sh_p2wpkh_redeem_script_push(input.script_sig)
        && is_der_signature_and_compressed_pubkey(witness)
    {
        return Some(BaselineSpendType::P2shP2wpkh);
    }
    // A witness section present for this input, but empty: the spending side
    // explicitly asserts "nothing needed here", which only pay-to-anchor does.
    if input.script_sig.is_empty() && matches!(input.witness.as_deref(), Some([])) {
        return Some(BaselineSpendType::PayToAnchor);
    }
    if witness.is_empty()
        && let Some(pushes) = read_direct_pushes(input.script_sig)
        && is_der_signature_and_legacy_pubkey(&pushes)
    {
        return Some(BaselineSpendType::P2pkh);
    }
    if !input.script_sig.is_empty() {
        return None;
    }
    if is_der_signature_and_compressed_pubkey(witness) {
        return Some(BaselineSpendType::P2wpkh);
    }
    if let [signature] = witness {
        // Schnorr signature, an optional trailing sighash byte. Anything with a
        // second witness item is treated as carrying an annex, which is Unmapped.
        if (64..=65).contains(&signature.len()) {
            return Some(BaselineSpendType::P2trKeyPath);
        }
    }
    None
}

/// A scriptSig of exactly a DER signature and a 33 or 65-byte public key.
fn is_der_signature_and_legacy_pubkey(pushes: &[&[u8]]) -> bool {
    matches!(pushes, [signature, pubkey]
        if (70..=73).contains(&signature.len()) && matches!(pubkey.len(), 33 | 65))
}

/// Reads `script_sig` as a sequence of direct data pushes (opcodes 0x01 to 0x4b,
/// each meaning "push the next N bytes"), the only push form a DER signature or a
/// legacy public key needs. Returns `None` if any byte is not a direct-push
/// length, or a push runs past the end of the script.
fn read_direct_pushes(script_sig: &[u8]) -> Option<Vec<&[u8]>> {
    let mut pushes = Vec::new();
    let mut rest = script_sig;
    while let Some((&len, tail)) = rest.split_first() {
        if !(1..=0x4b).contains(&len) {
            return None;
        }
        let (push, tail) = tail.split_at_checked(usize::from(len))?;
        pushes.push(push);
        rest = tail;
    }
    Some(pushes)
}

/// A witness of exactly a DER signature (70 to 73 bytes, sighash byte included)
/// and a 33-byte compressed public key. Lengths only; nothing is verified.
fn is_der_signature_and_compressed_pubkey(witness: &[&[u8]]) -> bool {
    matches!(witness, [signature, pubkey]
        if (70..=73).contains(&signature.len()) && pubkey.len() == 33)
}

/// A scriptSig that is a single push of exactly `0014<20-byte hash>` (22 bytes):
/// the P2SH-P2WPKH redeem script.
fn is_p2sh_p2wpkh_redeem_script_push(script_sig: &[u8]) -> bool {
    let [push_len, 0x00, 0x14, hash @ ..] = script_sig else {
        return false;
    };
    *push_len == 0x16 && hash.len() == 20
}

/// Bytes of the template witness `[pq_signature, pq_pubkey]`: the item count plus
/// each item with its compact-size length prefix.
fn template_witness_size(parameter_set: ParameterSet) -> u64 {
    let signature = parameter_set.signature_size();
    let public_key = parameter_set.public_key_size();
    1 + compact_size_len(signature) + signature + compact_size_len(public_key) + public_key
}

/// Non-witness bytes of an input given its scriptSig: outpoint 36, length prefix
/// and scriptSig itself, sequence 4.
fn input_stripped_size(script_sig: &[u8]) -> u64 {
    let script_len = script_sig.len() as u64;
    36 + compact_size_len(script_len) + script_len + 4
}

fn compact_size_len(value: u64) -> u64 {
    match value {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}
