//! Smoke tests for the `pqweight` binary: arguments and stdin in; stdout, stderr and
//! exit code out. The CLI holds no logic, so these only check the wiring. Weight
//! correctness is covered by the library's Fixture test.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// A real Fixture: its raw hex and the Oracle's recorded numbers.
struct Fixture {
    hex: String,
    weight: u64,
    vsize: u64,
    size: u64,
}

fn p2wpkh_fixture() -> Fixture {
    fixture("p2wpkh")
}

fn fixture(name: &str) -> Fixture {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../pqweight/tests/fixtures");
    let hex = std::fs::read_to_string(dir.join(format!("{name}.hex"))).unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap())
            .unwrap();
    Fixture {
        hex: hex.trim().to_string(),
        weight: meta["oracle"]["weight"].as_u64().unwrap(),
        vsize: meta["oracle"]["vsize"].as_u64().unwrap(),
        size: meta["oracle"]["size"].as_u64().unwrap(),
    }
}

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pqweight"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary starts");
    let mut pipe = child.stdin.take().unwrap();
    if let Some(text) = stdin {
        pipe.write_all(text.as_bytes()).unwrap();
    }
    drop(pipe); // close stdin so a command reading it sees EOF
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn weight_command_prints_weight_and_vsize_for_a_hex_argument() {
    let fx = p2wpkh_fixture();

    let output = run(&["weight", &fx.hex], None);

    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains(&format!("weight: {}", fx.weight)), "{text}");
    assert!(text.contains(&format!("vsize: {}", fx.vsize)), "{text}");
    assert!(text.contains(&format!("total size: {}", fx.size)), "{text}");
}

#[test]
fn weight_command_reads_hex_from_stdin_when_no_argument_is_given() {
    let fx = p2wpkh_fixture();

    // A trailing newline is what `bitcoin-cli ... | pqweight weight` would send.
    let output = run(&["weight"], Some(&format!("{}\n", fx.hex)));

    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains(&format!("weight: {}", fx.weight)), "{text}");
}

#[test]
fn json_flag_prints_machine_readable_output() {
    let fx = p2wpkh_fixture();

    let output = run(&["weight", "--json", &fx.hex], None);

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(json["weight"], fx.weight);
    assert_eq!(json["vsize"], fx.vsize);
    assert_eq!(json["total_size"], fx.size);
    assert!(json["stripped_size"].is_u64());
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn invalid_hex_fails_with_a_message_that_says_so() {
    let output = run(&["weight", "zz"], None);

    assert!(!output.status.success());
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains("invalid hex"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn incomplete_transaction_fails_with_what_was_being_read_and_where() {
    // Valid hex, but only a version and an input count: the previous output starts at byte 5.
    let output = run(&["weight", "0100000001"], None);

    assert!(!output.status.success());
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains("unexpected end of input while reading previous output at byte 5"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn migrate_command_prints_the_migrated_total_and_assumptions() {
    let fx = p2wpkh_fixture();

    let output = run(&["migrate", "--scheme", "ml-dsa-44", &fx.hex], None);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Weights hand-derived the same way as
    // pqweight::tests::migrate::p2wpkh_input_is_migrated_to_an_ml_dsa_44_witness.
    assert!(text.contains("migrated weight: 4069"), "{text}");
    assert!(text.contains("migrated vsize: 1018"), "{text}");
    assert!(text.to_lowercase().contains("assumptions"), "{text}");
}

#[test]
fn migrate_command_reports_fees_when_a_fee_rate_is_given() {
    let fx = p2wpkh_fixture();

    let output = run(
        &[
            "migrate",
            "--scheme",
            "ml-dsa-44",
            "--fee-rate",
            "2",
            &fx.hex,
        ],
        None,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Baseline vsize is the fixture's oracle vsize (109); PQ vsize is 1018
    // (see the test above), both at 2 sat/vB.
    assert!(
        text.contains(&format!("baseline fee: {} sat", 2 * fx.vsize)),
        "{text}"
    );
    assert!(text.contains("pq fee: 2036 sat"), "{text}");
}

#[test]
fn migrate_command_json_flag_prints_machine_readable_output() {
    let fx = p2wpkh_fixture();

    let output = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
        None,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(json["migrated"]["weight"], 4069);
    assert_eq!(json["migrated"]["vsize"], 1018);
    assert_eq!(json["exceeds_relay_limit"], false);
    assert!(
        json["assumptions"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    );
    assert_eq!(json["inputs"][0]["spend_type"], "P2WPKH");
}

#[test]
fn migrate_command_names_a_multisig_input_with_its_threshold() {
    let fx = fixture("p2wsh-multisig");

    let output = run(&["migrate", "--scheme", "ml-dsa-44", &fx.hex], None);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Template weight hand-derived in pqweight::tests::migrate::
    // p2wsh_multisig_input_is_migrated_with_every_public_key_in_the_script.
    assert!(
        text.contains("input 0: mapped (P2WSH multisig 2-of-3), weight: 8963"),
        "{text}"
    );
}

#[test]
fn migrate_command_json_reports_the_multisig_threshold_as_its_own_object() {
    let fx = fixture("p2wsh-multisig");

    let output = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
        None,
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    let input = &json["inputs"][0];
    assert_eq!(input["spend_type"], "P2WSH multisig");
    assert_eq!(input["threshold"]["m"], 2);
    assert_eq!(input["threshold"]["n"], 3);
    assert_eq!(input["template_weight"], 8963);
}

#[test]
fn migrate_command_json_has_no_threshold_for_a_single_key_input() {
    let fx = p2wpkh_fixture();

    let output = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
        None,
    );

    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert!(json["inputs"][0].get("threshold").is_none());
}

#[test]
fn migrate_command_rejects_an_unknown_scheme() {
    let fx = p2wpkh_fixture();

    let output = run(&["migrate", "--scheme", "rsa-2048", &fx.hex], None);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("rsa-2048"), "{}", stderr(&output));
}

#[test]
fn migrate_command_rejects_a_bad_fee_rate() {
    let fx = p2wpkh_fixture();

    let output = run(
        &[
            "migrate",
            "--scheme",
            "ml-dsa-44",
            "--fee-rate",
            "-1",
            &fx.hex,
        ],
        None,
    );

    assert!(!output.status.success());
    assert!(stderr(&output).contains("fee"), "{}", stderr(&output));
}

/// A path in the OS temp directory unique to this test process and name, so
/// parallel test runs never collide.
fn scratch_file(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "pqweight-cli-test-{name}-{}.txt",
        std::process::id()
    ))
}

#[test]
fn aggregate_command_sums_totals_over_a_multi_line_file() {
    let fx = p2wpkh_fixture();
    let path = scratch_file("sums-totals");
    // Two lines, the same fixture twice, so the expected sums are just double
    // the fixture's own already-verified numbers.
    std::fs::write(&path, format!("{}\n{}\n", fx.hex, fx.hex)).unwrap();

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44", path.to_str().unwrap()],
        None,
    );
    std::fs::remove_file(&path).ok();

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("transactions parsed: 2"), "{text}");
    assert!(text.contains("fully mapped: 2"), "{text}");
    assert!(text.contains("parse errors: 0"), "{text}");
    assert!(
        text.contains(&format!("baseline weight: {}", 2 * fx.weight)),
        "{text}"
    );
    // PQ weight per p2wpkh input at ML-DSA-44 is 4069 WU (see
    // pqweight::tests::migrate::p2wpkh_input_is_migrated_to_an_ml_dsa_44_witness).
    assert!(
        text.contains(&format!("migrated weight: {}", 2 * 4069)),
        "{text}"
    );
}

#[test]
fn aggregate_command_reads_from_stdin_when_no_path_is_given() {
    let fx = p2wpkh_fixture();

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44"],
        Some(&format!("{}\n{}\n", fx.hex, fx.hex)),
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("transactions parsed: 2"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn aggregate_command_records_a_bad_line_without_aborting_the_batch() {
    let fx = p2wpkh_fixture();
    let path = scratch_file("bad-line");
    std::fs::write(&path, format!("{}\nzz\n{}\n", fx.hex, fx.hex)).unwrap();

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44", path.to_str().unwrap()],
        None,
    );
    std::fs::remove_file(&path).ok();

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("transactions parsed: 2"), "{text}");
    assert!(text.contains("parse errors: 1"), "{text}");
    assert!(text.contains("line 2"), "{text}");
}

#[test]
fn migrate_command_names_the_unmapped_reason_of_an_unmapped_input() {
    let fx = fixture("p2tr-scriptpath");

    let human = run(&["migrate", "--scheme", "ml-dsa-44", &fx.hex], None);
    let json = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
        None,
    );

    assert!(
        stdout(&human).contains("input 0: unmapped (P2TR script-path)"),
        "{}",
        stdout(&human)
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(parsed["inputs"][0]["status"], "unmapped");
    assert_eq!(parsed["inputs"][0]["reason"], "P2TR script-path");
}

/// The whitespace-separated columns after `label` on the line that starts with
/// it, so the check doesn't depend on column padding.
fn row_after<'a>(text: &'a str, label: &str) -> Vec<&'a str> {
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(label))
        .unwrap_or_else(|| panic!("no row for {label} in:\n{text}"));
    line.trim_start()[label.len()..]
        .split_whitespace()
        .collect()
}

#[test]
fn aggregate_command_prints_a_breakdown_by_spend_type_and_unmapped_reason() {
    let p2wpkh = p2wpkh_fixture();
    let script_path = fixture("p2tr-scriptpath");
    let path = scratch_file("breakdown");
    std::fs::write(
        &path,
        format!("{}\n{}\n{}\n", p2wpkh.hex, script_path.hex, p2wpkh.hex),
    )
    .unwrap();

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44", path.to_str().unwrap()],
        None,
    );
    std::fs::remove_file(&path).ok();

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Input weights from the library's breakdown test: P2WPKH 270 each (template
    // 3903 each), P2TR script-path 299. 540 / 839 = 64.4%, 299 / 839 = 35.6%.
    // Columns: inputs, % of inputs, baseline Input weight, % of it, migrated.
    assert_eq!(
        row_after(&text, "P2WPKH"),
        ["2", "66.7%", "540", "64.4%", "7806"]
    );
    assert_eq!(
        row_after(&text, "P2TR script-path"),
        ["1", "33.3%", "299", "35.6%", "-"]
    );
    // The partially mapped transaction's Oracle weight, 465 of 436 + 465 + 436
    // = 1337 baseline weight, is 34.8%.
    assert!(
        text.contains("partially mapped baseline weight: 465 (34.8% of baseline)"),
        "{text}"
    );
}
