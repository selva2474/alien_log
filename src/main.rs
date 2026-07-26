//! `alien_log` CLI (roadmap P1.5).
//!
//! Three subcommands make up the walking skeleton:
//! - `serve`  — run the ingest/search HTTP server.
//! - `send`   — read logs from stdin (NDJSON or plain text) and POST them.
//! - `search` — query the server and print matching logs.

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;

use alien_log::client;
use alien_log::record::LogRecord;
use alien_log::server;
use alien_log::store::Store;
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "alien_log",
    version,
    about = "A developer-first, single-binary log aggregator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the ingest + search HTTP server.
    Serve(ServeArgs),
    /// Read logs from stdin and send them to a running server.
    Send(SendArgs),
    /// Search logs on a running server.
    Search(SearchArgs),
}

#[derive(Args)]
struct ServeArgs {
    /// Address to bind, host:port.
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: String,
    /// Path to the write-ahead log file.
    #[arg(long, default_value = "alien_log.wal")]
    wal: String,
}

#[derive(Args)]
struct SendArgs {
    /// Server address, host:port.
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: String,
}

#[derive(Args)]
struct SearchArgs {
    /// Free-text substring to match (body + attributes).
    query: Option<String>,
    /// Filter by severity (e.g. error, warn, info).
    #[arg(long)]
    level: Option<String>,
    /// Maximum results.
    #[arg(long, default_value_t = 100)]
    limit: usize,
    /// Server address, host:port.
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: String,
    /// Print raw JSON records instead of a formatted line.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Serve(args) => run_serve(args),
        Command::Send(args) => run_send(args),
        Command::Search(args) => run_search(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("alien_log: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_serve(args: ServeArgs) -> Result<(), String> {
    let store = Arc::new(Store::open(&args.wal).map_err(|e| e.to_string())?);
    let server = server::bind(&args.addr).map_err(|e| e.to_string())?;
    eprintln!(
        "alien_log serving on http://{} (wal: {}, {} record(s) replayed)",
        args.addr,
        store.wal_path().display(),
        store.len(),
    );
    server::serve(server, store);
    Ok(())
}

fn run_send(args: SendArgs) -> Result<(), String> {
    // Read stdin fully, then POST as one NDJSON batch.
    let stdin = std::io::stdin();
    let mut batch = String::new();
    let mut count = 0usize;
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        batch.push_str(&line);
        batch.push('\n');
        count += 1;
    }
    if count == 0 {
        eprintln!("alien_log: nothing on stdin to send");
        return Ok(());
    }
    let resp = client::post(&args.addr, "/ingest", &batch).map_err(|e| e.to_string())?;
    if !resp.is_success() {
        return Err(format!(
            "ingest failed ({}): {}",
            resp.status,
            resp.body.trim()
        ));
    }
    eprintln!("alien_log: sent {count} line(s)");
    Ok(())
}

fn run_search(args: SearchArgs) -> Result<(), String> {
    let mut path = String::from("/search?");
    let mut params: Vec<String> = Vec::new();
    if let Some(q) = &args.query {
        params.push(format!("q={}", url_encode(q)));
    }
    if let Some(level) = &args.level {
        params.push(format!("level={}", url_encode(level)));
    }
    params.push(format!("limit={}", args.limit));
    path.push_str(&params.join("&"));

    let resp = client::get(&args.addr, &path).map_err(|e| e.to_string())?;
    if !resp.is_success() {
        return Err(format!(
            "search failed ({}): {}",
            resp.status,
            resp.body.trim()
        ));
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in resp.body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if args.json {
            let _ = writeln!(out, "{line}");
            continue;
        }
        match serde_json::from_str::<LogRecord>(line) {
            Ok(rec) => {
                let _ = writeln!(out, "{}", format_record(&rec));
            }
            Err(_) => {
                let _ = writeln!(out, "{line}");
            }
        }
    }
    Ok(())
}

/// Human-friendly one-line rendering: `<ts> <LEVEL> <body>`.
fn format_record(rec: &LogRecord) -> String {
    format!(
        "{:>20} {:<5} {}",
        rec.timestamp_nanos,
        format!("{:?}", rec.severity).to_uppercase(),
        rec.body
    )
}

/// Percent-encode a query-parameter value (spaces and reserved chars).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
