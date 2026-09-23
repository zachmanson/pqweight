use std::io::Read;
use std::process::ExitCode;

use pqweight::{
    AggregateResult, BaselineSpendType, BreakdownKind, BreakdownRow, FeeRate, InputResult,
    Migration, MultisigThreshold, ParameterSet, UnmappedReason, aggregate, fee,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("pqweight: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("weight") => run_weight(&args[1..]),
        Some("migrate") => run_migrate(&args[1..]),
        Some("aggregate") => run_aggregate(&args[1..]),
        _ => Err(USAGE.to_string()),
    }
}

const USAGE: &str = "usage: pqweight weight [--json] [<hex>]\n       pqweight migrate --scheme <scheme> [--fee-rate <rate>] [--json] [<hex>]\n       pqweight aggregate --scheme <scheme> [--fee-rate <rate>] [<path>]";

fn run_weight(args: &[String]) -> Result<String, String> {
    let json = args.iter().any(|arg| arg == "--json");
    let positional: Vec<&String> = args.iter().filter(|arg| *arg != "--json").collect();
    let hex = match positional.as_slice() {
        [hex] => (*hex).clone(),
        [] => read_stdin()?,
        _ => return Err(USAGE.to_string()),
    };
    let bytes = decode_hex(hex.trim())?;
    let result = pqweight::transaction_weight(&bytes).map_err(|err| err.to_string())?;
    if json {
        Ok(format!(
            r#"{{"weight":{},"vsize":{},"stripped_size":{},"total_size":{}}}"#,
            result.weight, result.vsize, result.stripped_size, result.total_size
        ))
    } else {
        Ok(format!(
            "weight: {}\nvsize: {}\nstripped size: {}\ntotal size: {}",
            result.weight, result.vsize, result.stripped_size, result.total_size
        ))
    }
}

/// Parses the `--scheme <scheme>` and `--fee-rate <rate>` flags `migrate` and
/// `aggregate` share, returning the parsed values and the remaining
/// (non-flag) arguments.
fn parse_scheme_and_fee_rate_args(
    args: &[String],
) -> Result<(ParameterSet, Option<FeeRate>, Vec<&str>), String> {
    let mut scheme = None;
    let mut fee_rate_arg = None;
    let mut positional = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--scheme" => {
                scheme = Some(iter.next().ok_or("--scheme needs a value")?.as_str());
            }
            "--fee-rate" => {
                fee_rate_arg = Some(iter.next().ok_or("--fee-rate needs a value")?.as_str());
            }
            other => positional.push(other),
        }
    }
    let scheme = scheme.ok_or("--scheme is required")?;
    let parameter_set = parse_scheme(scheme)?;
    let fee_rate = fee_rate_arg
        .map(|rate| {
            FeeRate::parse(rate).map_err(|err| format!("invalid fee rate {rate:?}: {err:?}"))
        })
        .transpose()?;
    Ok((parameter_set, fee_rate, positional))
}

fn run_migrate(args: &[String]) -> Result<String, String> {
    let json = args.iter().any(|arg| arg == "--json");
    let args: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--json")
        .cloned()
        .collect();
    let (parameter_set, fee_rate, positional) = parse_scheme_and_fee_rate_args(&args)?;

    let hex = match positional.as_slice() {
        [hex] => (*hex).to_string(),
        [] => read_stdin()?,
        _ => return Err(USAGE.to_string()),
    };
    let bytes = decode_hex(hex.trim())?;
    let baseline = pqweight::transaction_weight(&bytes).map_err(|err| err.to_string())?;
    let migration = pqweight::migrate(&bytes, parameter_set).map_err(|err| err.to_string())?;

    if json {
        Ok(migrate_json(&migration, baseline.vsize, fee_rate))
    } else {
        Ok(migrate_human(&migration, baseline.vsize, fee_rate))
    }
}

fn run_aggregate(args: &[String]) -> Result<String, String> {
    let (parameter_set, fee_rate, positional) = parse_scheme_and_fee_rate_args(args)?;

    let text = match positional.as_slice() {
        [path] => {
            std::fs::read_to_string(path).map_err(|err| format!("could not read {path}: {err}"))?
        }
        [] => read_stdin()?,
        _ => return Err(USAGE.to_string()),
    };

    let result = aggregate(text.lines().map(str::to_string), parameter_set, fee_rate);
    Ok(aggregate_human(&result))
}

fn aggregate_human(result: &AggregateResult) -> String {
    let mut lines = vec![
        format!("transactions parsed: {}", result.counts.parsed),
        format!("fully mapped: {}", result.counts.fully_mapped),
        format!("partially mapped: {}", result.counts.partially_mapped),
        format!("unmapped inputs: {}", result.counts.unmapped_inputs),
        format!("parse errors: {}", result.counts.parse_errors),
        format!("baseline weight: {}", result.baseline.weight),
        format!("baseline vsize: {}", result.baseline.vsize),
    ];
    match &result.migrated {
        Some(total) => {
            lines.push(format!("migrated weight: {}", total.weight));
            lines.push(format!("migrated vsize: {}", total.vsize));
        }
        None => {
            lines.push("migrated total: unavailable (no fully mapped transactions)".to_string());
        }
    }
    if let Some(fees) = &result.fees {
        lines.push(format!("baseline fee: {} sat", fees.baseline));
        match fees.migrated {
            Some(migrated_fee) => lines.push(format!("migrated fee: {migrated_fee} sat")),
            None => {
                lines.push("migrated fee: unavailable (no fully mapped transactions)".to_string());
            }
        }
    }
    lines.push(format!(
        "partially mapped baseline weight: {} ({} of baseline)",
        result.partially_mapped.weight,
        percent(result.partially_mapped.weight, result.baseline.weight)
    ));
    lines.extend(breakdown_table(&result.breakdown));
    if !result.errors.is_empty() {
        lines.push("errors:".to_string());
        for error in &result.errors {
            lines.push(format!("- line {}: {}", error.line, error.message));
        }
    }
    lines.join("\n")
}

/// The breakdown as two sections, Mapped rows then Unmapped rows, each sorted
/// by baseline Input weight, largest first.
fn breakdown_table(breakdown: &[BreakdownRow]) -> Vec<String> {
    let total_inputs: usize = breakdown.iter().map(|row| row.inputs).sum();
    let total_weight: u64 = breakdown.iter().map(|row| row.baseline_weight).sum();
    let label = |row: &BreakdownRow| match row.kind {
        BreakdownKind::Mapped(spend_type) => spend_type_label(spend_type),
        BreakdownKind::Unmapped(reason) => unmapped_reason_name(reason).to_string(),
    };
    let width = breakdown
        .iter()
        .map(|row| label(row).len())
        .max()
        .unwrap_or(0);

    let mut lines = vec![
        "breakdown (inputs, % of inputs, baseline input weight, % of it, migrated input weight):"
            .to_string(),
    ];
    for (heading, mapped) in [("mapped:", true), ("unmapped:", false)] {
        lines.push(heading.to_string());
        let mut rows: Vec<&BreakdownRow> = breakdown
            .iter()
            .filter(|row| matches!(row.kind, BreakdownKind::Mapped(_)) == mapped)
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.baseline_weight));
        if rows.is_empty() {
            lines.push("  (none)".to_string());
        }
        for row in rows {
            let migrated = row
                .migrated_weight
                .map_or_else(|| "-".to_string(), |weight| weight.to_string());
            lines.push(format!(
                "  {:<width$}  {:>8}  {:>6}  {:>12}  {:>6}  {:>12}",
                label(row),
                row.inputs,
                percent(row.inputs as u64, total_inputs as u64),
                row.baseline_weight,
                percent(row.baseline_weight, total_weight),
                migrated,
            ));
        }
    }
    lines
}

/// `part` as a percentage of `whole` to one decimal place, such as `64.4%`.
fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "-".to_string();
    }
    // Display only: counts and weights this large (over 2^52) never occur, so
    // the precision loss doesn't affect the one decimal shown.
    #[allow(clippy::cast_precision_loss)]
    let ratio = part as f64 / whole as f64;
    format!("{:.1}%", ratio * 100.0)
}

fn unmapped_reason_name(reason: UnmappedReason) -> &'static str {
    match reason {
        UnmappedReason::P2trScriptPath => "P2TR script-path",
        UnmappedReason::P2trKeyPathAnnex => "P2TR key-path with annex",
        UnmappedReason::P2wshNonMultisig => "P2WSH non-multisig",
        UnmappedReason::P2shSegwitNonMultisig => "P2SH-wrapped segwit non-multisig",
        UnmappedReason::P2shNonMultisig => "P2SH non-multisig",
        UnmappedReason::LegacyOther => "legacy other",
        UnmappedReason::Unknown => "unknown",
    }
}

fn parse_scheme(scheme: &str) -> Result<ParameterSet, String> {
    match scheme {
        "ml-dsa-44" => Ok(ParameterSet::MlDsa44),
        "falcon-512" => Ok(ParameterSet::Falcon512),
        "slh-dsa-128s" => Ok(ParameterSet::SlhDsa128s),
        other => Err(format!(
            "unknown --scheme {other:?}; expected one of ml-dsa-44, falcon-512, slh-dsa-128s"
        )),
    }
}

fn spend_type_name(spend_type: BaselineSpendType) -> &'static str {
    match spend_type {
        BaselineSpendType::P2wpkh => "P2WPKH",
        BaselineSpendType::P2trKeyPath => "P2TR key-path",
        BaselineSpendType::P2shP2wpkh => "P2SH-P2WPKH",
        BaselineSpendType::P2pkh => "P2PKH",
        BaselineSpendType::PayToAnchor => "pay-to-anchor",
        BaselineSpendType::Coinbase => "coinbase",
        BaselineSpendType::P2wshMultisig(_) => "P2WSH multisig",
        BaselineSpendType::P2shP2wshMultisig(_) => "P2SH-P2WSH multisig",
        BaselineSpendType::P2shMultisig(_) => "P2SH multisig",
    }
}

/// The spend type's name with its threshold, such as `P2WSH multisig 2-of-3`.
fn spend_type_label(spend_type: BaselineSpendType) -> String {
    match spend_type.threshold() {
        Some(MultisigThreshold { m, n }) => format!("{} {m}-of-{n}", spend_type_name(spend_type)),
        None => spend_type_name(spend_type).to_string(),
    }
}

fn migrate_human(migration: &Migration, baseline_vsize: u64, fee_rate: Option<FeeRate>) -> String {
    let mut lines = Vec::new();
    for (i, input) in migration.inputs.iter().enumerate() {
        lines.push(match input {
            InputResult::Mapped {
                spend_type,
                template_weight,
                ..
            } => format!(
                "input {i}: mapped ({}), weight: {template_weight}",
                spend_type_label(*spend_type)
            ),
            InputResult::Unmapped { reason, .. } => {
                format!("input {i}: unmapped ({})", unmapped_reason_name(*reason))
            }
        });
    }
    match &migration.migrated {
        Some(total) => {
            lines.push(format!("migrated weight: {}", total.weight));
            lines.push(format!("migrated vsize: {}", total.vsize));
            lines.push(format!(
                "exceeds relay limit: {}",
                if migration.exceeds_relay_limit == Some(true) {
                    "yes"
                } else {
                    "no"
                }
            ));
            if let Some(rate) = fee_rate {
                let baseline_fee = fee(baseline_vsize, rate);
                let pq_fee = fee(total.vsize, rate);
                lines.push(format!("baseline fee: {baseline_fee} sat"));
                lines.push(format!("pq fee: {pq_fee} sat"));
                lines.push(format!(
                    "fee difference: {} sat",
                    pq_fee.abs_diff(baseline_fee)
                ));
                if baseline_fee > 0 {
                    // Display only: fees this large (over 2^52 sats) never occur
                    // in practice, so the precision loss doesn't affect the
                    // ratio shown.
                    #[allow(clippy::cast_precision_loss)]
                    let ratio = pq_fee as f64 / baseline_fee as f64;
                    lines.push(format!("fee ratio: {ratio:.2}"));
                }
            }
        }
        None => lines.push("migrated total: unavailable (not every input is mapped)".to_string()),
    }
    lines.push("assumptions:".to_string());
    for assumption in &migration.assumptions {
        lines.push(format!("- {assumption}"));
    }
    lines.join("\n")
}

fn migrate_json(migration: &Migration, baseline_vsize: u64, fee_rate: Option<FeeRate>) -> String {
    let inputs: Vec<String> = migration
        .inputs
        .iter()
        .map(|input| match input {
            InputResult::Mapped {
                spend_type,
                template_weight,
                ..
            } => {
                let threshold = match spend_type.threshold() {
                    Some(MultisigThreshold { m, n }) => {
                        format!(r#","threshold":{{"m":{m},"n":{n}}}"#)
                    }
                    None => String::new(),
                };
                format!(
                    r#"{{"status":"mapped","spend_type":"{}"{threshold},"template_weight":{template_weight}}}"#,
                    spend_type_name(*spend_type)
                )
            }
            InputResult::Unmapped { reason, .. } => format!(
                r#"{{"status":"unmapped","reason":"{}"}}"#,
                unmapped_reason_name(*reason)
            ),
        })
        .collect();

    let migrated = match &migration.migrated {
        Some(total) => format!(
            r#"{{"weight":{},"vsize":{},"stripped_size":{},"total_size":{}}}"#,
            total.weight, total.vsize, total.stripped_size, total.total_size
        ),
        None => "null".to_string(),
    };

    let exceeds_relay_limit = match migration.exceeds_relay_limit {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };

    let assumptions: Vec<String> = migration
        .assumptions
        .iter()
        .map(|a| format!("{a:?}"))
        .collect();

    let fee_field = match (fee_rate, &migration.migrated) {
        (Some(rate), Some(total)) => {
            let baseline_fee = fee(baseline_vsize, rate);
            let pq_fee = fee(total.vsize, rate);
            format!(
                r#","fee":{{"baseline":{baseline_fee},"pq":{pq_fee},"difference":{}}}"#,
                pq_fee.abs_diff(baseline_fee)
            )
        }
        _ => String::new(),
    };

    format!(
        r#"{{"inputs":[{}],"migrated":{migrated},"exceeds_relay_limit":{exceeds_relay_limit},"assumptions":[{}]{fee_field}}}"#,
        inputs.join(","),
        assumptions.join(","),
    )
}

fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|err| format!("could not read stdin: {err}"))?;
    Ok(input)
}

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
