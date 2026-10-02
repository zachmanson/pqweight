//! Smoke tests for the `pqweight` binary: arguments and stdin in; stdout, stderr and
//! exit code out. The CLI computes no weights, so these check the wiring and
//! what only the CLI decides: output formats and the order of report rows. Weight
//! correctness is covered by the library's Fixture test.

use std::fmt::Write as _;
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
fn migrate_command_json_reports_the_baseline_transaction_and_each_input_baseline_weight() {
    let fx = p2wpkh_fixture();
    let annex = fixture("p2tr-keypath-annex");

    let mapped = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
        None,
    );
    let unmapped = run(
        &["migrate", "--scheme", "ml-dsa-44", "--json", &annex.hex],
        None,
    );

    let mapped: serde_json::Value = serde_json::from_str(&stdout(&mapped)).expect("valid JSON");
    // The Oracle's numbers; stripped size from weight = 3 x stripped + total.
    assert_eq!(mapped["baseline"]["weight"], fx.weight);
    assert_eq!(mapped["baseline"]["vsize"], fx.vsize);
    assert_eq!(mapped["baseline"]["total_size"], fx.size);
    assert_eq!(
        mapped["baseline"]["stripped_size"],
        (fx.weight - fx.size) / 3
    );
    // Input weights as in the library's breakdown test: P2WPKH 270, P2TR
    // key-path with annex 234.
    assert_eq!(mapped["inputs"][0]["baseline_weight"], 270);
    let unmapped: serde_json::Value = serde_json::from_str(&stdout(&unmapped)).expect("valid JSON");
    assert_eq!(unmapped["inputs"][0]["baseline_weight"], 234);
    assert_eq!(unmapped["baseline"]["weight"], annex.weight);
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
    // Columns: inputs, % of inputs, baseline Input weight, % of it, migrated,
    // Added weight, % of all Added weight. 2 x (3903 - 270) = 7266 added.
    assert_eq!(
        row_after(&text, "P2WPKH"),
        ["2", "66.7%", "540", "69.8%", "7806", "7266", "100.0%"]
    );
    assert_eq!(
        row_after(&text, "P2TR key-path with annex"),
        ["1", "33.3%", "234", "30.2%", "-", "-", "-"]
    );
    // The partially mapped transaction's Oracle weight, 400 of 436 + 400 + 436
    // = 1272 baseline weight, is 31.4%.
    assert!(
        text.contains("partially mapped baseline weight: 400 (31.4% of baseline)"),
        "{text}"
    );
}

#[test]
fn aggregate_command_sorts_mapped_breakdown_rows_by_added_weight() {
    let p2wpkh = p2wpkh_fixture();
    let key_path = fixture("p2tr-keypath");
    let key_path_annex = fixture("p2tr-keypath-annex");
    let path = scratch_file("breakdown-added");
    std::fs::write(
        &path,
        format!(
            "{}
{}
{}
",
            p2wpkh.hex, key_path.hex, key_path_annex.hex
        ),
    )
    .unwrap();

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44", path.to_str().unwrap()],
        None,
    );
    std::fs::remove_file(&path).ok();

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Input weights: P2WPKH 270 -> 3903 (3633 added), P2TR key-path 41 x 4 +
    // witness 1 + (1 + 64) = 230 -> 3903 (3673 added), P2TR key-path with annex
    // Unmapped at 234. 734 baseline Input weight, 7306 Added weight in all.
    // P2WPKH is larger today but P2TR key-path adds more, so it comes first.
    assert_eq!(
        row_after(&text, "P2TR key-path"),
        ["1", "33.3%", "230", "31.3%", "3903", "3673", "50.3%"]
    );
    assert_eq!(
        row_after(&text, "P2WPKH"),
        ["1", "33.3%", "270", "36.8%", "3903", "3633", "49.7%"]
    );
    assert_eq!(
        row_after(&text, "P2TR key-path with annex"),
        ["1", "33.3%", "234", "31.9%", "-", "-", "-"]
    );
    let position = |label: &str| {
        text.lines()
            .position(|line| line.trim_start().starts_with(label))
            .unwrap()
    };
    assert!(position("P2TR key-path") < position("P2WPKH"), "{text}");
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
    // Each line is what `migrate --json` prints for that transaction, with its
    // 1-indexed line number as the first field.
    for (number, (line, fx)) in lines.iter().zip([&wpkh, &multisig]).enumerate() {
        let single = run(
            &["migrate", "--scheme", "ml-dsa-44", "--json", &fx.hex],
            None,
        );
        let single = stdout(&single);
        let expected = format!(
            r#"{{"line":{},{}"#,
            number + 1,
            single.trim_end().strip_prefix('{').unwrap()
        );
        assert_eq!(*line, expected);
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
    assert_eq!(lines[0]["line"], 1);
    // Line numbers count the blank line, as `aggregate`'s do.
    assert_eq!(lines[1]["line"], 3);
    assert!(
        lines[1]["error"]
            .as_str()
            .is_some_and(|e| e.contains("hex")),
        "{text}"
    );
    assert_eq!(lines[2]["migrated"]["weight"], 4069);
    assert_eq!(lines[2]["line"], 4);
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

fn aggregate_json(args: &[&str], lines: &str) -> serde_json::Value {
    let mut all = vec!["aggregate", "--scheme", "ml-dsa-44", "--json"];
    all.extend_from_slice(args);
    let output = run(&all, Some(lines));
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).expect("valid JSON")
}

#[test]
fn aggregate_command_json_reports_totals_breakdown_key_exposure_and_errors() {
    let p2wpkh = p2wpkh_fixture();
    let key_path = fixture("p2tr-keypath");
    let key_path_annex = fixture("p2tr-keypath-annex");

    let mut json = aggregate_json(
        &["--fee-rate", "1.5"],
        &format!(
            "{}\n{}\n\nzz\n{}\n",
            p2wpkh.hex, key_path.hex, key_path_annex.hex
        ),
    );

    // Oracle weight/vsize: p2wpkh 436/109, p2tr-keypath 396/99,
    // p2tr-keypath-annex 400/100 (partially mapped). Migrated: 4069/1018 for
    // each of the first two (see migrate_command_prints_the_migrated_total_and_assumptions).
    // Fees at 1.5 sat/vB, rounded up per transaction: 164 + 149 + 150 = 463
    // today, 1527 + 1527 = 3054 migrated. Input weights as in
    // aggregate_command_sorts_mapped_breakdown_rows_by_added_weight.
    let errors = json
        .as_object_mut()
        .unwrap()
        .remove("errors")
        .expect("errors field");
    assert_eq!(
        json,
        serde_json::json!({
            "scheme": "ml-dsa-44",
            "fee_rate": 1.5,
            "counts": {"parsed": 3, "fully_mapped": 2, "partially_mapped": 1,
                       "unmapped_inputs": 1, "parse_errors": 1},
            "baseline": {"weight": 1232, "vsize": 308},
            "migrated": {"weight": 8138, "vsize": 2036},
            "partially_mapped": {"weight": 400, "vsize": 100},
            "fee": {"baseline": 463, "migrated": 3054},
            "breakdown": [
                {"status": "mapped", "spend_type": "P2TR key-path",
                 "key_exposure": "Exposed in output", "inputs": 1,
                 "baseline_weight": 230, "migrated_weight": 3903, "added_weight": 3673},
                {"status": "mapped", "spend_type": "P2WPKH",
                 "key_exposure": "Hashed until spend", "inputs": 1,
                 "baseline_weight": 270, "migrated_weight": 3903, "added_weight": 3633},
                {"status": "unmapped", "reason": "P2TR key-path with annex",
                 "key_exposure": "Exposed in output", "inputs": 1, "baseline_weight": 234},
            ],
            "key_exposure": [
                {"key_exposure": "Exposed in output", "mapped_inputs": 1,
                 "baseline_weight": 230, "migrated_weight": 3903, "added_weight": 3673,
                 "unmapped_inputs": 1, "unmapped_baseline_weight": 234},
                {"key_exposure": "Hashed until spend", "mapped_inputs": 1,
                 "baseline_weight": 270, "migrated_weight": 3903, "added_weight": 3633,
                 "unmapped_inputs": 0, "unmapped_baseline_weight": 0},
                {"key_exposure": "No key", "mapped_inputs": 0,
                 "baseline_weight": 0, "migrated_weight": 0, "added_weight": 0,
                 "unmapped_inputs": 0, "unmapped_baseline_weight": 0},
                {"key_exposure": "Undetermined", "mapped_inputs": 0,
                 "baseline_weight": 0, "migrated_weight": 0, "added_weight": 0,
                 "unmapped_inputs": 0, "unmapped_baseline_weight": 0},
            ],
        })
    );
    // Line numbers count the blank line.
    assert_eq!(errors.as_array().map(Vec::len), Some(1), "{errors}");
    assert_eq!(errors[0]["line"], 4);
    assert!(
        errors[0]["message"]
            .as_str()
            .is_some_and(|m| m.contains("hex")),
        "{errors}"
    );
}

#[test]
fn aggregate_command_json_without_a_fee_rate_or_a_fully_mapped_transaction() {
    let key_path_annex = fixture("p2tr-keypath-annex");

    let json = aggregate_json(&[], &format!("{}\n", key_path_annex.hex));

    assert_eq!(json["fee_rate"], serde_json::Value::Null);
    assert!(json.get("fee").is_none(), "{json}");
    assert_eq!(json["migrated"], serde_json::Value::Null);
    assert_eq!(json["counts"]["fully_mapped"], 0);
    assert_eq!(json["baseline"]["weight"], key_path_annex.weight);
    assert_eq!(json["errors"], serde_json::json!([]));
}

#[test]
fn aggregate_command_json_fee_is_null_when_nothing_is_fully_mapped() {
    let key_path_annex = fixture("p2tr-keypath-annex");

    let json = aggregate_json(&["--fee-rate", "2"], &format!("{}\n", key_path_annex.hex));

    assert_eq!(json["fee_rate"], 2);
    // Oracle vsize 100 at 2 sat/vB.
    assert_eq!(
        json["fee"],
        serde_json::json!({"baseline": 200, "migrated": null})
    );
}

/// Every Fixture's hex, one per line, in file-name order.
fn all_fixture_lines() -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../pqweight/tests/fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hex"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap().trim().to_string() + "\n")
        .collect()
}

#[test]
fn aggregate_json_breakdown_is_migrate_json_lines_inputs_grouped_by_kind() {
    use std::collections::BTreeMap;

    let lines = all_fixture_lines();
    for scheme in ["ml-dsa-44", "falcon-512", "slh-dsa-128s"] {
        let per_tx = run(
            &["migrate", "--scheme", scheme, "--json-lines"],
            Some(&lines),
        );
        let aggregate = run(&["aggregate", "--scheme", scheme, "--json"], Some(&lines));
        assert!(per_tx.status.success(), "{}", stderr(&per_tx));
        assert!(aggregate.status.success(), "{}", stderr(&aggregate));

        // Regroup every input by the fields that name its row, summing the rest
        // the way the breakdown documents it.
        let mut grouped: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for line in stdout(&per_tx).lines() {
            let tx: serde_json::Value = serde_json::from_str(line).unwrap();
            for input in tx["inputs"].as_array().unwrap() {
                let mut key = input.clone();
                let key_fields = key.as_object_mut().unwrap();
                let baseline = key_fields
                    .remove("baseline_weight")
                    .unwrap()
                    .as_i64()
                    .unwrap();
                let template = key_fields
                    .remove("template_weight")
                    .map(|w| w.as_i64().unwrap());
                let row = grouped.entry(key.to_string()).or_insert_with(|| {
                    let mut row = key.clone();
                    let fields = row.as_object_mut().unwrap();
                    fields.insert("inputs".into(), 0.into());
                    fields.insert("baseline_weight".into(), 0.into());
                    if template.is_some() {
                        fields.insert("migrated_weight".into(), 0.into());
                        fields.insert("added_weight".into(), 0.into());
                    }
                    row
                });
                let add = |row: &mut serde_json::Value, field: &str, by: i64| {
                    row[field] = (row[field].as_i64().unwrap() + by).into();
                };
                add(row, "inputs", 1);
                add(row, "baseline_weight", baseline);
                if let Some(template) = template {
                    add(row, "migrated_weight", template);
                    add(row, "added_weight", template - baseline);
                }
            }
        }

        let aggregate: serde_json::Value = serde_json::from_str(&stdout(&aggregate)).unwrap();
        let mut from_aggregate: Vec<serde_json::Value> =
            aggregate["breakdown"].as_array().unwrap().clone();
        let mut from_migrate: Vec<serde_json::Value> = grouped.into_values().collect();
        from_aggregate.sort_by_key(ToString::to_string);
        from_migrate.sort_by_key(ToString::to_string);
        assert!(
            from_migrate.len() > 5,
            "{scheme}: too few kinds to be a real check"
        );
        assert_eq!(from_aggregate, from_migrate, "{scheme}");
    }
}

fn snapshot_fixture_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../pqweight/tests/fixtures/snapshot/regtest-utxo.dat")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn move_cost_json_reports_each_exposed_type_with_dust_cut_and_total() {
    let output = run(&["move-cost", "--json", &snapshot_fixture_path()], None);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["scanned"]["coins"], 118);
    assert_eq!(json["snapshot"]["coins"], 118);
    assert_eq!(
        json["snapshot"]["base_block_hash"].as_str().unwrap().len(),
        64
    );

    let rows = json["rows"].as_array().unwrap();
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["exposed_type"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "P2PK compressed",
            "P2PK uncompressed",
            "bare multisig",
            "bare multisig",
            "bare multisig",
            "bare multisig",
            "bare multisig",
            "P2TR"
        ]
    );
    assert_eq!(rows[2]["threshold"], serde_json::json!({"m": 1, "n": 1}));
    assert!(rows[0].get("threshold").is_none());

    let p2tr = &rows[7];
    assert_eq!(p2tr["all"]["coins"], 3);
    assert_eq!(p2tr["all"]["floor_weight"], 690);
    assert_eq!(p2tr["all"]["ceiling_weight"], 1_332);
    assert_eq!(p2tr["above_dust"]["coins"], 2);
    assert_eq!(p2tr["above_dust"]["floor_blocks"], 460.0 / 4_000_000.0);

    let floor_sum: u64 = rows
        .iter()
        .map(|row| row["all"]["floor_weight"].as_u64().unwrap())
        .sum();
    assert_eq!(json["total"]["all"]["floor_weight"], floor_sum);
    assert!(!json["assumptions"].as_array().unwrap().is_empty());
}

#[test]
fn move_cost_text_lists_rows_in_order_with_assumptions() {
    let output = run(&["move-cost", &snapshot_fixture_path()], None);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("coins scanned: 118"), "{text}");
    let position = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from:\n{text}"))
    };
    assert!(position("P2PK compressed") < position("P2PK uncompressed"));
    assert!(position("P2PK uncompressed") < position("bare multisig 1-of-1"));
    assert!(position("bare multisig 3-of-3") < position("P2TR"));
    assert!(position("coins of at least 546 sats:") > position("P2TR"));
    assert!(position("assumptions:") > position("coins of at least 546 sats:"));
}

#[test]
fn move_cost_on_a_file_that_is_not_a_snapshot_fails_with_the_reason() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../pqweight/tests/fixtures");
    let not_a_snapshot = dir.join("p2wpkh.hex");

    let output = run(&["move-cost", &not_a_snapshot.to_string_lossy()], None);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("not a UTXO snapshot"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn move_cost_without_a_path_prints_usage() {
    let output = run(&["move-cost"], None);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("pqweight move-cost"),
        "{}",
        stderr(&output)
    );
}

/// A block Fixture's hex and its known hash.
fn block_fixture(name: &str) -> (String, String) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../pqweight/tests/fixtures/block");
    let hex = std::fs::read_to_string(dir.join(format!("{name}.hex"))).unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap())
            .unwrap();
    (
        hex.trim().to_string(),
        meta["oracle"]["hash"].as_str().unwrap().to_string(),
    )
}

#[test]
fn aggregate_blocks_prints_a_row_per_block_and_what_over_the_limit_means() {
    let (regtest, regtest_hash) = block_fixture("regtest-block");
    let (block_170, block_170_hash) = block_fixture("block-170");

    let output = run(
        &["aggregate", "--scheme", "ml-dsa-44", "--blocks"],
        Some(&format!("{regtest}\n{block_170}\n")),
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("transactions parsed: 5"), "{text}");
    let regtest_row = row_after(&text, &regtest_hash);
    assert_eq!(regtest_row[0], "3", "{text}");
    assert!(regtest_row[3].starts_with('x'), "{text}");
    // Block 170's P2PK spend is Unmapped: no migrated weight, and the count.
    let block_170_row = row_after(&text, &block_170_hash);
    assert_eq!(block_170_row[..3], ["2", "1960", "-"], "{text}");
    assert!(text.contains("1 partially mapped"), "{text}");
    assert!(text.contains("wouldn't fit in one block"), "{text}");
}

#[test]
fn aggregate_blocks_json_has_a_row_per_block() {
    let (regtest, regtest_hash) = block_fixture("regtest-block");
    let (block_170, block_170_hash) = block_fixture("block-170");

    let json = aggregate_json(&["--blocks"], &format!("{regtest}\n{block_170}\n"));

    let blocks = json["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["hash"], regtest_hash.as_str());
    assert_eq!(blocks[0]["transactions"], 3);
    assert_eq!(blocks[0]["partially_mapped"], 0);
    let migrated = blocks[0]["migrated_weight"].as_u64().unwrap();
    let multiple = blocks[0]["migrated_limit_multiple"].as_f64().unwrap();
    let migrated = f64::from(u32::try_from(migrated).unwrap());
    assert!((multiple * 4_000_000.0 - migrated).abs() < 1e-6);
    assert_eq!(blocks[1]["hash"], block_170_hash.as_str());
    assert_eq!(blocks[1]["weight"], 1960);
    assert_eq!(blocks[1]["migrated_weight"], serde_json::Value::Null);
    assert_eq!(
        blocks[1]["migrated_limit_multiple"],
        serde_json::Value::Null
    );
    assert_eq!(blocks[1]["partially_mapped"], 1);
}

#[test]
fn aggregate_without_blocks_has_no_block_rows() {
    let json = aggregate_json(&[], &format!("{}\n", p2wpkh_fixture().hex));

    assert!(json.get("blocks").is_none(), "{json}");
}

#[test]
fn split_blocks_prints_each_transaction_and_each_block_hash_on_stderr() {
    let (regtest, regtest_hash) = block_fixture("regtest-block");
    let (block_170, block_170_hash) = block_fixture("block-170");

    let output = run(
        &["split-blocks"],
        Some(&format!("{regtest}\n\n{block_170}\n")),
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    // Every transaction of both blocks, in block order (the library's own
    // tests check `parse_block`'s split against Core and rust-bitcoin).
    let expected: Vec<String> = [&regtest, &block_170]
        .iter()
        .flat_map(|hex| {
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<u8>>();
            pqweight::parse_block(&bytes).unwrap().transactions
        })
        .map(|tx| {
            tx.iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
        })
        .collect();
    assert_eq!(lines, expected, "{text}");
    // Block 170's second transaction is the p2pk Fixture (Satoshi to Hal Finney).
    assert_eq!(lines[4], fixture("p2pk").hex);
    let err = stderr(&output);
    assert!(
        err.contains(&format!("{regtest_hash}: 3 transactions")),
        "{err}"
    );
    assert!(
        err.contains(&format!("{block_170_hash}: 2 transactions")),
        "{err}"
    );
}

#[test]
fn aggregating_split_blocks_gives_the_same_totals_as_aggregate_blocks() {
    let (regtest, _) = block_fixture("regtest-block");
    let (block_170, _) = block_fixture("block-170");
    let blocks = format!("{regtest}\n{block_170}\n");
    let split = run(&["split-blocks"], Some(&blocks));
    assert!(split.status.success(), "{}", stderr(&split));

    let by_transaction = aggregate_json(&["--fee-rate", "3"], &stdout(&split));
    let mut by_block = aggregate_json(&["--fee-rate", "3", "--blocks"], &blocks);

    by_block.as_object_mut().unwrap().remove("blocks");
    assert_eq!(by_block, by_transaction);
}

#[test]
fn split_blocks_stops_at_a_block_that_fails_verification() {
    let (regtest, _) = block_fixture("regtest-block");
    let (block_170, _) = block_fixture("block-170");
    // Flip the last hex digit: the last transaction's locktime.
    let mut corrupted = block_170.clone();
    let last = corrupted.pop().unwrap();
    corrupted.push(if last == '0' { '1' } else { '0' });

    let output = run(
        &["split-blocks"],
        Some(&format!("{regtest}\n{corrupted}\n")),
    );

    assert!(!output.status.success());
    assert_eq!(stdout(&output), "", "no partial output to feed other tools");
    let err = stderr(&output);
    assert!(err.contains("line 2"), "{err}");
    assert!(err.contains("merkle root mismatch"), "{err}");
}
