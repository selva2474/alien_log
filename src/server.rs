//! HTTP ingest + search server (roadmap P1.2 / P1.4).
//!
//! Endpoints (all tiny, blocking, no async runtime — matches the single-binary ethos):
//! - `POST /ingest`  — body is NDJSON; each line is parsed and appended. Returns
//!   `{"ingested": N}`.
//! - `GET  /search`  — query params `q`, `level`, `since`, `until`, `limit`. Returns
//!   NDJSON, one [`LogRecord`] per line, most recent first.
//! - `GET  /health`  — liveness check.

use crate::parse::parse_line;
use crate::record::Severity;
use crate::store::{Query, Store};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Method, Request, Response, Server};

/// Current wall-clock time as Unix epoch nanoseconds (used to stamp timestamp-less
/// records at ingest).
pub fn now_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Bind an HTTP server to `addr` (e.g. `"127.0.0.1:8080"`).
pub fn bind(addr: &str) -> std::io::Result<Server> {
    Server::http(addr).map_err(|e| std::io::Error::other(e.to_string()))
}

/// Serve requests forever against `store`. Blocks the calling thread.
pub fn serve(server: Server, store: Arc<Store>) {
    for request in server.incoming_requests() {
        handle(request, &store);
    }
}

fn handle(mut request: Request, store: &Store) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();

    let result = match (&method, path.as_str()) {
        (Method::Post, "/ingest") => handle_ingest(&mut request, store),
        (Method::Get, "/search") => handle_search(&url, store),
        (Method::Get, "/health") => Ok((200, "ok\n".to_string())),
        _ => Ok((404, "not found\n".to_string())),
    };

    let (code, body) = result.unwrap_or_else(|e| (400, format!("error: {e}\n")));
    let response = Response::from_string(body).with_status_code(code);
    let _ = request.respond(response);
}

fn handle_ingest(request: &mut Request, store: &Store) -> Result<(u16, String), String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;

    let now = now_nanos();
    let mut count = 0usize;
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let record = parse_line(line, now);
        store.append(record).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok((200, format!("{{\"ingested\":{count}}}\n")))
}

fn handle_search(url: &str, store: &Store) -> Result<(u16, String), String> {
    let query = parse_query(url);
    let hits = store.search(&query);
    let mut out = String::new();
    for rec in hits {
        out.push_str(&serde_json::to_string(&rec).map_err(|e| e.to_string())?);
        out.push('\n');
    }
    Ok((200, out))
}

/// Parse the query string of a `/search` URL into a [`Query`].
fn parse_query(url: &str) -> Query {
    let mut query = Query::default();
    let qs = match url.split_once('?') {
        Some((_, qs)) => qs,
        None => return query,
    };
    for pair in qs.split('&') {
        let (key, val) = match pair.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        let val = percent_decode(val);
        match key {
            "q" | "query" => query.text = Some(val),
            "level" | "severity" => {
                let sev = Severity::parse(&val);
                if sev != Severity::Unknown {
                    query.level = Some(sev);
                }
            }
            "since" => query.since_nanos = val.parse().ok(),
            "until" => query.until_nanos = val.parse().ok(),
            "limit" => {
                if let Ok(n) = val.parse() {
                    query.limit = n;
                }
            }
            _ => {}
        }
    }
    query
}

/// Minimal `application/x-www-form-urlencoded` decoding: `+` → space and `%XX` → byte.
/// Sufficient for our localhost CLI; a full URL crate is overkill for the MVP.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push(h << 4 | l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_reads_all_params() {
        let q = parse_query("/search?q=timeout&level=error&limit=5&since=10&until=20");
        assert_eq!(q.text.as_deref(), Some("timeout"));
        assert_eq!(q.level, Some(Severity::Error));
        assert_eq!(q.limit, 5);
        assert_eq!(q.since_nanos, Some(10));
        assert_eq!(q.until_nanos, Some(20));
    }

    #[test]
    fn parse_query_decodes_percent_and_plus() {
        let q = parse_query("/search?q=connection%20refused");
        assert_eq!(q.text.as_deref(), Some("connection refused"));
        let q = parse_query("/search?q=a+b");
        assert_eq!(q.text.as_deref(), Some("a b"));
    }

    #[test]
    fn parse_query_empty_is_default() {
        let q = parse_query("/search");
        assert!(q.text.is_none());
        assert_eq!(q.limit, 0);
    }

    #[test]
    fn unknown_level_is_ignored() {
        let q = parse_query("/search?level=bogus");
        assert_eq!(q.level, None);
    }
}
