//! The `mcp` subcommand, run as the real binary an MCP client launches.
//!
//! What only the process can show: standard output carries protocol messages
//! and nothing else, whatever the application logs, with or without the
//! write tools; writes are offered only with `--allow-writes`; and a machine
//! where the application never ran gets an error rather than a fresh
//! database.
//!
//! Linux only, because the platform data directory is redirected through
//! `XDG_DATA_HOME`; the server code under test is shared by every platform.

#![cfg(target_os = "linux")]

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};

use serde_json::Value;

fn mcp_command(data_home: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oxidgene-desktop"));
    command
        .args(args)
        .env("XDG_DATA_HOME", data_home)
        // Verbose on purpose: every log line must still stay off stdout.
        .env("OXIDGENE_LOG_LEVEL", "debug")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Run a session of the command line `args` over a fresh database:
/// initialize, then send each of `requests` (a method and its params) with
/// ids from 2 on.
///
/// Asserts that every line of standard output is a protocol message, that
/// the session ends cleanly once its input closes, and that the logs went to
/// standard error. Returns the answers, in order.
fn session(args: &[&str], requests: &[&str]) -> Vec<Value> {
    let data_home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data_home.path().join("oxidgene")).unwrap();
    // An empty file stands for a database the application created: the
    // subcommand applies the migrations to it.
    std::fs::File::create(data_home.path().join("oxidgene/oxidgene.db")).unwrap();

    let mut child = mcp_command(data_home.path(), args).spawn().unwrap();
    // Drained as it comes: at debug level the server logs every answer, a
    // whole tool list included, and a full pipe would stall it mid-session.
    let mut stderr = child.stderr.take().unwrap();
    let logs = std::thread::spawn(move || {
        let mut logs = Vec::new();
        std::io::Read::read_to_end(&mut stderr, &mut logs).unwrap();
        logs
    });
    let mut stdin = child.stdin.take().unwrap();
    for message in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    ] {
        writeln!(stdin, "{message}").unwrap();
    }
    for (id, request) in (2..).zip(requests) {
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":{id},{request}}}"#).unwrap();
    }

    // Every line is a protocol message, up to the answers to every request.
    // The server handles requests concurrently, so they may come in any
    // order.
    let mut answers = std::collections::BTreeMap::new();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    while answers.len() < requests.len() {
        let Some(line) = lines.next() else {
            break;
        };
        let line = line.unwrap();
        let message: Value = serde_json::from_str(&line)
            .unwrap_or_else(|_| panic!("not a protocol message: {line}"));
        assert_eq!(message["jsonrpc"], "2.0");
        if let Some(id) = message["id"].as_u64().filter(|id| *id >= 2) {
            answers.insert(id, message);
        }
    }

    // Closing input ends the session cleanly, before any assertion can leave
    // the process waiting on it.
    drop(stdin);
    for line in lines {
        let line = line.unwrap();
        assert!(
            serde_json::from_str::<Value>(&line).is_ok(),
            "not a protocol message: {line}"
        );
    }
    assert_eq!(answers.len(), requests.len());
    let status = child.wait().unwrap();
    assert!(status.success(), "exit status {status}");

    // The logs went somewhere, just not to stdout.
    assert!(!logs.join().unwrap().is_empty());
    answers.into_values().collect()
}

const LIST_TOOLS: &str = r#""method":"tools/list","params":{}"#;

fn offers(answer: &Value, tool: &str) -> bool {
    answer["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|listed| listed["name"] == tool)
}

#[test]
fn a_session_writes_only_protocol_messages_to_stdout() {
    let answers = session(
        &["mcp"],
        &[
            r#""method":"tools/call","params":{"name":"list_trees","arguments":{}}"#,
            LIST_TOOLS,
        ],
    );
    assert_eq!(answers[0]["result"]["structuredContent"]["total_count"], 0);
    // Read-only unless asked: no write tool is offered.
    assert!(offers(&answers[1], "get_kinship"));
    assert!(!offers(&answers[1], "create_person"));
}

#[test]
fn a_session_allowing_writes_offers_them_and_keeps_stdout_clean() {
    let answers = session(
        &["mcp", "--allow-writes"],
        &[
            LIST_TOOLS,
            // A write naming no existing tree runs the write path, its logs
            // included, and answers a tool error.
            r#""method":"tools/call","params":{"name":"create_person","arguments":{"tree_id":"01900000-0000-7000-8000-000000000000","sex":"female"}}"#,
        ],
    );
    assert!(offers(&answers[0], "create_person"));
    assert!(offers(&answers[0], "delete_person"));
    assert_eq!(answers[1]["result"]["isError"], true);
    assert_eq!(
        answers[1]["result"]["structuredContent"]["error"],
        "not_found"
    );
}

#[test]
fn allow_writes_is_refused_outside_the_mcp_subcommand() {
    let data_home = tempfile::tempdir().unwrap();

    let output = mcp_command(data_home.path(), &["--allow-writes"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!data_home.path().join("oxidgene").exists());
}

#[test]
fn no_database_is_created_where_the_application_never_ran() {
    let data_home = tempfile::tempdir().unwrap();

    let output = mcp_command(data_home.path(), &["mcp"]).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!data_home.path().join("oxidgene/oxidgene.db").exists());
}
