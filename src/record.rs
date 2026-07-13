//! The internal log record model (roadmap P0.9).
//!
//! Design goals:
//! - **Schema-on-read friendly.** A record always has a timestamp and a body; every
//!   other structured field lives in `attributes`, so we never force callers to design
//!   an index up front (this is our answer to Loki's cardinality footgun).
//! - **OTel-shaped.** Field names mirror the OpenTelemetry log data model so an OTLP
//!   receiver (roadmap P2.1) can map onto this with no impedance mismatch.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Severity, aligned to the OpenTelemetry severity buckets. We keep a small, stable
/// enum rather than raw ints so the CLI can filter with `level=error` ergonomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
    /// Severity could not be determined from the incoming log.
    #[default]
    Unknown,
}

impl Severity {
    /// Best-effort parse of a free-form level string (e.g. "WARN", "warning", "err").
    pub fn parse(raw: &str) -> Severity {
        match raw.trim().to_ascii_lowercase().as_str() {
            "trace" => Severity::Trace,
            "debug" => Severity::Debug,
            "info" | "information" | "notice" => Severity::Info,
            "warn" | "warning" => Severity::Warn,
            "error" | "err" => Severity::Error,
            "fatal" | "critical" | "crit" | "panic" => Severity::Fatal,
            _ => Severity::Unknown,
        }
    }
}

/// A single log record — the unit that flows ingest → store → query.
///
/// `attributes` and `resource` are intentionally untyped string maps for the MVP:
/// structure is discovered at query time, not enforced at ingest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// Event time, Unix epoch nanoseconds. When the source omits it, the receiver
    /// stamps ingest time.
    pub timestamp_nanos: i64,

    /// The raw log message / body.
    pub body: String,

    /// Parsed severity; `Unknown` when the source gives no usable level.
    #[serde(default)]
    pub severity: Severity,

    /// Per-event structured fields (from JSON/logfmt parsing or OTLP attributes).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,

    /// Fields describing the *source* of the logs (service.name, host, k8s pod, …).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource: BTreeMap<String, String>,

    /// Trace correlation, when present (hex-encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
}

impl LogRecord {
    /// Construct a minimal record from a timestamp and body; everything else empty.
    pub fn new(timestamp_nanos: i64, body: impl Into<String>) -> Self {
        LogRecord {
            timestamp_nanos,
            body: body.into(),
            severity: Severity::Unknown,
            attributes: BTreeMap::new(),
            resource: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_parse_is_forgiving() {
        assert_eq!(Severity::parse("WARN"), Severity::Warn);
        assert_eq!(Severity::parse(" warning "), Severity::Warn);
        assert_eq!(Severity::parse("err"), Severity::Error);
        assert_eq!(Severity::parse("critical"), Severity::Fatal);
        assert_eq!(Severity::parse("nonsense"), Severity::Unknown);
    }

    #[test]
    fn severity_orders_low_to_high() {
        assert!(Severity::Debug < Severity::Error);
    }

    #[test]
    fn record_roundtrips_through_json() {
        let mut rec = LogRecord::new(1_700_000_000_000_000_000, "connection timeout");
        rec.severity = Severity::Error;
        rec.attributes.insert("service".into(), "api".into());

        let json = serde_json::to_string(&rec).unwrap();
        let back: LogRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(rec, back);
    }

    #[test]
    fn empty_maps_are_omitted_from_json() {
        let rec = LogRecord::new(0, "hi");
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("attributes"));
        assert!(!json.contains("resource"));
    }
}
