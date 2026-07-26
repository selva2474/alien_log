//! End-to-end walking-skeleton test (roadmap P1.6): boot a real server on an ephemeral
//! port, POST logs over HTTP, search them back over HTTP, and assert on the hits.

use std::sync::Arc;
use std::thread;

use alien_log::{client, server, store::Store};

/// Start a server on 127.0.0.1:0 (OS-assigned port) backed by a throwaway WAL, and
/// return its `host:port` address plus the WAL path so the caller can clean up.
fn start_server(tag: &str) -> (String, std::path::PathBuf) {
    let mut wal = std::env::temp_dir();
    wal.push(format!("alien_log_e2e_{}_{}.wal", tag, std::process::id()));
    let _ = std::fs::remove_file(&wal);

    let store = Arc::new(Store::open(&wal).unwrap());
    let server = server::bind("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_ip().unwrap().to_string();

    thread::spawn(move || server::serve(server, store));
    (addr, wal)
}

#[test]
fn ingest_then_search_over_http() {
    let (addr, wal) = start_server("basic");

    let batch = "\
{\"level\":\"info\",\"message\":\"server started\",\"service\":\"api\"}
{\"level\":\"error\",\"message\":\"connection timeout\",\"service\":\"api\"}
plain text warning line
{\"level\":\"error\",\"message\":\"disk full\",\"service\":\"worker\"}
";

    let resp = client::post(&addr, "/ingest", batch).unwrap();
    assert!(resp.is_success(), "ingest status {}", resp.status);
    assert!(resp.body.contains("\"ingested\":4"), "body: {}", resp.body);

    // Full-text search: "timeout" should hit exactly one line.
    let resp = client::get(&addr, "/search?q=timeout").unwrap();
    assert!(resp.is_success());
    let lines: Vec<&str> = resp.body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "expected 1 hit, got: {:?}", lines);
    assert!(lines[0].contains("connection timeout"));

    // Level filter: two error lines.
    let resp = client::get(&addr, "/search?level=error").unwrap();
    let errors: Vec<&str> = resp.body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(errors.len(), 2, "expected 2 errors, got: {:?}", errors);

    // Attribute value is searchable too.
    let resp = client::get(&addr, "/search?q=worker").unwrap();
    let worker: Vec<&str> = resp.body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(worker.len(), 1);
    assert!(worker[0].contains("disk full"));

    // Plain-text line was ingested as a body.
    let resp = client::get(&addr, "/search?q=plain").unwrap();
    assert_eq!(resp.body.lines().filter(|l| !l.is_empty()).count(), 1);

    std::fs::remove_file(&wal).ok();
}

#[test]
fn health_endpoint_responds() {
    let (addr, wal) = start_server("health");
    let resp = client::get(&addr, "/health").unwrap();
    assert!(resp.is_success());
    assert_eq!(resp.body.trim(), "ok");
    std::fs::remove_file(&wal).ok();
}
