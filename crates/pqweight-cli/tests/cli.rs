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
fn migrate_command_names_a_p2tr_single_key_leaf_input_and_states_its_output_assumption() {
    let fx = fixture("p2tr-scriptpath");

    let output = run(&["migrate", "--scheme", "ml-dsa-44", &fx.hex], None);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Template weight hand-derived in pqweight::tests::migrate::
    // p2tr_single_key_leaf_is_migrated_with_the_pq_key_in_the_leaf_and_no_internal_key.
    assert!(
        text.contains("input 0: mapped (P2TR script-path single-key), weight: 3909"),
        "{text}"
    );
    assert!(text.contains("Merkle root directly"), "{text}");
}

#[test]
fn migrate_command_names_the_unmapped_reason_of_an_unmapped_input() {
    let fx = fixture("p2tr-keypath-annex");

    let human = run(&["migrate", "--scheme", "ml-dsa-44", &fx.hex], None);
    let json = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
        None,
    );

    assert!(
        stdout(&human).contains("input 0: unmapped (P2TR key-path with annex)"),
        "{}",
        stdout(&human)
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(parsed["inputs"][0]["status"], "unmapped");
    assert_eq!(parsed["inputs"][0]["reason"], "P2TR key-path with annex");
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
    let key_path_annex = fixture("p2tr-keypath-annex");
    let path = scratch_file("breakdown");
    std::fs::write(
        &path,
        format!("{}\n{}\n{}\n", p2wpkh.hex, key_path_annex.hex, p2wpkh.hex),
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
    // 3903 each), P2TR key-path with annex 234. 540 / 774 = 69.8%,
    // 234 / 774 = 30.2%.
    // Columns: inputs, % of inputs, baseline Input weight, % of it, migrated.
    assert_eq!(
        row_after(&text, "P2WPKH"),
        ["2", "66.7%", "540", "69.8%", "7806"]
    );
    assert_eq!(
        row_after(&text, "P2TR key-path with annex"),
        ["1", "33.3%", "234", "30.2%", "-"]
    );
    // The partially mapped transaction's Oracle weight, 400 of 436 + 400 + 436
    // = 1272 baseline weight, is 31.4%.
    assert!(
        text.contains("partially mapped baseline weight: 400 (31.4% of baseline)"),
        "{text}"
    );
}

#[test]
fn migrate_command_names_the_key_exposure_of_every_input() {
    // (fixture, human input line, Key exposure). Weights and reasons as in the
    // tests above.
    let cases = [
        (
            "p2wpkh",
            "input 0: mapped (P2WPKH), weight: 3903, key exposure: Hashed until spend",
            "Hashed until spend",
        ),
        (
            "p2tr-keypath-annex",
            "input 0: unmapped (P2TR key-path with annex), key exposure: Exposed in output",
            "Exposed in output",
        ),
    ];
    for (name, line, key_exposure) in cases {
        let fx = fixture(name);

        let human = run(&["migrate", "--scheme", "ml-dsa-44", &fx.hex], None);
        let json = run(
            &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
            None,
        );

        assert!(stdout(&human).contains(line), "{}", stdout(&human));
        let parsed: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
        assert_eq!(parsed["inputs"][0]["key_exposure"], key_exposure, "{name}");
    }
}

#[test]
fn aggregate_command_prints_added_weight_by_key_exposure() {
    let p2wpkh = p2wpkh_fixture();
    let key_path_annex = fixture("p2tr-keypath-annex");
    let path = scratch_file("exposure");
    std::fs::write(
        &path,
        format!("{}\n{}\n{}\n", p2wpkh.hex, key_path_annex.hex, p2wpkh.hex),
    )
    .unwrap();

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44", path.to_str().unwrap()],
        None,
    );
    std::fs::remove_file(&path).ok();

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Columns: mapped inputs, baseline Input weight, migrated, Added weight,
    // % of all Added weight, Unmapped inputs, their baseline Input weight.
    // P2WPKH: 2 x 270 = 540 today, 2 x 3903 = 7806 migrated, 7266 added: all of it.
    // P2TR key-path with annex: Unmapped, 234.
    assert_eq!(
        row_after(&text, "Hashed until spend"),
        ["2", "540", "7806", "7266", "100.0%", "0", "0"]
    );
    assert_eq!(
        row_after(&text, "Exposed in output"),
        ["0", "0", "0", "0", "0.0%", "1", "234"]
    );
    assert_eq!(
        row_after(&text, "No key"),
        ["0", "0", "0", "0", "0.0%", "0", "0"]
    );
    assert_eq!(
        row_after(&text, "Undetermined"),
        ["0", "0", "0", "0", "0.0%", "0", "0"]
    );
}

#[test]
fn migrate_json_lines_prints_one_migrate_json_object_per_line_of_a_file() {
    let wpkh = p2wpkh_fixture();
    let multisig = fixture("p2wsh-multisig");
    let path = scratch_file("json-lines");
    std::fs::write(&path, format!("{}\n{}\n", wpkh.hex, multisig.hex)).unwrap();

    let output = run(
        &[
            "migrate",
            "--scheme",
            "ml-dsa-44",
            "--json-lines",
            path.to_str().unwrap(),
        ],
        None,
    );
    std::fs::remove_file(&path).ok();

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    // Each line is exactly what `migrate --json` prints for that transaction.
    for (line, fx) in lines.iter().zip([&wpkh, &multisig]) {
        let single = run(
            &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
            None,
        );
        assert_eq!(*line, stdout(&single).trim_end());
    }
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSON");
    assert_eq!(first["migrated"]["weight"], 4069);
}

#[test]
fn migrate_json_lines_reads_stdin_skips_blank_lines_and_reports_a_bad_line_in_place() {
    let fx = p2wpkh_fixture();

    let output = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json-lines"],
        Some(&format!("{}\n\nzz\n{}\n", fx.hex, fx.hex)),
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid JSON"))
        .collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[0]["migrated"]["weight"], 4069);
    // Line numbers count the blank line, as `aggregate`'s do.
    assert_eq!(lines[1]["line"], 3);
    assert!(
        lines[1]["error"]
            .as_str()
            .is_some_and(|e| e.contains("hex")),
        "{text}"
    );
    assert_eq!(lines[2]["migrated"]["weight"], 4069);
}

#[test]
fn migrate_json_lines_fails_when_the_file_cannot_be_read() {
    let output = run(
        &[
            "migrate",
            "--scheme",
            "ml-dsa-44",
            "--json-lines",
            "no-such-file.txt",
        ],
        None,
    );

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("no-such-file.txt"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn migrate_json_lines_with_a_path_and_a_hex_argument_is_a_usage_error() {
    let fx = p2wpkh_fixture();

    let output = run(
        &[
            "migrate",
            "--scheme",
            "ml-dsa-44",
            "--json-lines",
            "sample.txt",
            &fx.hex,
        ],
        None,
    );

    assert!(!output.status.success());
    assert!(stderr(&output).contains("usage"), "{}", stderr(&output));
}
