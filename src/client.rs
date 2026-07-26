//! A tiny blocking HTTP/1.1 client for the CLI (roadmap P1.5).
//!
//! Scope is deliberately narrow: plaintext HTTP to a localhost `alien_log` server. We
//! hand-roll it over `TcpStream` so the binary pulls in no TLS/HTTP-client dependency —
//! keeping the "one small static binary" promise. Anything beyond localhost (TLS, auth)
//! is a later concern.

use std::io::{Read, Write};
use std::net::TcpStream;

/// A parsed HTTP response.
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// `POST` `body` to `http://{addr}{path}`.
pub fn post(addr: &str, path: &str, body: &str) -> std::io::Result<HttpResponse> {
    request(addr, "POST", path, Some(body))
}

/// `GET` `http://{addr}{path}`.
pub fn get(addr: &str, path: &str) -> std::io::Result<HttpResponse> {
    request(addr, "GET", path, None)
}

fn request(
    addr: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> std::io::Result<HttpResponse> {
    let host = addr.trim_start_matches("http://");
    let mut stream = TcpStream::connect(host)?;

    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(b) = body {
        req.push_str(&format!(
            "Content-Type: application/x-ndjson\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes())?;
    if let Some(b) = body {
        stream.write_all(b.as_bytes())?;
    }
    stream.flush()?;

    // Connection: close means the server closes after the body, so read to EOF.
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> std::io::Result<HttpResponse> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));

    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| std::io::Error::other("malformed HTTP status line"))?;

    Ok(HttpResponse {
        status,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let resp = parse_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "hello");
        assert!(resp.is_success());
    }

    #[test]
    fn parses_error_status() {
        let raw = b"HTTP/1.1 404 Not Found\r\n\r\nnope";
        let resp = parse_response(raw).unwrap();
        assert_eq!(resp.status, 404);
        assert!(!resp.is_success());
    }
}
