//! alien_log — a developer-first, single-binary log aggregator.
//!
//! See `docs/ROADMAP.md` for the plan. Phase 1 (walking skeleton) wires these modules
//! into an end-to-end ingest → store → search pipeline:
//!
//! - [`record`] — the internal log record model
//! - [`parse`]  — NDJSON / schema-on-read ingest parsing
//! - [`store`]  — in-memory store + WAL, with search
//! - [`server`] — HTTP ingest/search endpoints
//! - [`client`] — tiny HTTP client used by the CLI

pub mod client;
pub mod parse;
pub mod record;
pub mod server;
pub mod store;

pub use record::{LogRecord, Severity};
pub use store::{Query, Store};
