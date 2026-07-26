# alien_log

A developer-first, single-binary log aggregator.

**Wedge:** cheap and predictable to run, OTLP-native (planned), object-storage backed
(planned), with *one* dead-simple search that never makes you think about cardinality.

📍 Plan & progress: [**docs/ROADMAP.md**](docs/ROADMAP.md) — competitive analysis,
pain-point mapping, architecture, and the phased task breakdown.

Status: **Phase 1 complete** — an end-to-end ingest → store → search walking skeleton.

## Quickstart — index your first logs in 30 seconds

```bash
# 1. build
cargo build --release

# 2. run the server (ingest + search, backed by a local WAL)
./target/release/alien_log serve &

# 3. pipe some logs in (NDJSON or plain text — both work)
printf '%s\n' \
  '{"level":"info","message":"boot complete","service":"api"}' \
  '{"level":"error","message":"payment gateway timeout","order_id":42}' \
  'a plain text line works too' \
  | ./target/release/alien_log send

# 4. search
./target/release/alien_log search timeout        # full-text
./target/release/alien_log search --level error  # by severity
./target/release/alien_log search 42             # matches attribute values too
./target/release/alien_log search --json         # raw JSON records
```

Logs are durable: they're written to a WAL (`alien_log.wal` by default) and replayed on
restart.

## How it works today

```
 send (stdin) ──POST /ingest──▶ server ──▶ WAL + in-memory store
 search (CLI) ──GET  /search──▶ server ──▶ filter (text/level/time) ──▶ NDJSON
```

- **Ingest format:** newline-delimited. Each line is a JSON object (well-known keys like
  `message`/`level`/`timestamp_nanos` are lifted; everything else becomes searchable
  `attributes`) or plain text (used as the body). No schema required.
- **Search:** case-insensitive substring over body + attribute/resource values, with
  optional `--level`, time range, and `--limit`.

### HTTP API
| Method | Path | Notes |
|--------|------|-------|
| `POST` | `/ingest` | Body is NDJSON. Returns `{"ingested": N}`. |
| `GET`  | `/search?q=&level=&since=&until=&limit=` | Returns NDJSON, most recent first. |
| `GET`  | `/health` | Liveness check. |

## What's next (see roadmap)

Phase 2 replaces the in-memory `Vec` with columnar segments on object storage (S3/MinIO),
adds an OTLP receiver, and retention. Phase 3 brings the real query differentiator.

## Development

```bash
cargo test          # unit + end-to-end tests
cargo fmt --all     # format
cargo clippy --all-targets -- -D warnings
```
