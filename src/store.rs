//! In-memory store with a durable write-ahead log (roadmap P1.3) and basic search
//! (P1.4).
//!
//! This is deliberately the simplest thing that survives a restart: every appended
//! record is written as one NDJSON line to the WAL, and on startup we replay the WAL
//! back into memory. Columnar segments on object storage (Phase 2) will replace the
//! in-memory `Vec`, but the ingest/search API here is meant to stay stable.

use crate::record::{LogRecord, Severity};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Query parameters for [`Store::search`].
#[derive(Debug, Default, Clone)]
pub struct Query {
    /// Case-insensitive substring matched against body, attribute values, and resource
    /// values. `None` matches everything.
    pub text: Option<String>,
    /// Exact severity filter, when set.
    pub level: Option<Severity>,
    /// Inclusive lower bound on `timestamp_nanos`.
    pub since_nanos: Option<i64>,
    /// Exclusive upper bound on `timestamp_nanos`.
    pub until_nanos: Option<i64>,
    /// Maximum number of records to return (most recent first).
    pub limit: usize,
}

impl Query {
    pub const DEFAULT_LIMIT: usize = 100;
}

/// A thread-safe append-and-search store backed by an on-disk WAL.
pub struct Store {
    records: Mutex<Vec<LogRecord>>,
    wal: Mutex<BufWriter<File>>,
    wal_path: PathBuf,
}

impl Store {
    /// Open (creating if needed) a store backed by the WAL at `wal_path`, replaying any
    /// existing contents into memory.
    pub fn open(wal_path: impl AsRef<Path>) -> std::io::Result<Store> {
        let wal_path = wal_path.as_ref().to_path_buf();
        let records = replay_wal(&wal_path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;
        Ok(Store {
            records: Mutex::new(records),
            wal: Mutex::new(BufWriter::new(file)),
            wal_path,
        })
    }

    /// Number of records currently held in memory.
    pub fn len(&self) -> usize {
        self.records.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The path of the backing WAL file.
    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    /// Append one record: persist it to the WAL (flushed) then hold it in memory.
    ///
    /// The WAL write happens first so a record is never searchable without also being
    /// durable.
    pub fn append(&self, record: LogRecord) -> std::io::Result<()> {
        let line = serde_json::to_string(&record).expect("LogRecord serializes");
        {
            let mut wal = self.wal.lock().unwrap();
            wal.write_all(line.as_bytes())?;
            wal.write_all(b"\n")?;
            wal.flush()?;
        }
        self.records.lock().unwrap().push(record);
        Ok(())
    }

    /// Search the store, returning matches most-recent-first, capped at `query.limit`
    /// (or [`Query::DEFAULT_LIMIT`] when zero).
    pub fn search(&self, query: &Query) -> Vec<LogRecord> {
        let needle = query.text.as_ref().map(|t| t.to_lowercase());
        let limit = if query.limit == 0 {
            Query::DEFAULT_LIMIT
        } else {
            query.limit
        };

        let records = self.records.lock().unwrap();
        records
            .iter()
            .rev() // most recent first
            .filter(|r| matches(r, needle.as_deref(), query))
            .take(limit)
            .cloned()
            .collect()
    }
}

fn matches(record: &LogRecord, needle: Option<&str>, query: &Query) -> bool {
    if let Some(level) = query.level {
        if record.severity != level {
            return false;
        }
    }
    if let Some(since) = query.since_nanos {
        if record.timestamp_nanos < since {
            return false;
        }
    }
    if let Some(until) = query.until_nanos {
        if record.timestamp_nanos >= until {
            return false;
        }
    }
    if let Some(needle) = needle {
        if !text_contains(record, needle) {
            return false;
        }
    }
    true
}

/// Case-insensitive substring search across body + attribute/resource values.
fn text_contains(record: &LogRecord, needle_lower: &str) -> bool {
    if record.body.to_lowercase().contains(needle_lower) {
        return true;
    }
    record
        .attributes
        .values()
        .chain(record.resource.values())
        .any(|v| v.to_lowercase().contains(needle_lower))
}

/// Read an existing WAL back into memory. A missing WAL is an empty store; a corrupt
/// trailing line (e.g. from a crash mid-write) is skipped rather than fatal.
fn replay_wal(path: &Path) -> std::io::Result<Vec<LogRecord>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<LogRecord>(&line) {
            Ok(rec) => records.push(rec),
            Err(_) => continue, // tolerate a torn final write
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rec(ts: i64, body: &str, sev: Severity) -> LogRecord {
        let mut r = LogRecord::new(ts, body);
        r.severity = sev;
        r
    }

    fn temp_wal(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "alien_log_test_{}_{}.wal",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn append_then_search_finds_by_text() {
        let path = temp_wal("text");
        let store = Store::open(&path).unwrap();
        store
            .append(rec(1, "connection timeout", Severity::Error))
            .unwrap();
        store.append(rec(2, "all good", Severity::Info)).unwrap();

        let hits = store.search(&Query {
            text: Some("TIMEOUT".into()), // case-insensitive
            ..Default::default()
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].body, "connection timeout");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn level_filter_applies() {
        let path = temp_wal("level");
        let store = Store::open(&path).unwrap();
        store.append(rec(1, "a", Severity::Error)).unwrap();
        store.append(rec(2, "b", Severity::Info)).unwrap();

        let hits = store.search(&Query {
            level: Some(Severity::Error),
            ..Default::default()
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].body, "a");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn results_are_most_recent_first_and_limited() {
        let path = temp_wal("order");
        let store = Store::open(&path).unwrap();
        for i in 0..5 {
            store
                .append(rec(i, &format!("line{i}"), Severity::Info))
                .unwrap();
        }
        let hits = store.search(&Query {
            limit: 2,
            ..Default::default()
        });
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].body, "line4");
        assert_eq!(hits[1].body, "line3");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn wal_replays_across_reopen() {
        let path = temp_wal("replay");
        {
            let store = Store::open(&path).unwrap();
            let mut r = rec(1, "durable", Severity::Warn);
            r.attributes.insert("k".into(), "v".into());
            store.append(r).unwrap();
        }
        // Reopen: the record should come back from the WAL.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.len(), 1);
        let hits = store.search(&Query::default());
        assert_eq!(hits[0].body, "durable");
        assert_eq!(hits[0].attributes.get("k").unwrap(), "v");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn time_range_filters() {
        let path = temp_wal("time");
        let store = Store::open(&path).unwrap();
        for i in 0..5 {
            store
                .append(rec(i, &format!("l{i}"), Severity::Info))
                .unwrap();
        }
        let hits = store.search(&Query {
            since_nanos: Some(1),
            until_nanos: Some(3),
            ..Default::default()
        });
        // timestamps 1 and 2 (until is exclusive)
        let bodies: BTreeMap<_, _> = hits.iter().map(|r| (r.timestamp_nanos, ())).collect();
        assert_eq!(bodies.len(), 2);
        assert!(bodies.contains_key(&1) && bodies.contains_key(&2));
        std::fs::remove_file(&path).ok();
    }
}
