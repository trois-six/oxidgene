//! The `mcp` subcommand, run as the real binary an MCP client launches.
//!
//! What only the process can show: standard output carries protocol messages
//! and nothing else, whatever the application logs, and a machine where the
//! application never ran gets an error rather than a fresh database.
//!
//! Linux only, because the platform data directory is redirected through
//! `XDG_DATA_HOME`; the server code under test is shared by every platform.

#![cfg(target_os = "linux")]

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};

use serde_json::Value;

fn mcp_command(data_home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oxidgene-desktop"));
    command
        .arg("mcp")
        .env("XDG_DATA_HOME", data_home)
        // Verbose on purpose: every log line must still stay off stdout.
        .env("OXIDGENE_LOG_LEVEL", "debug")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn a_session_writes_only_protocol_messages_to_stdout() {
    let data_home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data_home.path().join("oxidgene")).unwrap();
    // An empty file stands for a database the application created: the
    // subcommand applies the migrations to it.
    std::fs::File::create(data_home.path().join("oxidgene/oxidgene.db")).unwrap();

    let mut child = mcp_command(data_home.path()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_trees","arguments":{}}}"#,
    ] {
        writeln!(stdin, "{message}").unwrap();
    }

    // Every line is a protocol message, up to the answer to the tool call.
    let mut answer = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        let message: Value = serde_json::from_str(&line)
            .unwrap_or_else(|_| panic!("not a protocol message: {line}"));
        assert_eq!(message["jsonrpc"], "2.0");
        if message["id"] == 2 {
            answer = Some(message);
            break;
        }
    }
    let answer = answer.expect("list_trees answered");
    assert_eq!(answer["result"]["structuredContent"]["total_count"], 0);

    // Closing input ends the session cleanly.
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "exit status {}", output.status);

    // The logs went somewhere, just not to stdout.
    assert!(!output.stderr.is_empty());
}

#[test]
fn no_database_is_created_where_the_application_never_ran() {
    let data_home = tempfile::tempdir().unwrap();

    let output = mcp_command(data_home.path()).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!data_home.path().join("oxidgene/oxidgene.db").exists());
}
