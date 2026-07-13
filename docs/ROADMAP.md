# alien_log — Log Aggregation Tool: Roadmap & Competitive Analysis

> Living planning doc. Step 1 of our process: analyze the landscape, find the pain,
> pick a wedge, and break the work down as finely as possible.
> Status legend: `[ ]` todo · `[~]` in progress · `[x]` done

---

## 1. Competitive Landscape (2026)

We split the market into three tiers. The point is not to copy any of them — it's
to see where users are unhappy enough to switch.

### Commercial SaaS (expensive, powerful, sticky)

| Tool | Strength | Where it hurts |
|------|----------|----------------|
| **Splunk** | Best-in-class SPL correlation, enterprise security/SIEM, massive scale | Extremely expensive; cost scales aggressively with volume; heavyweight to operate |
| **Datadog Logs** | Great correlated incident investigation if already on Datadog | Bills twice (ingest **and** index); "high-water mark" host billing; surprise bills are a meme |
| **New Relic** | All-in-one observability | Per-user pricing punishes big teams; reports of agent-generated log volume inflating bills |
| **Sumo Logic** | Mature, cloud-native | Cost + query experience complaints; dated UX |

### Open-source / self-hosted (cheap to run, more DIY)

| Tool | Design | Where it hurts |
|------|--------|----------------|
| **Grafana Loki** | Indexes only labels, not full text → very cheap storage | Bad label design tanks search; broad full-text queries over long ranges time out; cardinality is a footgun |
| **Elastic / ELK / OpenSearch** | Full-text inverted index, rich pipelines | Heavy resource footprint; licensing churn; operationally complex |
| **Graylog** | Built-in SIEM, RBAC, compliance | Security-focused; heavier setup |
| **OpenObserve** | Rust, object-storage backed, ~140x cheaper storage vs ES, unified logs/metrics/traces, SQL | Younger; unified scope = bigger surface |
| **Quickwit** | Rust, inverted index on object storage, Kafka-native, real-time | Fewer integrations; young; some features immature |
| **Parseable / SigNoz / Uptrace** | Cost-efficient OSS observability | Varying maturity, smaller ecosystems |

### Collectors / plumbing (not competitors, likely dependencies)

Fluentd, Fluent Bit, Logstash, Vector, OpenTelemetry Collector. **We should ingest
from these, not replace them.** OpenTelemetry (OTLP) is the emerging standard — being
OTLP-native day one is table stakes.

---

## 2. Pain Points → Opportunity

Recurring complaints across reviews (Reddit/G2) and the comparison blogs:

1. **Unpredictable cost.** Usage-based billing with ingest+index double-charging and
   "high-water mark" traps. Teams live in fear of the bill.
   → **Opportunity:** predictable, volume-decoupled cost. Object storage (S3/GCS/MinIO)
   backed so retention is cheap and *flat*.

2. **Cardinality is a footgun (Loki).** Get labels wrong and search dies.
   → **Opportunity:** don't make users pre-design the index. Schema-on-read / full-text
   over columnar object storage so "just search" works without label archaeology.

3. **Too many query languages.** LogQL + PromQL + TraceQL + SPL = cognitive overhead
   mid-incident.
   → **Opportunity:** **one** query surface. A simple, grep-like search that covers 90%
   of incident needs, with SQL as the power-user escape hatch.

4. **Operational burden of self-hosting.** ELK/Loki production setups need clusters,
   object storage, tuning.
   → **Opportunity:** **single binary**, zero-dependency local mode; scale-out optional.
   `./alien_log` and you're searching logs in 30 seconds.

5. **Poor developer/local DX.** Most tools are ops-team platforms, not dev tools. Local
   debugging still means `grep`/`tail`/`jq` on raw files.
   → **Opportunity:** be delightful for a *developer* first (live tail, structured +
   unstructured, instant search), then grow into the team/ops platform.

### Our wedge (recommended positioning)

> **A developer-first, single-binary log aggregator that's cheap and predictable to run,
> speaks OTLP natively, stores on object storage, and gives you *one* dead-simple search
> that never makes you think about cardinality.**

Start where the incumbents are weakest — **local/dev DX + cost predictability + zero-ops
single binary** — and expand toward team/scale features. This is deliberately narrower
than "unified observability platform" so we can actually ship.

> ⚠️ **Positioning is the one decision that reshapes everything below.** The roadmap is
> written for the wedge above; if we instead target ops-at-scale or SaaS-first, phases
> reorder. Confirm before Phase 1.

---

## 3. Architecture Sketch (MVP-oriented)

```
 sources                 ingest              store                query/serve
┌─────────────┐        ┌──────────┐       ┌────────────┐        ┌──────────────┐
│ OTLP/HTTP   │        │ receiver │       │ WAL (local)│        │ search API   │
│ Fluent/Vec  │──────▶ │  + parse │─────▶ │ + columnar │◀────── │ (grep + SQL) │
│ stdin/files │        │  + batch │       │ on object  │        │ live tail    │
│ syslog      │        │          │       │ storage    │        │ web UI / TUI │
└─────────────┘        └──────────┘       └────────────┘        └──────────────┘
```

Key bets:
- **Object storage as the source of truth** (local FS in dev, S3/GCS/MinIO in prod).
- **Columnar segments** (Parquet-like) for cheap scan + compression.
- **Schema-on-read**: ingest anything (JSON, logfmt, plain text); structure discovered
  at query time. No mandatory label design.
- **Single binary, embedded everything** for MVP; pull apart into ingest/query/compactor
  services only when scale demands it.

Open tech decisions (see §6): implementation language, storage/query engine
(build vs. embed DataFusion/DuckDB/Tantivy), UI (web vs. TUI first).

---

## 4. Roadmap — Phases

### Phase 0 — Foundations & decisions *(this step)*
Analysis, positioning, tech choices, project skeleton. Ship this doc.

### Phase 1 — Walking skeleton (ingest → store → search)
End-to-end thinnest slice: accept a log line, persist it, search it back. No scale, no
UI polish. Proves the pipeline.

### Phase 2 — Real ingestion & storage
OTLP receiver, batching, columnar segments, object-storage backend, retention.

### Phase 3 — Query experience
The differentiator: one grep-like search that's fast and cardinality-proof, SQL escape
hatch, live tail.

### Phase 4 — Interfaces
CLI/TUI for developers, then a minimal web UI. Live tail in both.

### Phase 5 — Ops & scale
Auth, multi-tenant, horizontal scale-out, compaction, alerting hooks. Only what the
wedge demands.

### Phase 6 — Adoption
Docs, quickstart, integrations (Fluent Bit/Vector/OTel Collector), benchmarks vs. Loki.

---

## 5. Task Breakdown (as granular as possible)

> Each `[ ]` is meant to be a single, shippable PR-sized unit.

### Phase 0 — Foundations
- [x] P0.1 Competitive analysis (this doc §1)
- [x] P0.2 Pain-point → opportunity mapping (§2)
- [x] P0.3 Draft positioning / wedge (§2)
- [ ] P0.4 Confirm positioning with stakeholder (blocking gate)
- [ ] P0.5 Choose implementation language (candidates: Rust, Go)
- [ ] P0.6 Choose storage/query engine strategy (build vs embed: DataFusion / DuckDB / Tantivy / Parquet)
- [ ] P0.7 Decide first interface (TUI vs web vs plain HTTP+curl)
- [ ] P0.8 Repo skeleton: build tooling, lint, format, CI, test harness
- [ ] P0.9 Define the internal log record model (fields: ts, level, body, attributes, resource, trace ids)
- [ ] P0.10 Write ADRs for P0.5–P0.7 decisions

### Phase 1 — Walking skeleton
- [ ] P1.1 Define ingest wire format for MVP (line-delimited JSON over HTTP POST)
- [ ] P1.2 HTTP ingest endpoint that accepts and parses a batch
- [ ] P1.3 In-memory store + append to local WAL/file
- [ ] P1.4 Basic search endpoint: substring/keyword over a time range
- [ ] P1.5 `alien_log` CLI: `send` and `search` subcommands
- [ ] P1.6 End-to-end test: send N lines, search, assert hits
- [ ] P1.7 Quickstart: "index your first logs in 30s" in README

### Phase 2 — Ingestion & storage
- [ ] P2.1 OTLP/HTTP logs receiver (protobuf/JSON)
- [ ] P2.2 Parsers: JSON, logfmt, plaintext fallback (schema-on-read)
- [ ] P2.3 Batching + backpressure on ingest
- [ ] P2.4 Columnar segment format (Parquet or equivalent) + flush policy
- [ ] P2.5 Object-storage backend abstraction (local FS first)
- [ ] P2.6 S3-compatible backend (works with MinIO in tests)
- [ ] P2.7 Segment index/manifest (time ranges, min/max, stats for pruning)
- [ ] P2.8 Retention / TTL enforcement
- [ ] P2.9 Ingest throughput + durability tests

### Phase 3 — Query experience
- [ ] P3.1 Query planner: prune segments by time + manifest stats
- [ ] P3.2 Full-text/substring scan over columnar segments
- [ ] P3.3 Simple query language grammar (grep-like: `level=error "timeout" service=api`)
- [ ] P3.4 SQL escape hatch (embed DataFusion/DuckDB over segments)
- [ ] P3.5 Field extraction / schema-on-read at query time
- [ ] P3.6 Live tail (follow) via streaming endpoint
- [ ] P3.7 Query result pagination + sorting + limits
- [ ] P3.8 Cardinality-proof validation: broad query over long range stays bounded
- [ ] P3.9 Query benchmarks vs. baseline

### Phase 4 — Interfaces
- [ ] P4.1 CLI search UX: colorized, structured + raw views, `--follow`
- [ ] P4.2 TUI: live tail + search + filter (optional, decision-gated)
- [ ] P4.3 Minimal web UI: search box, results table, time picker
- [ ] P4.4 Live tail in web UI (SSE/websocket)
- [ ] P4.5 Saved searches / shareable query URLs

### Phase 5 — Ops & scale
- [ ] P5.1 AuthN/AuthZ (API keys → OIDC later)
- [ ] P5.2 Multi-tenant / namespace isolation
- [ ] P5.3 Split ingest / query / compactor into services (behind a flag)
- [ ] P5.4 Background compaction of small segments
- [ ] P5.5 Horizontal scale-out of query
- [ ] P5.6 Alerting hooks (query → webhook/Slack)
- [ ] P5.7 Metrics/self-observability (expose own OTel)

### Phase 6 — Adoption
- [ ] P6.1 Integration docs: Fluent Bit, Vector, OTel Collector → alien_log
- [ ] P6.2 Deployment guides: single binary, Docker, k8s
- [ ] P6.3 Benchmark writeup: cost + query vs. Loki/ELK
- [ ] P6.4 Public quickstart site / landing
- [ ] P6.5 Example dashboards / recipes

---

## 6. Open Decisions (need answers before Phase 1)

1. **Positioning** — dev-first zero-ops wedge (recommended) vs ops-at-scale vs SaaS-first?
2. **Language** — Rust (perf, single binary, matches OpenObserve/Quickwit) vs Go (velocity, ecosystem)?
3. **Query/storage engine** — build minimal ourselves vs embed DataFusion/DuckDB (SQL) + Tantivy (full-text)?
4. **First interface** — plain HTTP+CLI (fastest), TUI, or web UI?
5. **Scope guardrail** — logs only for v1 (recommended), or logs+metrics+traces from the start?

---

## 7. Next Action

Resolve §6.1 (positioning) and §6.2 (language), then execute **Phase 1** one task at a
time. Everything below P1.1 is intentionally small enough to be a single PR.
