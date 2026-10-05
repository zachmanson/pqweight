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

/// **Key exposure**: where an input's public key sat before this spend, as
/// seen from the spending side. Names what was seen, never what an attacker
/// could do: a key that is only hashed may already be public through address
/// reuse, which the spending side can't show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyExposure {
    /// The key is in the output itself (P2TR, P2PK, bare multisig): open to a
    /// Long-exposure attack for as long as the coins sit there.
    ExposedInOutput,
    /// Only a hash was on-chain until this spend revealed the key.
    HashedUntilSpend,
    /// No key to attack (pay-to-anchor, coinbase).
    NoKey,
    /// The spend shape doesn't say where the key was.
    Undetermined,
}

/// The kind of spend a real input performs today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineSpendType {
    P2wpkh,
    P2trKeyPath,
    P2shP2wpkh,
    P2pkh,
    /// Empty scriptSig, empty witness (or no witness section): no signature,
    /// nothing to migrate. From the spending side this also covers any other
    /// output that needs no key, such as a bare `OP_TRUE`, which migrates the
    /// same way.
    PayToAnchor,
    /// The single input of a block's first transaction: no previous output, no
    /// signature, nothing to migrate.
    Coinbase,
    /// Empty scriptSig, witness `[dummy, m signatures, OP_CHECKMULTISIG script]`.
    P2wshMultisig(MultisigThreshold),
    /// scriptSig is one push of `0020<32-byte script hash>`, witness as for
    /// P2WSH multisig.
    P2shP2wshMultisig(MultisigThreshold),
    /// Empty witness, scriptSig of pushes `[OP_0, m signatures, redeem script]`.
    /// Migrates to a witness-carried script with an empty scriptSig.
    P2shMultisig(MultisigThreshold),
    /// Empty scriptSig, witness `[stack items, leaf, control block]` whose leaf
    /// is a **Single-key leaf**. Migrates to a PQ key in the leaf, a PQ
    /// signature in place of the Schnorr one, and a control block without the
    /// internal key (see [`single_key_leaf_spend`]).
    P2trScriptPathSingleKey,
    /// Empty scriptSig, witness `[stack items, script]` whose script is a
    /// **Contract script**. Migrates by swapping every key and signature for a
    /// PQ one and keeping every other byte (see [`contract_spend`]).
    P2wshContract,
    /// scriptSig is one push of `0020<32-byte script hash>`, witness as for a
    /// P2WSH contract.
    P2shP2wshContract,
}

impl BaselineSpendType {
    /// Whether this spend has no signature, so migration leaves it unchanged.
    #[must_use]
    pub fn is_no_op(self) -> bool {
        matches!(self, Self::PayToAnchor | Self::Coinbase)
    }

    /// Whether this is a P2WSH or P2SH-P2WSH spend of a **Contract script**.
    #[must_use]
    pub fn is_contract(self) -> bool {
        matches!(self, Self::P2wshContract | Self::P2shP2wshContract)
    }

    /// Where this spend type's public key sat before the spend.
    #[must_use]
    pub fn key_exposure(self) -> KeyExposure {
        match self {
            // A script-path output key is the internal key tweaked, so breaking
            // it opens the key path whatever the leaves say.
            Self::P2trKeyPath | Self::P2trScriptPathSingleKey => KeyExposure::ExposedInOutput,
            Self::P2wpkh
            | Self::P2shP2wpkh
            | Self::P2pkh
            | Self::P2wshMultisig(_)
            | Self::P2shP2wshMultisig(_)
            | Self::P2shMultisig(_)
            | Self::P2wshContract
            | Self::P2shP2wshContract => KeyExposure::HashedUntilSpend,
            Self::PayToAnchor | Self::Coinbase => KeyExposure::NoKey,
        }
    }

    /// The Multisig threshold of a multisig spend type, `None` for single-key ones.
    #[must_use]
    pub fn threshold(self) -> Option<MultisigThreshold> {
        match self {
            Self::P2wshMultisig(threshold)
            | Self::P2shP2wshMultisig(threshold)
            | Self::P2shMultisig(threshold) => Some(threshold),
            Self::P2wpkh
            | Self::P2trKeyPath
            | Self::P2shP2wpkh
            | Self::P2pkh
            | Self::PayToAnchor
            | Self::Coinbase
            | Self::P2trScriptPathSingleKey
            | Self::P2wshContract
            | Self::P2shP2wshContract => None,
        }
    }
}

/// The m-of-n shape of a multisig spend: n public keys in its script, m
/// signatures in its witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MultisigThreshold {
    pub m: u8,
    pub n: u8,
}

/// The outcome for one input: its **Migration template** weight, or **Unmapped**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputResult {
    Mapped {
        spend_type: BaselineSpendType,
        /// The input's **Input weight** today.
        baseline_weight: u64,
        /// The input's **Input weight** after migration: its non-witness bytes at
        /// 4 WU each plus its witness bytes at 1 WU each.
        template_weight: u64,
    },
    Unmapped {
        reason: UnmappedReason,
        /// The input's **Input weight** today.
        baseline_weight: u64,
    },
}

/// The spend shape observed on an **Unmapped** input, recognized from the
/// spending side only. A label for what was seen, not a classification to rely
/// on: the checks are heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmappedReason {
    /// Empty scriptSig; after removing any annex, at least 2 witness items remain
    /// and the last is a control block (33 + 32k bytes, tapscript leaf version),
    /// but the spend is not a single-key leaf (multi-key leaves, an annex, and
    /// other leaf shapes).
    P2trScriptPath,
    /// Empty scriptSig, a 64 or 65-byte Schnorr signature, then an annex.
    P2trKeyPathAnnex,
    /// Empty scriptSig and 2 or more witness items: a witness script spend that
    /// is neither standard multisig nor a **Contract script** (a truncated push,
    /// no signature check, or no public key in the script or on the stack).
    P2wshNonMultisig,
    /// scriptSig is one push of a `0014` or `0020` redeem script (P2SH-P2WPKH or
    /// P2SH-P2WSH), but the witness matched no template.
    P2shSegwitNonMultisig,
    /// Empty witness and a push-only scriptSig whose last push looks like a
    /// redeem script: not a signature or public key, and well-formed script ops.
    P2shNonMultisig,
    /// Empty witness and a scriptSig of exactly one push, a strict-DER
    /// signature: a P2PK spend, whose public key sits in the output itself.
    P2pk,
    /// Empty witness and a scriptSig of `OP_0` then one or more pushes, every one
    /// a strict-DER signature, with no redeem script after them: a bare
    /// multisig spend, whose public keys sit in the output itself.
    BareMultisig,
    /// Empty witness and any other non-empty scriptSig, such as P2PKH spends
    /// outside the template's signature and key sizes.
    LegacyOther,
    /// None of the shapes above.
    Unknown,
}

impl UnmappedReason {
    /// Where the public key of the shape this reason names sat before the spend.
    #[must_use]
    pub fn key_exposure(self) -> KeyExposure {
        match self {
            Self::P2trScriptPath | Self::P2trKeyPathAnnex | Self::P2pk | Self::BareMultisig => {
                KeyExposure::ExposedInOutput
            }
            Self::P2wshNonMultisig | Self::P2shSegwitNonMultisig | Self::P2shNonMultisig => {
                KeyExposure::HashedUntilSpend
            }
            Self::LegacyOther | Self::Unknown => KeyExposure::Undetermined,
        }
    }
}

impl InputResult {
    /// The input's **Input weight** today: its own non-witness bytes at 4 WU
    /// each plus its witness bytes at 1 WU each.
    #[must_use]
    pub fn baseline_weight(&self) -> u64 {
        match *self {
            Self::Mapped {
                baseline_weight, ..
            }
            | Self::Unmapped {
                baseline_weight, ..
            } => baseline_weight,
        }
    }
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
pub const RELAY_WEIGHT_LIMIT: u64 = 400_000;

/// Assumptions every migration makes, regardless of parameter set.
const BASE_ASSUMPTIONS: [&str; 3] = [
    "PQ outputs commit to a hash of the public key, as today's outputs do",
    "the 520-byte stack element limit (witness items and pushes inside a script) and sigop accounting are changed by a soft fork",
    "outputs are unchanged by migration",
];

/// Assumptions stated when any input is a multisig spend.
const MULTISIG_ASSUMPTIONS: [&str; 2] = [
    "the 10,000-byte script size limit and witnessScript standardness limits (3,600 bytes, 100 stack items) are raised by a soft fork",
    "PQ multisig keeps today's OP_CHECKMULTISIG layout, including its dummy element, with every public key in the script",
];

/// Assumption stated when any input is a P2WSH or P2SH-P2WSH contract spend
/// (ticket 06, decision 10): how the template reads a **Contract script**.
const CONTRACT_ASSUMPTION: &str = "in a P2WSH contract, every key-shaped push (33 bytes starting 02 or 03, or 65 bytes starting 04) is a public key and every strict-DER stack item is a signature; all other bytes are carried unchanged";

/// Assumption stated when any input is a legacy P2SH multisig spend.
const P2SH_MULTISIG_ASSUMPTION: &str =
    "P2SH multisig spends migrate to a witness-carried script with an empty scriptSig";

/// Assumption stated when any input is a P2TR script-path single-key spend
/// (ticket 05, decision 1).
const P2TR_SCRIPT_PATH_ASSUMPTION: &str = "migrated script-path outputs commit to the script tree's Merkle root directly (no internal key, no key path, as in BIP-360); the control block is the leaf-version byte plus the Merkle path";

/// The assumptions that apply, in a fixed order: the base assumptions every
/// migration states, then any specific to this parameter set, then any specific
/// to the spend types that were mapped. `contract_uses_checkmultisig` says
/// whether a mapped contract's script contains `OP_CHECKMULTISIG(VERIFY)`,
/// which the spend type alone doesn't record.
fn assumptions(
    parameter_set: ParameterSet,
    inputs: &[InputResult],
    contract_uses_checkmultisig: bool,
) -> Vec<&'static str> {
    let mut assumptions: Vec<&'static str> = BASE_ASSUMPTIONS.to_vec();
    if parameter_set == ParameterSet::Falcon512 {
        assumptions.push(
            "Falcon-512 signatures are padded to a fixed 666 bytes; real signatures are variable-length",
        );
    }
    let spend_types = || {
        inputs.iter().filter_map(|input| match input {
            InputResult::Mapped { spend_type, .. } => Some(*spend_type),
            InputResult::Unmapped { .. } => None,
        })
    };
    let [script_limits, checkmultisig_layout] = MULTISIG_ASSUMPTIONS;
    let is_multisig = spend_types().any(|spend_type| spend_type.threshold().is_some());
    let is_contract = spend_types().any(BaselineSpendType::is_contract);
    if is_multisig || is_contract {
        assumptions.push(script_limits);
    }
    if is_multisig || contract_uses_checkmultisig {
        assumptions.push(checkmultisig_layout);
    }
    if spend_types().any(|spend_type| matches!(spend_type, BaselineSpendType::P2shMultisig(_))) {
        assumptions.push(P2SH_MULTISIG_ASSUMPTION);
    }
    if spend_types().any(|spend_type| spend_type == BaselineSpendType::P2trScriptPathSingleKey) {
        assumptions.push(P2TR_SCRIPT_PATH_ASSUMPTION);
    }
    if is_contract {
        assumptions.push(CONTRACT_ASSUMPTION);
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
    let is_coinbase = matches!(tx.inputs.as_slice(), [input] if is_null_outpoint(input));
    let spend_types: Vec<Option<BaselineSpendType>> = tx
        .inputs
        .iter()
        .map(|input| {
            if is_coinbase {
                Some(BaselineSpendType::Coinbase)
            } else {
                classify(input)
            }
        })
        .collect();
    // The migrated transaction has a witness section if the original did, or if
    // any input gains a PQ witness. Then every input needs at least an item
    // count; otherwise (only no-op inputs, all legacy) it stays legacy.
    let migrated_is_segwit = tx.inputs.iter().any(|input| input.witness.is_some())
        || spend_types
            .iter()
            .flatten()
            .any(|spend_type| !spend_type.is_no_op());
    for (input, spend_type) in tx.inputs.iter().zip(spend_types) {
        let stripped_size = input_stripped_size(input.script_sig);
        baseline_input_stripped_size += stripped_size;
        let baseline_weight = 4 * stripped_size + input.witness.as_deref().map_or(0, witness_size);
        inputs.push(match spend_type {
            Some(spend_type) => {
                let non_witness_size =
                    input_stripped_size(migrated_script_sig(spend_type, input.script_sig));
                let witness_size = if migrated_is_segwit {
                    migrated_witness_size(spend_type, parameter_set, input)
                } else {
                    0
                };
                migrated_input_stripped_size += non_witness_size;
                migrated_witness_total += witness_size;
                InputResult::Mapped {
                    spend_type,
                    baseline_weight,
                    template_weight: 4 * non_witness_size + witness_size,
                }
            }
            None => InputResult::Unmapped {
                reason: unmapped_reason(input),
                baseline_weight,
            },
        });
    }

    let migrated = inputs
        .iter()
        .all(|result| matches!(result, InputResult::Mapped { .. }))
        .then(|| {
            let stripped_size =
                tx.stripped_size - baseline_input_stripped_size + migrated_input_stripped_size;
            let marker_and_flag = if migrated_is_segwit {
                MARKER_AND_FLAG_SIZE
            } else {
                0
            };
            let total_size = stripped_size + marker_and_flag + migrated_witness_total;
            let weight = 3 * stripped_size + total_size;
            TransactionWeight {
                weight,
                vsize: weight.div_ceil(4),
                stripped_size,
                total_size,
            }
        });

    let exceeds_relay_limit = migrated.as_ref().map(|w| w.weight > RELAY_WEIGHT_LIMIT);

    let contract_uses_checkmultisig = tx.inputs.iter().zip(&inputs).any(|(input, result)| {
        matches!(result, InputResult::Mapped { spend_type, .. } if spend_type.is_contract())
            && contract_spend(input.witness.as_deref().unwrap_or(&[]))
                .is_some_and(|contract| contract.uses_checkmultisig())
    });
    let assumptions = assumptions(parameter_set, &inputs, contract_uses_checkmultisig);
    Ok(Migration {
        inputs,
        migrated,
        exceeds_relay_limit,
        assumptions,
    })
}

/// The outpoint a coinbase input spends: an all-zero txid and index `0xffffffff`.
fn is_null_outpoint(input: &ParsedInput<'_>) -> bool {
    let (txid, index) = input.previous_output.split_at(32);
    txid.iter().all(|&byte| byte == 0) && index == [0xff; 4]
}

/// The scriptSig a template leaves after migration: empty for the witness-only
/// templates, unchanged for P2SH-P2WPKH, which still needs a scriptSig that
/// hashes to its P2SH output, and for a coinbase, which has nothing to migrate.
fn migrated_script_sig(spend_type: BaselineSpendType, original: &[u8]) -> &[u8] {
    match spend_type {
        BaselineSpendType::P2wpkh
        | BaselineSpendType::P2trKeyPath
        | BaselineSpendType::P2pkh
        | BaselineSpendType::PayToAnchor
        | BaselineSpendType::P2wshMultisig(_)
        | BaselineSpendType::P2shMultisig(_)
        | BaselineSpendType::P2trScriptPathSingleKey
        | BaselineSpendType::P2wshContract => &[],
        BaselineSpendType::P2shP2wpkh
        | BaselineSpendType::P2shP2wshMultisig(_)
        | BaselineSpendType::P2shP2wshContract
        | BaselineSpendType::Coinbase => original,
    }
}

/// Bytes of the migrated witness for a mapped input in a migrated transaction
/// that has a witness section: the PQ template for every spend type but the
/// no-op ones, which keep the witness they have (an empty one if they had
/// none).
fn migrated_witness_size(
    spend_type: BaselineSpendType,
    parameter_set: ParameterSet,
    input: &ParsedInput<'_>,
) -> u64 {
    match spend_type {
        BaselineSpendType::P2wpkh
        | BaselineSpendType::P2trKeyPath
        | BaselineSpendType::P2shP2wpkh
        | BaselineSpendType::P2pkh => template_witness_size(parameter_set),
        BaselineSpendType::PayToAnchor | BaselineSpendType::Coinbase => {
            witness_size(input.witness.as_deref().unwrap_or(&[]))
        }
        BaselineSpendType::P2wshMultisig(threshold)
        | BaselineSpendType::P2shP2wshMultisig(threshold)
        | BaselineSpendType::P2shMultisig(threshold) => {
            multisig_witness_size(threshold, parameter_set)
        }
        BaselineSpendType::P2trScriptPathSingleKey => {
            single_key_leaf_spend(input.witness.as_deref().unwrap_or(&[]))
                .expect("classified as a single-key leaf spend")
                .migrated_witness_size(parameter_set)
        }
        BaselineSpendType::P2wshContract | BaselineSpendType::P2shP2wshContract => {
            contract_spend(input.witness.as_deref().unwrap_or(&[]))
                .expect("classified as a contract spend")
                .migrated_witness_size(parameter_set)
        }
    }
}

fn classify(input: &ParsedInput<'_>) -> Option<BaselineSpendType> {
    let witness = input.witness.as_deref().unwrap_or(&[]);
    if is_p2sh_p2wpkh_redeem_script_push(input.script_sig)
        && is_der_signature_and_compressed_pubkey(witness)
    {
        return Some(BaselineSpendType::P2shP2wpkh);
    }
    if is_p2sh_p2wsh_redeem_script_push(input.script_sig)
        && let Some(threshold) = multisig_threshold(witness, KeySizes::CompressedOnly)
    {
        return Some(BaselineSpendType::P2shP2wshMultisig(threshold));
    }
    if is_p2sh_p2wsh_redeem_script_push(input.script_sig) && contract_spend(witness).is_some() {
        return Some(BaselineSpendType::P2shP2wshContract);
    }
    // Nothing in the scriptSig or witness: no signature to migrate. A
    // transaction whose inputs are all like this must be serialized without the
    // segwit marker, since Core rejects a marker when every witness is empty.
    if input.script_sig.is_empty() && witness.is_empty() {
        return Some(BaselineSpendType::PayToAnchor);
    }
    if witness.is_empty()
        && let Some(pushes) = read_direct_pushes(input.script_sig)
        && is_der_signature_and_legacy_pubkey(&pushes)
    {
        return Some(BaselineSpendType::P2pkh);
    }
    if witness.is_empty()
        && let Some(pushes) = read_push_only(input.script_sig)
        && let Some(threshold) = multisig_threshold(&pushes, KeySizes::CompressedOrUncompressed)
    {
        return Some(BaselineSpendType::P2shMultisig(threshold));
    }
    if !input.script_sig.is_empty() {
        return None;
    }
    if is_der_signature_and_compressed_pubkey(witness) {
        return Some(BaselineSpendType::P2wpkh);
    }
    if let Some(threshold) = multisig_threshold(witness, KeySizes::CompressedOnly) {
        return Some(BaselineSpendType::P2wshMultisig(threshold));
    }
    if single_key_leaf_spend(witness).is_some() {
        return Some(BaselineSpendType::P2trScriptPathSingleKey);
    }
    if let [signature] = witness {
        // Schnorr signature, an optional trailing sighash byte. Anything with a
        // second witness item is treated as carrying an annex, which is Unmapped.
        if (64..=65).contains(&signature.len()) {
            return Some(BaselineSpendType::P2trKeyPath);
        }
    }
    if contract_spend(witness).is_some() {
        return Some(BaselineSpendType::P2wshContract);
    }
    None
}

/// What an input no template covers looks like. Checked in a fixed order; the
/// first match wins.
fn unmapped_reason(input: &ParsedInput<'_>) -> UnmappedReason {
    let witness = input.witness.as_deref().unwrap_or(&[]);
    if input.script_sig.is_empty() && is_p2tr_script_path(without_annex(witness)) {
        return UnmappedReason::P2trScriptPath;
    }
    if input.script_sig.is_empty() && is_p2tr_key_path_with_annex(witness) {
        return UnmappedReason::P2trKeyPathAnnex;
    }
    if input.script_sig.is_empty() && witness.len() >= 2 {
        return UnmappedReason::P2wshNonMultisig;
    }
    if is_p2sh_p2wpkh_redeem_script_push(input.script_sig)
        || is_p2sh_p2wsh_redeem_script_push(input.script_sig)
    {
        return UnmappedReason::P2shSegwitNonMultisig;
    }
    if witness.is_empty()
        && let Some(pushes) = read_push_only(input.script_sig)
        && let Some(last) = pushes.last()
        && is_redeem_script_candidate(last)
    {
        return UnmappedReason::P2shNonMultisig;
    }
    if witness.is_empty()
        && let Some(pushes) = read_push_only(input.script_sig)
        && let [signature] = pushes.as_slice()
        && is_strict_der_signature(signature)
    {
        return UnmappedReason::P2pk;
    }
    if witness.is_empty()
        && let Some(pushes) = read_push_only(input.script_sig)
        && let [dummy, signatures @ ..] = pushes.as_slice()
        && dummy.is_empty()
        && !signatures.is_empty()
        && signatures.iter().all(|push| is_strict_der_signature(push))
    {
        return UnmappedReason::BareMultisig;
    }
    if witness.is_empty() && !input.script_sig.is_empty() {
        return UnmappedReason::LegacyOther;
    }
    UnmappedReason::Unknown
}

/// A last scriptSig push that could be a P2SH redeem script. Almost any bytes
/// read as some opcodes, so what rules a push out is looking like the other
/// things a legacy scriptSig ends with: a DER signature or a public key.
fn is_redeem_script_candidate(push: &[u8]) -> bool {
    let looks_like_signature = looks_like_der_signature(push);
    let looks_like_public_key = matches!(
        (push.len(), push.first()),
        (33, Some(0x02 | 0x03)) | (65, Some(0x04))
    );
    !push.is_empty()
        && !looks_like_signature
        && !looks_like_public_key
        && read_script_ops(push).is_some()
}

/// The witness with its BIP-341 annex removed: with at least 2 items, a last
/// item starting `0x50` is the annex.
fn without_annex<'w, 'a>(witness: &'w [&'a [u8]]) -> &'w [&'a [u8]] {
    match witness {
        [rest @ .., last] if !rest.is_empty() && last.first() == Some(&0x50) => rest,
        _ => witness,
    }
}

/// At least a script and a control block, the control block 33 + 32k bytes
/// (leaf version and internal key, then k Merkle path hashes) whose first byte
/// is the tapscript leaf version `0xc0` plus a parity bit.
fn is_p2tr_script_path(witness: &[&[u8]]) -> bool {
    let [_, .., control_block] = witness else {
        return false;
    };
    control_block.len() >= 33
        && (control_block.len() - 33).is_multiple_of(32)
        && control_block[0] & 0xfe == 0xc0
}

/// A Schnorr signature, then an annex (starting `0x50`).
fn is_p2tr_key_path_with_annex(witness: &[&[u8]]) -> bool {
    matches!(witness, [signature, annex]
        if is_schnorr_signature(signature) && annex.first() == Some(&0x50))
}

/// A P2TR script-path spend whose leaf is a **Single-key leaf**, with the parts
/// its Migration template changes.
struct SingleKeyLeafSpend<'a> {
    /// The witness items the leaf runs on, one of them the signature.
    stack: &'a [&'a [u8]],
    leaf: &'a [u8],
    merkle_path_hashes: usize,
}

impl SingleKeyLeafSpend<'_> {
    /// Bytes of the migrated witness `[stack items, leaf, control block]`: the
    /// signature becomes a PQ signature, the leaf's 32-byte key push becomes a
    /// PQ key push, and the control block drops the internal key, keeping the
    /// leaf-version byte and the Merkle path. Every other byte is unchanged.
    fn migrated_witness_size(&self, parameter_set: ParameterSet) -> u64 {
        let item = |len: u64| compact_size_len(len) + len;
        let stack: u64 = self
            .stack
            .iter()
            .map(|stack_item| {
                if is_schnorr_signature(stack_item) {
                    item(parameter_set.signature_size())
                } else {
                    item(stack_item.len() as u64)
                }
            })
            .sum();
        let public_key = parameter_set.public_key_size();
        let leaf = self.leaf.len() as u64 - (1 + 32) + push_opcode_len(public_key) + public_key;
        let control_block = 1 + 32 * self.merkle_path_hashes as u64;
        let item_count = compact_size_len(self.stack.len() as u64 + 2);
        item_count + stack + item(leaf) + item(control_block)
    }
}

/// A 64-byte BIP-340 Schnorr signature, or 65 with a sighash byte.
fn is_schnorr_signature(item: &[u8]) -> bool {
    (64..=65).contains(&item.len())
}

/// `OP_CHECKSIG` and `OP_CHECKSIGVERIFY`.
const OP_CHECKSIG: u8 = 0xac;
const OP_CHECKSIGVERIFY: u8 = 0xad;

/// Opcodes that check more than one key: `OP_CHECKMULTISIG`,
/// `OP_CHECKMULTISIGVERIFY` (disabled in tapscript) and `OP_CHECKSIGADD`
/// (`multi_a`).
fn is_multi_key_check(opcode: u8) -> bool {
    matches!(opcode, OP_CHECKMULTISIG | OP_CHECKMULTISIGVERIFY | 0xba)
}

/// BIP-342's `OP_SUCCESSx` opcodes: one anywhere in a leaf makes it succeed
/// without running, so no signature is checked.
fn is_op_success(opcode: u8) -> bool {
    matches!(
        opcode,
        0x50 | 0x62 | 0x7e..=0x81 | 0x83..=0x86 | 0x89..=0x8a | 0x8d..=0x8e | 0x95..=0x99 | 0xbb..=0xfe
    )
}

/// Reads `witness` as a script-path spend of a **Single-key leaf**: no annex, a
/// tapscript control block, a leaf whose only signature check is one directly
/// pushed 32-byte key (no multi-key check, no `OP_SUCCESSx`), and exactly one
/// Schnorr-signature-sized stack item.
fn single_key_leaf_spend<'a>(witness: &'a [&'a [u8]]) -> Option<SingleKeyLeafSpend<'a>> {
    if without_annex(witness).len() != witness.len() || !is_p2tr_script_path(witness) {
        return None;
    }
    let [stack @ .., leaf, control_block] = witness else {
        return None;
    };
    let ops = read_leaf_ops(leaf)?;
    let unsupported_opcode = ops.iter().any(|op| {
        matches!(op, LeafOp::Opcode(opcode) if is_multi_key_check(*opcode) || is_op_success(*opcode))
    });
    if unsupported_opcode {
        return None;
    }
    let mut checks = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| matches!(op, LeafOp::Opcode(OP_CHECKSIG | OP_CHECKSIGVERIFY)));
    let (check_index, _) = checks.next()?;
    let key_pushed_before_check = check_index > 0
        && matches!(ops[check_index - 1], LeafOp::DirectPush(key) if key.len() == 32);
    let one_signature = stack
        .iter()
        .filter(|stack_item| is_schnorr_signature(stack_item))
        .count()
        == 1;
    (checks.next().is_none() && key_pushed_before_check && one_signature).then_some(
        SingleKeyLeafSpend {
            stack,
            leaf,
            merkle_path_hashes: (control_block.len() - 33) / 32,
        },
    )
}

/// One element of a tapscript leaf or a contract's witnessScript. Pushes are
/// told apart by encoding because the templates only swap a key pushed
/// directly.
enum LeafOp<'a> {
    /// A push with opcode 0x01 to 0x4b.
    DirectPush(&'a [u8]),
    /// A push with `OP_PUSHDATA1`, `OP_PUSHDATA2` or `OP_PUSHDATA4`.
    PushData,
    Opcode(u8),
}

/// Splits a leaf or witnessScript into ops, with every push form a script may
/// use (inscription envelopes push data with `OP_PUSHDATA1` and
/// `OP_PUSHDATA2`). Returns `None` for a push that runs past the end.
fn read_leaf_ops(leaf: &[u8]) -> Option<Vec<LeafOp<'_>>> {
    let mut ops = Vec::new();
    let mut rest = leaf;
    while let Some((&opcode, tail)) = rest.split_first() {
        let (op, tail) = match opcode {
            0x01..=0x4b => {
                let (push, tail) = tail.split_at_checked(usize::from(opcode))?;
                (LeafOp::DirectPush(push), tail)
            }
            0x4c..=0x4e => {
                let (len, tail) = match opcode {
                    0x4c => {
                        let (&len, tail) = tail.split_first()?;
                        (usize::from(len), tail)
                    }
                    0x4d => {
                        let (len, tail) = tail.split_first_chunk::<2>()?;
                        (usize::from(u16::from_le_bytes(*len)), tail)
                    }
                    _ => {
                        let (len, tail) = tail.split_first_chunk::<4>()?;
                        (usize::try_from(u32::from_le_bytes(*len)).ok()?, tail)
                    }
                };
                let (_, tail) = tail.split_at_checked(len)?;
                (LeafOp::PushData, tail)
            }
            _ => (LeafOp::Opcode(opcode), tail),
        };
        ops.push(op);
        rest = tail;
    }
    Some(ops)
}

/// A P2WSH spend of a **Contract script**, with the parts its Migration
/// template changes.
struct ContractSpend<'a> {
    /// The witness items the script runs on.
    stack: &'a [&'a [u8]],
    script: &'a [u8],
}

impl ContractSpend<'_> {
    /// Whether the script contains `OP_CHECKMULTISIG` or
    /// `OP_CHECKMULTISIGVERIFY`, so the migration relies on PQ keeping that
    /// opcode's layout.
    fn uses_checkmultisig(&self) -> bool {
        read_leaf_ops(self.script).is_some_and(|ops| {
            ops.iter().any(|op| {
                matches!(
                    op,
                    LeafOp::Opcode(OP_CHECKMULTISIG | OP_CHECKMULTISIGVERIFY)
                )
            })
        })
    }

    /// Bytes of the migrated witness `[stack items, script]`: every signature
    /// becomes a PQ signature, every public key (on the stack or pushed in the
    /// script) a PQ public key. Every other byte is unchanged.
    fn migrated_witness_size(&self, parameter_set: ParameterSet) -> u64 {
        let item = |len: u64| compact_size_len(len) + len;
        let public_key = parameter_set.public_key_size();
        let stack: u64 = self
            .stack
            .iter()
            .map(|stack_item| {
                if is_strict_der_signature(stack_item) {
                    item(parameter_set.signature_size())
                } else if is_public_key(stack_item) {
                    item(public_key)
                } else {
                    item(stack_item.len() as u64)
                }
            })
            .sum();
        let script_keys = script_public_keys(self.script);
        let script = self.script.len() as u64
            - script_keys
                .iter()
                .map(|key| 1 + key.len() as u64)
                .sum::<u64>()
            + script_keys.len() as u64 * (push_opcode_len(public_key) + public_key);
        let item_count = compact_size_len(self.stack.len() as u64 + 1);
        item_count + stack + item(script)
    }
}

/// An ECDSA signature with BIP-66's DER structure: `30 <len> 02 <r len> <r>
/// 02 <s len> <s>`, every length consistent, `r` and `s` non-empty, then a
/// sighash byte. DER drops leading zero bytes of `r` and `s`, so real
/// signatures can be shorter than the usual 71 or 72 bytes. Every template
/// recognizes a signature this way: a contract's stack can hold data, and a
/// random 32-byte preimage starts with the `0x30` tag about once in 256, so
/// the tag and length alone aren't enough.
fn is_strict_der_signature(item: &[u8]) -> bool {
    let [0x30, sequence_len, 0x02, r_len, rest @ ..] = item else {
        return false;
    };
    let r_len = usize::from(*r_len);
    let Some([0x02, s_len, rest @ ..]) = rest.get(r_len..) else {
        return false;
    };
    let s_len = usize::from(*s_len);
    item.len() <= 73
        && r_len > 0
        && s_len > 0
        && rest.len() == s_len + 1
        && usize::from(*sequence_len) == 4 + r_len + s_len
}

/// A compressed (33 bytes, `02` or `03`) or uncompressed (65 bytes, `04`)
/// public key, by shape alone.
fn is_public_key(item: &[u8]) -> bool {
    matches!(
        (item.len(), item.first()),
        (33, Some(0x02 | 0x03)) | (65, Some(0x04))
    )
}

/// The public keys a script pushes directly, wherever they sit.
fn script_public_keys(script: &[u8]) -> Vec<&[u8]> {
    read_leaf_ops(script)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|op| match op {
            LeafOp::DirectPush(push) if is_public_key(push) => Some(push),
            _ => None,
        })
        .collect()
}

/// Reads `witness` as a spend of a **Contract script**: at least one stack item
/// and a script that parses, contains a signature check and has a public key in
/// it or on the stack. Standard multisig is matched before this, and a witness
/// shaped like a Taproot spend is never a contract, even when its control
/// block or annex happens to parse as one.
fn contract_spend<'a>(witness: &'a [&'a [u8]]) -> Option<ContractSpend<'a>> {
    let [stack @ .., script] = witness else {
        return None;
    };
    if stack.is_empty()
        || is_p2tr_script_path(without_annex(witness))
        || is_p2tr_key_path_with_annex(witness)
    {
        return None;
    }
    let ops = read_leaf_ops(script)?;
    let has_signature_check = ops.iter().any(|op| {
        matches!(
            op,
            LeafOp::Opcode(
                OP_CHECKSIG | OP_CHECKSIGVERIFY | OP_CHECKMULTISIG | OP_CHECKMULTISIGVERIFY
            )
        )
    });
    let has_public_key = !script_public_keys(script).is_empty()
        || stack.iter().any(|stack_item| is_public_key(stack_item));
    (has_signature_check && has_public_key).then_some(ContractSpend { stack, script })
}

/// A scriptSig of exactly a DER signature and a 33 or 65-byte public key.
fn is_der_signature_and_legacy_pubkey(pushes: &[&[u8]]) -> bool {
    matches!(pushes, [signature, pubkey]
        if is_strict_der_signature(signature) && matches!(pubkey.len(), 33 | 65))
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

/// A witness of exactly a DER signature (see [`is_strict_der_signature`])
/// and a 33-byte compressed public key. Structure only; nothing is verified.
fn is_der_signature_and_compressed_pubkey(witness: &[&[u8]]) -> bool {
    matches!(witness, [signature, pubkey]
        if is_strict_der_signature(signature) && pubkey.len() == 33)
}

/// The DER sequence tag `0x30` and 9 to 73 bytes, sighash byte included.
/// Deliberately looser than [`is_strict_der_signature`], which every template
/// uses: this only rules a push out as a redeem script when picking an
/// Unmapped reason, and a malformed signature is still more likely than a
/// script there.
fn looks_like_der_signature(item: &[u8]) -> bool {
    item.first() == Some(&0x30) && (9..=73).contains(&item.len())
}

/// A scriptSig that is a single push of exactly `0014<20-byte hash>` (22 bytes):
/// the P2SH-P2WPKH redeem script.
fn is_p2sh_p2wpkh_redeem_script_push(script_sig: &[u8]) -> bool {
    let [push_len, 0x00, 0x14, hash @ ..] = script_sig else {
        return false;
    };
    *push_len == 0x16 && hash.len() == 20
}

/// `OP_CHECKMULTISIG` and `OP_CHECKMULTISIGVERIFY`.
const OP_CHECKMULTISIG: u8 = 0xae;
const OP_CHECKMULTISIGVERIFY: u8 = 0xaf;

/// Public key sizes a multisig script may use. Segwit policy rejects
/// uncompressed keys, so only legacy P2SH allows them.
#[derive(Clone, Copy)]
enum KeySizes {
    /// 33 bytes.
    CompressedOnly,
    /// 33 or 65 bytes, mixed freely.
    CompressedOrUncompressed,
}

impl KeySizes {
    fn allows(self, len: usize) -> bool {
        match self {
            Self::CompressedOnly => len == 33,
            Self::CompressedOrUncompressed => matches!(len, 33 | 65),
        }
    }
}

/// The threshold of a spend whose stack items are `[dummy, m signatures, script]`,
/// where the dummy is empty, each signature is DER (see [`is_strict_der_signature`])
/// and the script is exactly `OP_m <n keys> OP_n OP_CHECKMULTISIG` with
/// keys of `key_sizes`. Lengths and structure only; nothing is verified.
fn multisig_threshold(items: &[&[u8]], key_sizes: KeySizes) -> Option<MultisigThreshold> {
    let [dummy, signatures @ .., script] = items else {
        return None;
    };
    let threshold = parse_multisig_script(script, key_sizes)?;
    let signatures_match = signatures.len() == usize::from(threshold.m)
        && signatures.iter().all(|s| is_strict_der_signature(s));
    (dummy.is_empty() && signatures_match).then_some(threshold)
}

/// The threshold of a bare multisig scriptPubKey: exactly `OP_m <n keys> OP_n
/// OP_CHECKMULTISIG` with 33 or 65-byte keys, read as a P2SH multisig script is.
pub(crate) fn bare_multisig_threshold(script: &[u8]) -> Option<MultisigThreshold> {
    parse_multisig_script(script, KeySizes::CompressedOrUncompressed)
}

/// Most keys a standard multisig script can hold (Bitcoin Core's
/// `MAX_PUBKEYS_PER_MULTISIG`).
const MAX_MULTISIG_KEYS: u8 = 20;

/// Reads `<m> <n keys> <n> OP_CHECKMULTISIG`, with 1 <= m <= n <= 20, `m` and
/// `n` minimally encoded, and every key a direct push of `key_sizes`.
fn parse_multisig_script(script: &[u8], key_sizes: KeySizes) -> Option<MultisigThreshold> {
    let ops = read_script_ops(script)?;
    let [m, keys @ .., n, ScriptOp::Opcode(OP_CHECKMULTISIG)] = ops.as_slice() else {
        return None;
    };
    let m = script_number(m)?;
    let n = script_number(n)?;
    let keys_match = keys.len() == usize::from(n)
        && keys
            .iter()
            .all(|key| matches!(key, ScriptOp::Push(k) if key_sizes.allows(k.len())));
    (1 <= m && m <= n && n <= MAX_MULTISIG_KEYS && keys_match).then_some(MultisigThreshold { m, n })
}

/// One element of a script: a direct data push, or any other single opcode.
#[derive(Clone, Copy)]
enum ScriptOp<'a> {
    Push(&'a [u8]),
    Opcode(u8),
}

/// Splits `script` into direct pushes (0x01 to 0x4b) and single-byte opcodes.
/// Returns `None` for `OP_PUSHDATA1/2/4`, which a multisig script never needs
/// for its keys, or a push that runs past the end of the script.
fn read_script_ops(script: &[u8]) -> Option<Vec<ScriptOp<'_>>> {
    let mut ops = Vec::new();
    let mut rest = script;
    while let Some((&opcode, tail)) = rest.split_first() {
        match opcode {
            0x01..=0x4b => {
                let (push, tail) = tail.split_at_checked(usize::from(opcode))?;
                ops.push(ScriptOp::Push(push));
                rest = tail;
            }
            0x4c..=0x4e => return None,
            _ => {
                ops.push(ScriptOp::Opcode(opcode));
                rest = tail;
            }
        }
    }
    Some(ops)
}

/// Reads a push-only scriptSig as its pushed items: `OP_0` pushes an empty
/// item, 0x01 to 0x4b push that many bytes, `OP_PUSHDATA1` and `OP_PUSHDATA2`
/// push the length that follows (1 or 2 bytes, little-endian). Returns `None`
/// for any other opcode or a push that runs past the end.
fn read_push_only(script_sig: &[u8]) -> Option<Vec<&[u8]>> {
    let mut pushes = Vec::new();
    let mut rest = script_sig;
    while let Some((&opcode, tail)) = rest.split_first() {
        let (len, tail) = match opcode {
            0x00..=0x4b => (usize::from(opcode), tail),
            0x4c => {
                let (&len, tail) = tail.split_first()?;
                (usize::from(len), tail)
            }
            0x4d => {
                let (len, tail) = tail.split_first_chunk::<2>()?;
                (usize::from(u16::from_le_bytes(*len)), tail)
            }
            _ => return None,
        };
        let (push, tail) = tail.split_at_checked(len)?;
        pushes.push(push);
        rest = tail;
    }
    Some(pushes)
}

/// A small script number in its minimal encoding: `OP_1` to `OP_16` (0x51 to
/// 0x60), or a 1-byte push of a value from 17 to 127.
fn script_number(op: &ScriptOp<'_>) -> Option<u8> {
    match *op {
        ScriptOp::Opcode(opcode @ 0x51..=0x60) => Some(opcode - 0x50),
        ScriptOp::Push(&[value @ 17..=0x7f]) => Some(value),
        _ => None,
    }
}

/// Bytes a script number takes in its minimal encoding (see [`script_number`]).
fn script_number_len(value: u8) -> u64 {
    if value <= 16 { 1 } else { 2 }
}

/// Bytes of the literal-swap multisig template witness
/// `[dummy, m PQ signatures, OP_m <n PQ keys> OP_n OP_CHECKMULTISIG]`: the item
/// count plus each item with its compact-size length prefix.
fn multisig_witness_size(threshold: MultisigThreshold, parameter_set: ParameterSet) -> u64 {
    let m = u64::from(threshold.m);
    let n = u64::from(threshold.n);
    let signature = parameter_set.signature_size();
    let public_key = parameter_set.public_key_size();

    let item_count = compact_size_len(m + 2);
    let dummy = 1;
    let signatures = m * (compact_size_len(signature) + signature);
    let script_len = script_number_len(threshold.m)
        + n * (push_opcode_len(public_key) + public_key)
        + script_number_len(threshold.n)
        + 1;
    let script = compact_size_len(script_len) + script_len;
    item_count + dummy + signatures + script
}

/// Bytes of the opcode that pushes `len` bytes inside a script: a direct push up
/// to 75, then `OP_PUSHDATA1`, `OP_PUSHDATA2` and `OP_PUSHDATA4` with their
/// 1, 2 and 4-byte lengths.
fn push_opcode_len(len: u64) -> u64 {
    match len {
        0..=0x4b => 1,
        0x4c..=0xff => 2,
        0x100..=0xffff => 3,
        _ => 5,
    }
}

/// A scriptSig that is a single push of exactly `0020<32-byte hash>` (34 bytes):
/// the P2SH-P2WSH redeem script.
fn is_p2sh_p2wsh_redeem_script_push(script_sig: &[u8]) -> bool {
    let [push_len, 0x00, 0x20, hash @ ..] = script_sig else {
        return false;
    };
    *push_len == 0x22 && hash.len() == 32
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

/// Serialized bytes of a witness: its item count, then each item with its
/// length prefix.
fn witness_size(witness: &[&[u8]]) -> u64 {
    let items = witness
        .iter()
        .map(|item| compact_size_len(item.len() as u64) + item.len() as u64);
    compact_size_len(witness.len() as u64) + items.sum::<u64>()
}

pub(crate) fn compact_size_len(value: u64) -> u64 {
    match value {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}
