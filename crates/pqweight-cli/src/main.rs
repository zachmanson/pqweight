use std::io::Read;
use std::process::ExitCode;

use pqweight::{BaselineSpendType, FeeRate, InputResult, Migration, ParameterSet, fee};

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
        _ => Err(USAGE.to_string()),
    }
}

const USAGE: &str = "usage: pqweight weight [--json] [<hex>]\n       pqweight migrate --scheme <scheme> [--fee-rate <rate>] [--json] [<hex>]";

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

fn run_migrate(args: &[String]) -> Result<String, String> {
    let mut scheme = None;
    let mut fee_rate_arg = None;
    let mut json = false;
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
            "--json" => json = true,
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
    }
}

fn migrate_human(migration: &Migration, baseline_vsize: u64, fee_rate: Option<FeeRate>) -> String {
    let mut lines = Vec::new();
    for (i, input) in migration.inputs.iter().enumerate() {
        lines.push(match input {
            InputResult::Mapped {
                spend_type,
                template_weight,
            } => format!(
                "input {i}: mapped ({}), weight: {template_weight}",
                spend_type_name(*spend_type)
            ),
            InputResult::Unmapped => format!("input {i}: unmapped"),
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
            } => format!(
                r#"{{"status":"mapped","spend_type":"{}","template_weight":{template_weight}}}"#,
                spend_type_name(*spend_type)
            ),
            InputResult::Unmapped => r#"{"status":"unmapped"}"#.to_string(),
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
