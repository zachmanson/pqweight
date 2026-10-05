use std::fmt::Write as _;
use std::io::Read;
use std::process::ExitCode;

use pqweight::{
    AggregateResult, BaselineSpendType, BlockRow, BreakdownKind, BreakdownRow, ExposedType,
    ExposureRow, FeeRate, Hash256, InputResult, KeyExposure, MAX_BLOCK_WEIGHT,
    MOVE_COST_ASSUMPTIONS, Migration, MoveCost, MoveCostRow, MoveCostTotals, MultisigThreshold,
    ParameterSet, RELAY_WEIGHT_LIMIT, SnapshotHeader, TransactionWeight, UnmappedReason, aggregate,
    aggregate_blocks, fee, read_snapshot,
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
        Some("move-cost") => run_move_cost(&args[1..]),
        Some("split-blocks") => run_split_blocks(&args[1..]),
        _ => Err(USAGE.to_string()),
    }
}

const USAGE: &str = "usage: pqweight weight [--json] [<hex>]\n       pqweight migrate --scheme <scheme> [--fee-rate <rate>] [--json] [<hex>]\n       pqweight migrate --scheme <scheme> [--fee-rate <rate>] --json-lines [<path>]\n       pqweight aggregate --scheme <scheme> [--fee-rate <rate>] [--json] [--blocks] [<path>]\n       pqweight move-cost [--json] <snapshot>\n       pqweight split-blocks [<path>]";

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
        Ok(weight_json(&result))
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
    let json_lines = args.iter().any(|arg| arg == "--json-lines");
    let args: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--json" && *arg != "--json-lines")
        .cloned()
        .collect();
    let (parameter_set, fee_rate, positional) = parse_scheme_and_fee_rate_args(&args)?;

    if json_lines {
        let text = read_path_or_stdin(&positional)?;
        return Ok(migrate_json_lines(&text, parameter_set, fee_rate));
    }

    let hex = match positional.as_slice() {
        [hex] => (*hex).to_string(),
        [] => read_stdin()?,
        _ => return Err(USAGE.to_string()),
    };
    let (baseline, migration) = migrate_one(&hex, parameter_set)?;
    if json {
        Ok(migrate_json(None, &baseline, &migration, fee_rate))
    } else {
        Ok(migrate_human(
            &baseline,
            &migration,
            parameter_set,
            fee_rate,
        ))
    }
}

/// `migrate --json` over every non-blank line of `text`, one output line each,
/// with `"line":N` (N 1-indexed, counting blank lines) as its first field. A
/// line that fails prints `{"line":N,"error":"..."}` and the batch continues,
/// as in `aggregate`.
fn migrate_json_lines(
    text: &str,
    parameter_set: ParameterSet,
    fee_rate: Option<FeeRate>,
) -> String {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| match migrate_one(line, parameter_set) {
            Ok((baseline, migration)) => {
                migrate_json(Some(index + 1), &baseline, &migration, fee_rate)
            }
            Err(message) => format!(
                r#"{{"line":{},"error":{}}}"#,
                index + 1,
                json_string(&message)
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One transaction's weight today and its migration.
fn migrate_one(
    hex: &str,
    parameter_set: ParameterSet,
) -> Result<(TransactionWeight, Migration), String> {
    let bytes = decode_hex(hex.trim())?;
    let baseline = pqweight::transaction_weight(&bytes).map_err(|err| err.to_string())?;
    let migration = pqweight::migrate(&bytes, parameter_set).map_err(|err| err.to_string())?;
    Ok((baseline, migration))
}

fn run_aggregate(args: &[String]) -> Result<String, String> {
    let json = args.iter().any(|arg| arg == "--json");
    let blocks = args.iter().any(|arg| arg == "--blocks");
    let args: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--json" && *arg != "--blocks")
        .cloned()
        .collect();
    let (parameter_set, fee_rate, positional) = parse_scheme_and_fee_rate_args(&args)?;

    let text = read_path_or_stdin(&positional)?;

    let lines = text.lines().map(str::to_string);
    let result = if blocks {
        aggregate_blocks(lines, parameter_set, fee_rate)
    } else {
        aggregate(lines, parameter_set, fee_rate)
    };
    if json {
        Ok(aggregate_json(&result, parameter_set, fee_rate, blocks))
    } else {
        Ok(aggregate_human(&result, blocks))
    }
}

/// One raw block per line in, verified by `parse_block`; one transaction hex
/// per line out, the input `aggregate` and the Python scripts take. Each
/// block's hash and transaction count go to stderr. A block that fails stops
/// the command before anything is printed, so nothing downstream reads a
/// sample with a block quietly missing.
fn run_split_blocks(args: &[String]) -> Result<String, String> {
    let positional: Vec<&str> = args.iter().map(String::as_str).collect();
    let text = read_path_or_stdin(&positional)?;
    let mut transactions = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let hex = line.trim();
        if hex.is_empty() {
            continue;
        }
        let block = decode_hex(hex)
            .and_then(|bytes| pqweight::parse_block(&bytes).map_err(|err| err.to_string()))
            .map_err(|message| format!("line {}: {message}", index + 1))?;
        eprintln!("{}: {} transactions", block.hash, block.transactions.len());
        transactions.extend(block.transactions.iter().map(|tx| to_hex(tx)));
    }
    Ok(transactions.join("\n"))
}

fn run_move_cost(args: &[String]) -> Result<String, String> {
    let json = args.iter().any(|arg| arg == "--json");
    let positional: Vec<&String> = args.iter().filter(|arg| *arg != "--json").collect();
    let [path] = positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let file = std::fs::File::open(path).map_err(|err| format!("could not read {path}: {err}"))?;
    let snapshot = read_snapshot(file).map_err(|err| format!("{path}: {err}"))?;
    let header = snapshot.header;
    let mut cost = MoveCost::new();
    for coin in snapshot {
        cost.add(&coin.map_err(|err| format!("{path}: {err}"))?);
    }
    if json {
        Ok(move_cost_json(&header, &cost))
    } else {
        Ok(move_cost_human(&header, &cost))
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        // Writing to a String never fails.
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn exposed_type_name(exposed_type: ExposedType) -> &'static str {
    match exposed_type {
        ExposedType::P2pkCompressed => "P2PK compressed",
        ExposedType::P2pkUncompressed => "P2PK uncompressed",
        ExposedType::BareMultisig(_) => "bare multisig",
        ExposedType::P2tr => "P2TR",
    }
}

/// The type's name with its threshold, such as `bare multisig 1-of-3`.
fn exposed_type_label(exposed_type: ExposedType) -> String {
    match exposed_type {
        ExposedType::BareMultisig(MultisigThreshold { m, n }) => {
            format!("{} {m}-of-{n}", exposed_type_name(exposed_type))
        }
        _ => exposed_type_name(exposed_type).to_string(),
    }
}

/// Satoshis as BTC with all 8 decimal places.
fn btc(sats: u64) -> String {
    format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000)
}

fn move_cost_totals_json(totals: &MoveCostTotals) -> String {
    format!(
        r#"{{"coins":{},"value":{},"floor_weight":{},"floor_blocks":{},"ceiling_weight":{},"ceiling_blocks":{}}}"#,
        totals.coins,
        totals.value,
        totals.floor_weight,
        totals.floor_blocks(),
        totals.ceiling_weight,
        totals.ceiling_blocks(),
    )
}

/// `move-cost --json`. Values are in satoshis.
fn move_cost_json(header: &SnapshotHeader, cost: &MoveCost) -> String {
    let rows: Vec<String> = cost
        .rows
        .iter()
        .map(|row| {
            let threshold = match row.exposed_type {
                ExposedType::BareMultisig(MultisigThreshold { m, n }) => {
                    format!(r#","threshold":{{"m":{m},"n":{n}}}"#)
                }
                _ => String::new(),
            };
            format!(
                r#"{{"exposed_type":"{}"{threshold},"all":{},"above_dust":{}}}"#,
                exposed_type_name(row.exposed_type),
                move_cost_totals_json(&row.all),
                move_cost_totals_json(&row.above_dust),
            )
        })
        .collect();
    let (all, above_dust) = cost.total();
    let assumptions: Vec<String> = MOVE_COST_ASSUMPTIONS
        .iter()
        .map(|a| json_string(a))
        .collect();
    format!(
        r#"{{"snapshot":{{"base_block_hash":"{}","coins":{}}},"scanned":{{"coins":{},"value":{}}},"rows":[{}],"total":{{"all":{},"above_dust":{}}},"assumptions":[{}]}}"#,
        Hash256(header.base_block_hash),
        header.coins_count,
        cost.scanned_coins,
        cost.scanned_value,
        rows.join(","),
        move_cost_totals_json(&all),
        move_cost_totals_json(&above_dust),
        assumptions.join(","),
    )
}

fn move_cost_human(header: &SnapshotHeader, cost: &MoveCost) -> String {
    let mut lines = vec![
        format!("snapshot base block: {}", Hash256(header.base_block_hash)),
        format!(
            "coins scanned: {} ({} BTC)",
            cost.scanned_coins,
            btc(cost.scanned_value)
        ),
    ];
    let width = cost
        .rows
        .iter()
        .map(|row| exposed_type_label(row.exposed_type).len())
        .chain(["total".len()])
        .max()
        .unwrap_or(0);
    let line = |label: &str, totals: &MoveCostTotals| {
        format!(
            "  {label:<width$}  {:>10}  {:>20}  {:>14}  {:>10.2}  {:>14}  {:>10.2}",
            totals.coins,
            btc(totals.value),
            totals.floor_weight,
            totals.floor_blocks(),
            totals.ceiling_weight,
            totals.ceiling_blocks(),
        )
    };
    let (all, above_dust) = cost.total();
    let mut section =
        |heading: &str, total: &MoveCostTotals, pick: fn(&MoveCostRow) -> MoveCostTotals| {
            lines.push(heading.to_string());
            if cost.rows.is_empty() {
                lines.push("  (none)".to_string());
            }
            for row in &cost.rows {
                lines.push(line(&exposed_type_label(row.exposed_type), &pick(row)));
            }
            lines.push(line("total", total));
        };
    section(
        "all Exposed coins (coins, BTC, floor WU, floor blocks, ceiling WU, ceiling blocks):",
        &all,
        |row| row.all,
    );
    section("coins of at least 546 sats:", &above_dust, |row| {
        row.above_dust
    });
    lines.push("assumptions:".to_string());
    for assumption in MOVE_COST_ASSUMPTIONS {
        lines.push(format!("- {assumption}"));
    }
    lines.join("\n")
}

/// `aggregate --json`: the same numbers as the human output, with the scheme and
/// fee rate echoed so a saved file says what produced it.
fn aggregate_json(
    result: &AggregateResult,
    parameter_set: ParameterSet,
    fee_rate: Option<FeeRate>,
    blocks: bool,
) -> String {
    let totals = |weight: u64, vsize: u64| format!(r#"{{"weight":{weight},"vsize":{vsize}}}"#);
    let counts = &result.counts;
    let migrated = result.migrated.map_or_else(
        || "null".to_string(),
        |total| totals(total.weight, total.vsize),
    );
    let fee_field = result.fees.map_or_else(String::new, |fees| {
        let migrated = fees
            .migrated
            .map_or_else(|| "null".to_string(), |fee| fee.to_string());
        format!(
            r#","fee":{{"baseline":{},"migrated":{migrated}}}"#,
            fees.baseline
        )
    });
    let breakdown: Vec<String> = sorted_breakdown(&result.breakdown)
        .into_iter()
        .map(breakdown_row_json)
        .collect();
    let exposure: Vec<String> = result
        .exposure
        .iter()
        .map(|row| {
            format!(
                r#"{{"key_exposure":"{}","mapped_inputs":{},"baseline_weight":{},"migrated_weight":{},"added_weight":{},"unmapped_inputs":{},"unmapped_baseline_weight":{}}}"#,
                key_exposure_name(row.key_exposure),
                row.mapped_inputs,
                row.baseline_weight,
                row.migrated_weight,
                row.added_weight(),
                row.unmapped_inputs,
                row.unmapped_baseline_weight,
            )
        })
        .collect();
    let errors: Vec<String> = result
        .errors
        .iter()
        .map(|error| {
            format!(
                r#"{{"line":{},"message":{}}}"#,
                error.line,
                json_string(&error.message)
            )
        })
        .collect();
    let blocks_field = if blocks {
        let rows: Vec<String> = result.blocks.iter().map(block_row_json).collect();
        format!(r#","blocks":[{}]"#, rows.join(","))
    } else {
        String::new()
    };
    format!(
        r#"{{"scheme":"{}","fee_rate":{},"counts":{{"parsed":{},"fully_mapped":{},"partially_mapped":{},"unmapped_inputs":{},"parse_errors":{}}},"baseline":{},"migrated":{migrated},"partially_mapped":{}{fee_field},"breakdown":[{}],"key_exposure":[{}]{blocks_field},"errors":[{}]}}"#,
        scheme_name(parameter_set),
        fee_rate.map_or_else(|| "null".to_string(), |rate| rate.to_string()),
        counts.parsed,
        counts.fully_mapped,
        counts.partially_mapped,
        counts.unmapped_inputs,
        counts.parse_errors,
        totals(result.baseline.weight, result.baseline.vsize),
        totals(
            result.partially_mapped.weight,
            result.partially_mapped.vsize
        ),
        breakdown.join(","),
        exposure.join(","),
        errors.join(","),
    )
}

/// `weight` as an exact decimal multiple of [`MAX_BLOCK_WEIGHT`]: weight x 25
/// / 10^8, so at most 8 decimal places and no float rounding.
fn limit_multiple(weight: u64) -> String {
    let scaled = weight * 25;
    let fraction = format!("{:08}", scaled % 100_000_000);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        (scaled / 100_000_000).to_string()
    } else {
        format!("{}.{fraction}", scaled / 100_000_000)
    }
}

/// `weight` as a multiple of [`MAX_BLOCK_WEIGHT`] to one decimal place,
/// rounded half up, such as `x7.9`.
fn limit_multiple_label(weight: u64) -> String {
    let tenths = (weight * 10 + MAX_BLOCK_WEIGHT / 2) / MAX_BLOCK_WEIGHT;
    format!("x{}.{}", tenths / 10, tenths % 10)
}

/// One `aggregate --blocks --json` block row.
fn block_row_json(row: &BlockRow) -> String {
    let null = || "null".to_string();
    format!(
        r#"{{"hash":"{}","transactions":{},"weight":{},"migrated_weight":{},"migrated_limit_multiple":{},"partially_mapped":{}}}"#,
        row.hash,
        row.transactions,
        row.weight,
        row.migrated_weight
            .map_or_else(null, |weight| weight.to_string()),
        row.migrated_weight.map_or_else(null, limit_multiple),
        row.partially_mapped,
    )
}

/// The `aggregate --blocks` table: one row per block, then what a migrated
/// weight over the limit means.
fn blocks_table(blocks: &[BlockRow]) -> Vec<String> {
    let mut lines = vec![
        "blocks (transactions, Block weight, migrated Block weight, x the 4,000,000 WU limit):"
            .to_string(),
    ];
    if blocks.is_empty() {
        lines.push("  (none)".to_string());
    }
    for row in blocks {
        let migrated = match row.migrated_weight {
            Some(weight) => format!("{:>9}  {}", weight, limit_multiple_label(weight)),
            None => format!("{:>9}  ({} partially mapped)", "-", row.partially_mapped),
        };
        lines.push(format!(
            "  {}  {:>6}  {:>9}  {migrated}",
            row.hash, row.transactions, row.weight
        ));
    }
    lines.push(
        "over the limit means these transactions wouldn't fit in one block, not that the block is invalid; \"-\" means a partially mapped transaction leaves the block without a migrated weight"
            .to_string(),
    );
    lines
}

/// One breakdown row as JSON, named the way `migrate --json` names its inputs.
fn breakdown_row_json(row: &BreakdownRow) -> String {
    match row.kind {
        BreakdownKind::Mapped(spend_type) => format!(
            r#"{{"status":"mapped","spend_type":"{}"{},"key_exposure":"{}","inputs":{},"baseline_weight":{},"migrated_weight":{},"added_weight":{}}}"#,
            spend_type_name(spend_type),
            threshold_field(spend_type),
            key_exposure_name(spend_type.key_exposure()),
            row.inputs,
            row.baseline_weight,
            row.migrated_weight
                .expect("a Mapped row has a migrated weight"),
            row.added_weight()
                .expect("a Mapped row has an Added weight"),
        ),
        BreakdownKind::Unmapped(reason) => format!(
            r#"{{"status":"unmapped","reason":"{}","key_exposure":"{}","inputs":{},"baseline_weight":{}}}"#,
            unmapped_reason_name(reason),
            key_exposure_name(reason.key_exposure()),
            row.inputs,
            row.baseline_weight,
        ),
    }
}

/// `,"threshold":{"m":M,"n":N}` for a multisig spend type, nothing otherwise.
fn threshold_field(spend_type: BaselineSpendType) -> String {
    match spend_type.threshold() {
        Some(MultisigThreshold { m, n }) => format!(r#","threshold":{{"m":{m},"n":{n}}}"#),
        None => String::new(),
    }
}

/// The breakdown in report order: Mapped rows by Added weight, then Unmapped
/// rows (which have none) by baseline Input weight, largest first in both.
fn sorted_breakdown(breakdown: &[BreakdownRow]) -> Vec<&BreakdownRow> {
    let (mut mapped, mut unmapped): (Vec<&BreakdownRow>, Vec<&BreakdownRow>) = breakdown
        .iter()
        .partition(|row| matches!(row.kind, BreakdownKind::Mapped(_)));
    mapped.sort_by_key(|row| std::cmp::Reverse(row.added_weight()));
    unmapped.sort_by_key(|row| std::cmp::Reverse(row.baseline_weight));
    mapped.extend(unmapped);
    mapped
}

fn aggregate_human(result: &AggregateResult, blocks: bool) -> String {
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
    lines.extend(exposure_table(&result.exposure));
    if blocks {
        lines.extend(blocks_table(&result.blocks));
    }
    if !result.errors.is_empty() {
        lines.push("errors:".to_string());
        for error in &result.errors {
            lines.push(format!("- line {}: {}", error.line, error.message));
        }
    }
    lines.join("\n")
}

/// The breakdown as two sections, Mapped rows then Unmapped rows, in
/// [`sorted_breakdown`]'s order.
fn breakdown_table(breakdown: &[BreakdownRow]) -> Vec<String> {
    let total_inputs: usize = breakdown.iter().map(|row| row.inputs).sum();
    let total_weight: u64 = breakdown.iter().map(|row| row.baseline_weight).sum();
    let total_added: i64 = breakdown
        .iter()
        .filter_map(BreakdownRow::added_weight)
        .sum();
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
        "breakdown (inputs, % of inputs, baseline input weight, % of it, migrated input weight, added input weight, % of it):"
            .to_string(),
    ];
    for (heading, mapped) in [("mapped:", true), ("unmapped:", false)] {
        lines.push(heading.to_string());
        let rows: Vec<&BreakdownRow> = sorted_breakdown(breakdown)
            .into_iter()
            .filter(|row| matches!(row.kind, BreakdownKind::Mapped(_)) == mapped)
            .collect();
        if rows.is_empty() {
            lines.push("  (none)".to_string());
        }
        for row in rows {
            let dash = || "-".to_string();
            let migrated = row
                .migrated_weight
                .map_or_else(dash, |weight| weight.to_string());
            let added = row
                .added_weight()
                .map_or_else(dash, |weight| weight.to_string());
            let added_percent = row
                .added_weight()
                .map_or_else(dash, |weight| signed_percent(weight, total_added));
            lines.push(format!(
                "  {:<width$}  {:>8}  {:>6}  {:>12}  {:>6}  {:>12}  {:>12}  {:>6}",
                label(row),
                row.inputs,
                percent(row.inputs as u64, total_inputs as u64),
                row.baseline_weight,
                percent(row.baseline_weight, total_weight),
                migrated,
                added,
                added_percent,
            ));
        }
    }
    lines
}

/// One row per Key exposure, in the library's fixed order.
fn exposure_table(exposure: &[ExposureRow]) -> Vec<String> {
    let total_added: i64 = exposure.iter().map(ExposureRow::added_weight).sum();
    let width = exposure
        .iter()
        .map(|row| key_exposure_name(row.key_exposure).len())
        .max()
        .unwrap_or(0);

    let mut lines = vec![
        "key exposure (mapped inputs, baseline input weight, migrated input weight, added weight, % of it, unmapped inputs, their baseline input weight):"
            .to_string(),
    ];
    for row in exposure {
        lines.push(format!(
            "  {:<width$}  {:>8}  {:>12}  {:>12}  {:>12}  {:>6}  {:>8}  {:>12}",
            key_exposure_name(row.key_exposure),
            row.mapped_inputs,
            row.baseline_weight,
            row.migrated_weight,
            row.added_weight(),
            signed_percent(row.added_weight(), total_added),
            row.unmapped_inputs,
            row.unmapped_baseline_weight,
        ));
    }
    lines
}

/// `part` as a percentage of `whole` to one decimal place, for signed values
/// such as Added weight.
fn signed_percent(part: i64, whole: i64) -> String {
    if whole == 0 {
        return "-".to_string();
    }
    // Display only, as in `percent`.
    #[allow(clippy::cast_precision_loss)]
    let ratio = part as f64 / whole as f64;
    format!("{:.1}%", ratio * 100.0)
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

fn key_exposure_name(key_exposure: KeyExposure) -> &'static str {
    match key_exposure {
        KeyExposure::ExposedInOutput => "Exposed in output",
        KeyExposure::HashedUntilSpend => "Hashed until spend",
        KeyExposure::NoKey => "No key",
        KeyExposure::Undetermined => "Undetermined",
    }
}

fn unmapped_reason_name(reason: UnmappedReason) -> &'static str {
    match reason {
        UnmappedReason::P2trScriptPath => "P2TR script-path",
        UnmappedReason::P2trKeyPathAnnex => "P2TR key-path with annex",
        UnmappedReason::P2wshNonMultisig => "P2WSH non-multisig",
        UnmappedReason::P2shSegwitNonMultisig => "P2SH-wrapped segwit non-multisig",
        UnmappedReason::P2shNonMultisig => "P2SH non-multisig",
        UnmappedReason::P2pk => "P2PK",
        UnmappedReason::BareMultisig => "bare multisig",
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

fn scheme_name(parameter_set: ParameterSet) -> &'static str {
    match parameter_set {
        ParameterSet::MlDsa44 => "ml-dsa-44",
        ParameterSet::Falcon512 => "falcon-512",
        ParameterSet::SlhDsa128s => "slh-dsa-128s",
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
        BaselineSpendType::P2trScriptPathSingleKey => "P2TR script-path single-key",
        BaselineSpendType::P2wshContract => "P2WSH contract",
        BaselineSpendType::P2shP2wshContract => "P2SH-P2WSH contract",
    }
}

/// The spend type's name with its threshold, such as `P2WSH multisig 2-of-3`.
fn spend_type_label(spend_type: BaselineSpendType) -> String {
    match spend_type.threshold() {
        Some(MultisigThreshold { m, n }) => format!("{} {m}-of-{n}", spend_type_name(spend_type)),
        None => spend_type_name(spend_type).to_string(),
    }
}

fn migrate_human(
    baseline: &TransactionWeight,
    migration: &Migration,
    parameter_set: ParameterSet,
    fee_rate: Option<FeeRate>,
) -> String {
    let migrated = migration.migrated.as_ref();
    // (rate, fee today, post-quantum fee if there's a migrated total).
    let fees = fee_rate.map(|rate| {
        (
            rate,
            fee(baseline.vsize, rate),
            migrated.map(|total| fee(total.vsize, rate)),
        )
    });

    let mut lines = vec![migrate_summary(baseline, migration, parameter_set, fees)];

    // Today vs post-quantum; "-" where there's no migrated total.
    let pq = |value: Option<u64>| value.map_or_else(|| "-".to_string(), grouped);
    let mut rows = vec![
        [
            "weight (WU)".to_string(),
            grouped(baseline.weight),
            pq(migrated.map(|total| total.weight)),
        ],
        [
            "vsize (vB)".to_string(),
            grouped(baseline.vsize),
            pq(migrated.map(|total| total.vsize)),
        ],
    ];
    if let Some((rate, baseline_fee, pq_fee)) = fees {
        rows.push([
            format!("fee (sat, {rate} sat/vB)"),
            grouped(baseline_fee),
            pq(pq_fee),
        ]);
    }
    lines.push(String::new());
    lines.extend(migrate_table(&rows));

    lines.push(String::new());
    lines.push("inputs".to_string());
    lines.extend(migrate_input_lines(migration));

    if let Some(exceeds) = migration.exceeds_relay_limit {
        lines.push(String::new());
        lines.push(format!(
            "fits the {} WU relay limit: {}",
            grouped(RELAY_WEIGHT_LIMIT),
            if exceeds { "no" } else { "yes" }
        ));
    }

    lines.push(String::new());
    lines.push("assumptions".to_string());
    for assumption in &migration.assumptions {
        lines.push(format!("  - {assumption}"));
    }
    lines.join("\n")
}

/// `migrate`'s first line: how many times heavier (and pricier, given a fee
/// rate), or how many inputs are unmapped when there's no migrated total.
fn migrate_summary(
    baseline: &TransactionWeight,
    migration: &Migration,
    parameter_set: ParameterSet,
    fees: Option<(FeeRate, u64, Option<u64>)>,
) -> String {
    let scheme = scheme_name(parameter_set);
    let Some(total) = &migration.migrated else {
        let unmapped = migration
            .inputs
            .iter()
            .filter(|input| matches!(input, InputResult::Unmapped { .. }))
            .count();
        return format!(
            "{scheme}: no post-quantum total, {unmapped} of {} inputs unmapped",
            migration.inputs.len()
        );
    };
    let mut summary = format!(
        "{scheme}: {} the weight",
        times(total.weight, baseline.weight)
    );
    if let Some((_, baseline_fee, Some(pq_fee))) = fees
        && baseline_fee > 0
    {
        let _ = write!(summary, ", {} the fee", times(pq_fee, baseline_fee));
    }
    summary
}

/// `rows` of (label, today, post-quantum) under a header, numbers right-aligned.
fn migrate_table(rows: &[[String; 3]]) -> Vec<String> {
    let header = [
        String::new(),
        "today".to_string(),
        "post-quantum".to_string(),
    ];
    let width = |column: usize| {
        rows.iter()
            .chain([&header])
            .map(|row| row[column].len())
            .max()
            .unwrap_or(0)
    };
    let (label_width, today_width, pq_width) = (width(0), width(1), width(2));
    [&header]
        .into_iter()
        .chain(rows)
        .map(|[label, today, post_quantum]| {
            format!("{label:label_width$}  {today:>today_width$}  {post_quantum:>pq_width$}")
        })
        .collect()
}

/// One line per input: what it is, its Input weight today -> migrated (or just
/// today for an unmapped input), and its Key exposure.
fn migrate_input_lines(migration: &Migration) -> Vec<String> {
    let inputs: Vec<(String, String, KeyExposure)> = migration
        .inputs
        .iter()
        .map(|input| match input {
            InputResult::Mapped {
                spend_type,
                baseline_weight,
                template_weight,
            } => (
                spend_type_label(*spend_type),
                format!(
                    "{} -> {} WU",
                    grouped(*baseline_weight),
                    grouped(*template_weight)
                ),
                spend_type.key_exposure(),
            ),
            InputResult::Unmapped {
                reason,
                baseline_weight,
            } => (
                format!("unmapped: {}", unmapped_reason_name(*reason)),
                format!("{} WU", grouped(*baseline_weight)),
                reason.key_exposure(),
            ),
        })
        .collect();
    let index_width = format!("#{}", inputs.len().saturating_sub(1)).len();
    let kind_width = inputs.iter().map(|input| input.0.len()).max().unwrap_or(0);
    let weight_width = inputs.iter().map(|input| input.1.len()).max().unwrap_or(0);
    inputs
        .iter()
        .enumerate()
        .map(|(i, (kind, weights, key_exposure))| {
            format!(
                "  {:index_width$}  {kind:kind_width$}  {weights:>weight_width$}  key: {}",
                format!("#{i}"),
                key_exposure_name(*key_exposure)
            )
        })
        .collect()
}

/// `n` with a comma every three digits, such as `10,180`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `part / whole` to one decimal place, rounded half up, such as `9.3x`.
/// `whole` must not be zero.
fn times(part: u64, whole: u64) -> String {
    let tenths = (part * 10 + whole / 2) / whole;
    format!("{}.{}x", tenths / 10, tenths % 10)
}

/// `migrate --json` for one transaction. `line`, when given, is its line number
/// in a `--json-lines` batch, printed as the first field.
fn migrate_json(
    line: Option<usize>,
    baseline: &TransactionWeight,
    migration: &Migration,
    fee_rate: Option<FeeRate>,
) -> String {
    let inputs: Vec<String> = migration
        .inputs
        .iter()
        .map(|input| match input {
            InputResult::Mapped {
                spend_type,
                baseline_weight,
                template_weight,
            } => {
                format!(
                    r#"{{"status":"mapped","spend_type":"{}"{},"baseline_weight":{baseline_weight},"template_weight":{template_weight},"key_exposure":"{}"}}"#,
                    spend_type_name(*spend_type),
                    threshold_field(*spend_type),
                    key_exposure_name(spend_type.key_exposure())
                )
            }
            InputResult::Unmapped {
                reason,
                baseline_weight,
            } => format!(
                r#"{{"status":"unmapped","reason":"{}","baseline_weight":{baseline_weight},"key_exposure":"{}"}}"#,
                unmapped_reason_name(*reason),
                key_exposure_name(reason.key_exposure())
            ),
        })
        .collect();

    let migrated = migration
        .migrated
        .as_ref()
        .map_or_else(|| "null".to_string(), weight_json);

    let exceeds_relay_limit = match migration.exceeds_relay_limit {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };

    let assumptions: Vec<String> = migration
        .assumptions
        .iter()
        .map(|a| json_string(a))
        .collect();

    let fee_field = match (fee_rate, &migration.migrated) {
        (Some(rate), Some(total)) => {
            let baseline_fee = fee(baseline.vsize, rate);
            let pq_fee = fee(total.vsize, rate);
            format!(
                r#","fee":{{"baseline":{baseline_fee},"pq":{pq_fee},"difference":{}}}"#,
                pq_fee.abs_diff(baseline_fee)
            )
        }
        _ => String::new(),
    };

    let line_field = line.map_or_else(String::new, |line| format!(r#""line":{line},"#));
    format!(
        r#"{{{line_field}"inputs":[{}],"baseline":{},"migrated":{migrated},"exceeds_relay_limit":{exceeds_relay_limit},"assumptions":[{}]{fee_field}}}"#,
        inputs.join(","),
        weight_json(baseline),
        assumptions.join(","),
    )
}

/// A transaction's weight as the JSON object `weight --json` prints.
fn weight_json(weight: &TransactionWeight) -> String {
    format!(
        r#"{{"weight":{},"vsize":{},"stripped_size":{},"total_size":{}}}"#,
        weight.weight, weight.vsize, weight.stripped_size, weight.total_size
    )
}

/// The text of the file at the one positional argument, or of stdin if there
/// is none. With `--json-lines` a lone argument is always read as the path.
fn read_path_or_stdin(positional: &[&str]) -> Result<String, String> {
    match positional {
        [path] => {
            std::fs::read_to_string(path).map_err(|err| format!("could not read {path}: {err}"))
        }
        [] => read_stdin(),
        _ => Err(USAGE.to_string()),
    }
}

/// `text` as a JSON string literal, quotes included.
fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                // Writing to a String never fails.
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
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

#[cfg(test)]
mod tests {
    use super::{grouped, limit_multiple, limit_multiple_label, times};

    #[test]
    fn grouped_puts_a_comma_every_three_digits() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(10_180), "10,180");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn times_rounds_half_up_to_one_place() {
        // 4069 / 436 = 9.33.
        assert_eq!(times(4069, 436), "9.3x");
        assert_eq!(times(436, 436), "1.0x");
        // 0.95 exactly rounds up; 0.949 down.
        assert_eq!(times(95, 100), "1.0x");
        assert_eq!(times(949, 1000), "0.9x");
    }

    #[test]
    fn limit_multiple_is_the_exact_decimal_without_trailing_zeros() {
        assert_eq!(limit_multiple(4_000_000), "1");
        assert_eq!(limit_multiple(6_000_000), "1.5");
        // 1 WU is 25 / 10^8 of the limit: the smallest step, 8 places.
        assert_eq!(limit_multiple(1), "0.00000025");
        // 32,781,178 x 25 = 819,529,450.
        assert_eq!(limit_multiple(32_781_178), "8.1952945");
    }

    #[test]
    fn limit_multiple_label_rounds_half_up_to_one_place() {
        assert_eq!(limit_multiple_label(0), "x0.0");
        // 32,781,178 / 4,000,000 = 8.195...
        assert_eq!(limit_multiple_label(32_781_178), "x8.2");
        // 3,799,999 is just under 0.95: rounds down. 3,800,000 is exactly 0.95: up.
        assert_eq!(limit_multiple_label(3_799_999), "x0.9");
        assert_eq!(limit_multiple_label(3_800_000), "x1.0");
    }
}
