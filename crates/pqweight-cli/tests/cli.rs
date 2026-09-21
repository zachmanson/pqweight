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
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../pqweight/tests/fixtures");
    let hex = std::fs::read_to_string(dir.join("p2wpkh.hex")).unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("p2wpkh.json")).unwrap()).unwrap();
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
