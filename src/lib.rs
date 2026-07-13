//! alien_log — a developer-first, single-binary log aggregator.
//!
//! See `docs/ROADMAP.md` for the plan. This crate is at Phase 0/1: the core record
//! model exists; ingest, store, and query land next.

pub mod record;

pub use record::{LogRecord, Severity};
