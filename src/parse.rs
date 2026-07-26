//! Ingest wire format + parsing (roadmap P1.1, and a first cut of P2.2).
//!
//! The MVP wire format is **NDJSON** (newline-delimited): one log per line. A line is
//! parsed with schema-on-read semantics — we never require a fixed schema:
//!
//! - If the line is a JSON **object**, we lift a few well-known keys (`body`/`message`,
//!   `severity`/`level`, `timestamp_nanos`, `trace_id`, `span_id`) and fold every other
//!   scalar field into `attributes`.
//! - Anything else (plain text, a JSON scalar) becomes the `body` verbatim.
//!
//! When the line carries no usable timestamp we stamp `ingest_time_nanos` — supplied by
//! the caller so parsing stays pure and testable.

use crate::record::{LogRecord, Severity};
use serde_json::Value;

/// Keys we recognize as the message body, in priority order.
const BODY_KEYS: &[&str] = &["body", "message", "msg", "log"];
/// Keys we recognize as the severity/level, in priority order.
const LEVEL_KEYS: &[&str] = &["severity", "level", "lvl", "loglevel"];

/// Parse a single raw ingest line into a [`LogRecord`].
///
/// `ingest_time_nanos` is used as the timestamp only when the line supplies none.
pub fn parse_line(raw: &str, ingest_time_nanos: i64) -> LogRecord {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => parse_object(map, ingest_time_nanos, raw),
        // A JSON scalar or non-object, or not JSON at all: treat the line as the body.
        _ => LogRecord::new(ingest_time_nanos, raw),
    }
}

fn parse_object(
    mut map: serde_json::Map<String, Value>,
    ingest_time_nanos: i64,
    raw: &str,
) -> LogRecord {
    let timestamp_nanos = map
        .remove("timestamp_nanos")
        .and_then(|v| v.as_i64())
        .unwrap_or(ingest_time_nanos);

    let body = take_first(&mut map, BODY_KEYS)
        .map(value_to_string)
        // No recognizable body key: keep the original JSON so nothing is lost.
        .unwrap_or_else(|| raw.to_string());

    let severity = take_first(&mut map, LEVEL_KEYS)
        .map(|v| Severity::parse(&value_to_string(v)))
        .unwrap_or(Severity::Unknown);

    let trace_id = map.remove("trace_id").map(value_to_string);
    let span_id = map.remove("span_id").map(value_to_string);

    // Everything left over is a per-event attribute. Nested objects/arrays are kept as
    // their JSON text for now (structured extraction is a later query-time concern).
    let mut record = LogRecord::new(timestamp_nanos, body);
    record.severity = severity;
    record.trace_id = trace_id;
    record.span_id = span_id;
    for (k, v) in map {
        record.attributes.insert(k, value_to_string(v));
    }
    record
}

/// Remove and return the first present key from `keys`.
fn take_first(map: &mut serde_json::Map<String, Value>, keys: &[&str]) -> Option<Value> {
    keys.iter().find_map(|k| map.remove(*k))
}

/// Render a JSON value as a plain string without wrapping strings in quotes.
fn value_to_string(v: Value) -> String {
    match v {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_becomes_body_with_ingest_time() {
        let rec = parse_line("connection refused", 42);
        assert_eq!(rec.body, "connection refused");
        assert_eq!(rec.timestamp_nanos, 42);
        assert_eq!(rec.severity, Severity::Unknown);
    }

    #[test]
    fn json_object_lifts_known_fields() {
        let raw = r#"{"level":"error","message":"boom","service":"api","timestamp_nanos":100}"#;
        let rec = parse_line(raw, 7);
        assert_eq!(rec.body, "boom");
        assert_eq!(rec.severity, Severity::Error);
        assert_eq!(rec.timestamp_nanos, 100);
        assert_eq!(rec.attributes.get("service").unwrap(), "api");
    }

    #[test]
    fn json_without_timestamp_uses_ingest_time() {
        let rec = parse_line(r#"{"msg":"hi"}"#, 999);
        assert_eq!(rec.body, "hi");
        assert_eq!(rec.timestamp_nanos, 999);
    }

    #[test]
    fn json_object_without_body_key_keeps_raw() {
        let raw = r#"{"service":"api","code":500}"#;
        let rec = parse_line(raw, 1);
        assert_eq!(rec.body, raw);
        assert_eq!(rec.attributes.get("code").unwrap(), "500");
    }

    #[test]
    fn trace_correlation_is_extracted() {
        let raw = r#"{"body":"x","trace_id":"abc","span_id":"def"}"#;
        let rec = parse_line(raw, 1);
        assert_eq!(rec.trace_id.as_deref(), Some("abc"));
        assert_eq!(rec.span_id.as_deref(), Some("def"));
    }
}
