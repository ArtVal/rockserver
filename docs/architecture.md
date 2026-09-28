# Architecture outline

## Scope and boundaries

RockServer owns remote station discovery: accepting a natural-language query, interpreting it, searching a catalog, ranking candidates, and returning stream metadata. It does not own the Windows UI or audio playback. Those remain in RockCast, which also retains a local catalog fallback.

## Search request flow

1. RockCast sends a request to the versioned search endpoint.
2. The HTTP layer validates the DTO and assigns a request ID.
3. `QueryParser` receives only validated request text and locale and returns provider-neutral terms, tags, language, and country intent. `LlmQueryParser` receives its JSON through provider-neutral `LlmProvider`; `YandexLlmProvider` is selected only with both Yandex environment settings. The deterministic parser is the default and failure fallback; no parser or LLM provider receives catalog rows.
4. The optional `EmbeddingProvider` receives only query text. The current concrete provider is an explicitly development-only deterministic fixture selected by environment; production query parsing is available through the separate Yandex LLM adapter, while production embeddings remain future work.
5. The search domain owns hard language/country filters, exclusions, the fixed hybrid formula, stable ordering, and limit.
6. `StationRepository` executes those rules. The in-memory backend ignores embeddings and uses the shared metadata ranker. PostgreSQL applies hard filters and exclusions first, optionally joins only model/version/dimension-compatible station embeddings, computes exact cosine hybrid scores, orders score descending then station ID ascending, and applies limit last.
7. Provider failure or absent/incompatible embeddings degrade to metadata ranking without changing the HTTP result shape. Repository failure still maps to the existing 500 behavior.
8. The HTTP layer maps normalized intent and ranked domain results to the contract DTO.
9. RockCast selects a result and passes its stream URL to the existing playback controller after the next integration stage.

Provider failures degrade predictably today. Query parsing and embeddings live behind traits and tests use deterministic fakes. The request path never crawls Radio Browser, writes station embeddings, or probes streams; importing, embedding backfill, and future health checking are controlled background concerns.

## Current catalog import flow

Radio Browser import is a separate one-shot process, not part of the HTTP router, search service, readiness probe, or server startup:

1. `import_radio_browser` validates that `DATABASE_URL` is present and loads bounded Radio Browser configuration.
2. `PostgresImportStore` connects and applies the same embedded versioned migrations used by the HTTP PostgreSQL backend.
3. `CatalogImporter` creates a durable `started` run through the import-only persistence trait.
4. `RadioBrowserClient`, behind the import-provider trait, requests bounded pages from `/json/stations/search` with `hidebroken=true`, stable name ordering, absolute offset, explicit limit, timeout, response-size cap, and descriptive User-Agent.
5. The provider maps each upstream DTO into the provider-neutral `ImportedStation` model or increments the skip count. Mapping validates provider UUID, last upstream check, name, and resolved stream URL; it deterministically normalizes optional homepage, tags, language code, country code, codec, and bitrate. Language selection scans all valid codes, prefers the first two-letter code, and falls back to the first three-letter code only when no two-letter code exists.
6. Before persistence, `CatalogImporter` requires every record source to equal the provider source that owns the run. A mismatch rejects the whole normalized page, increments failed counts, records a sanitized terminal failure, and performs no upsert.
7. `PostgresImportStore` transactionally upserts each page by `(source, source_station_id)` and the matching stream identity. It verifies that the explicit source belongs to the still-started run and uses that argument for writes instead of trusting record source fields. Only Radio Browser-owned rows are updated; built-in rows use a separate `builtin` ownership namespace.
8. A short upstream page or the configured maximum page count ends a successful run. Provider or persistence failure records a terminal failed run with partial counts and a sanitized summary. Missing upstream rows are retained; RS-006 performs no deletion or disable sweep.

The provider does not depend on `StationRepository`, and HTTP search does not depend on the provider client. PostgreSQL search sees imported rows through the existing catalog schema after a successful transaction, without introducing network I/O into the request path.

## Current HTTP service

The crate is split into a reusable library and a thin process entry point. The library builds the Axum router, owns configuration and telemetry setup, and exposes the serving boundary so application behavior can be tested without starting a process. The binary loads configuration, binds the listener, and supplies the Ctrl+C shutdown signal.

The operational routes remain outside the versioned public API:

- `GET /health/live` reports that the process is running.
- `GET /health/ready` returns 200/`ok` for the in-memory backend and checks `SELECT 1` for PostgreSQL, returning 503/`not_ready` if the database is unavailable.

Liveness always returns the stable JSON model `{"status":"ok"}` and never depends on PostgreSQL. Request spans are produced by the HTTP tracing middleware and application events use structured JSON tracing. The listener defaults to `127.0.0.1:3000` and can be overridden with `ROCKSERVER_BIND_ADDR`.

## Search and voice-command APIs

`api/openapi.yaml` is the source of truth for `POST /api/v1/search` and the Windows voice-command boundary. `POST /api/v1/search` keeps its established schema and behavior. `POST /api/v1/voice/command` is the canonical route for a client-supplied, already-recognized `transcript`; `POST /api/v1/voice/command` is its deprecated compatibility alias. The voice route accepts the same locale, result-limit, and exclusion controls, then invokes the existing `SearchService` rather than a parallel search implementation. Its response returns the trimmed transcript, normalized query, ranked station list, and `selected_station`, which is the first result or `null` when no station matches.

The JSON voice boundary deliberately accepts no audio. The separate canonical WebSocket `GET /api/v1/voice/stream` begins with a validated `start` event, accepts PCM signed 16-bit little-endian mono binary chunks, emits provider transcript updates, and commits the final transcript into the same `SearchService`. `/api/v1/voice/stream` is a deprecated same-handler alias. Each chunk is limited to 65,536 bytes, a session to 10 MiB, provider operations to ten seconds, and final search to five seconds. Protocol and provider failures use terminal `error` events with the standard `code`, `message`, `request_id`, and `details` fields.

`StreamingSpeechRecognizer` and per-connection `SpeechStreamSession` isolate upstream transport and credentials from Axum DTOs. The default startup recognizer is deliberately unavailable until a production Yandex or OpenAI adapter is configured; tests inject a deterministic fake and never call an external network.

After STT yields final text, `CommandInterpreter` is the provider-neutral boundary for full player commands. `LlmCommandInterpreter` asks an `LlmProvider` for strict structured output and deserializes it directly into `VoiceCommand`/`Intent`/`RadioQuery`; local code only normalizes bounded values and validates intent-specific invariants. There is no second heuristic command parser. Search commands can subsequently be mapped into the established search path, while playback-control intents remain independent of catalog access.

The HTTP layer owns DTO deserialization, defaults, validation, request IDs, and `ErrorResponse` mapping. Query interpretation, embedding generation, ranking semantics, and persistence use separate boundaries. `LlmQueryParser` sends a fixed instruction and JSON-encoded command/locale to `LlmProvider`, requires an intent JSON Schema, and locally validates the bounded response; the Yandex implementation applies `Api-Key` authentication, status checks, response-size and timeout limits without logging its secret or raw upstream payload. The search domain owns the repository-neutral meaning of hard filtering, exclusions, deterministic scoring, score-descending/station-ID-ascending ordering, and final limit. `StationRepository` returns ranked domain models and exposes the readiness check. PostgreSQL executes those rules in SQL so it can filter, order, and limit before transferring rows; the in-memory fallback applies metadata rules in Rust. This avoids leaking SQL/vector types into the domain while keeping large-catalog work out of the HTTP and provider layers.

## Device-control playback projection

A paired RockCast publishes revisioned runtime playback state through the authenticated device WebSocket. RockServer validates an optional bounded `track_title`, stores the player state, and includes it in the owner-scoped REST/WebSocket directory projection. The title is display metadata only; `station_id` remains identity and command routing is unchanged. RockMobile resolves that ID through the exact station catalog endpoint, with its verified offline catalog as fallback, and renders the title only when present.

## Persistence

At startup, `DATABASE_URL` is required; the service connects through SQLx, applies embedded versioned migrations from `migrations/`, and selects `PostgresStationRepository`. The schema separates station metadata from playable stream URLs, enforces stable IDs, positive bitrates, valid health states, at most one primary stream per station, timestamps, and useful metadata/relationship indexes. A second idempotent migration seeds PostgreSQL with the same six development stations used by isolated tests. The RS-006 migration adds provider ownership to stations and streams, unique source identities for idempotent upsert, and `import_runs` with status, counts, timestamps, and safe error summaries. Existing seed rows are backfilled as `builtin`; Radio Browser rows use `radio_browser`.

The RS-007 migration enables pgvector and creates `station_embeddings`. Each row is owned by station plus model/version, records dimension and timestamps, and enforces that the unbounded `vector` value has the declared dimension and a non-zero norm. The unbounded column avoids locking schema to the deterministic fixture's dimension. Exact search joins only matching model/version/dimension provenance. No ANN index is created because pgvector indexes require a fixed-dimensional expression/operator class; model-specific indexing is deferred until a production model and catalog scale justify it.

The controlled `backfill_embeddings` binary requires PostgreSQL and explicit `ROCKSERVER_SEMANTIC_PROVIDER=deterministic-dev`. It pages station documents by stable ID, embeds one station at a time, and idempotently inserts or updates provenance-specific rows. It is not called during HTTP startup or `POST /api/v1/search`, and it performs no provider network I/O. Radio Browser ownership/import semantics remain unchanged.

The `account_cleanup` binary is a separate staging operator boundary. Its default command is a
read-only inventory of account, passkey, identity, device, native-session, refresh-token,
browser-session, pairing, WebAuthn-challenge, and audit rows with secret and personal-data fields
redacted. An apply command accepts one UUID and an exact action-specific confirmation phrase;
account deactivation and device/passkey revocation are transactional, auditable, and retain rows
for recovery review. It never merges accounts, infers test ownership from names/age, or removes
browser/Google Password Manager entries. It requires the non-secret
`ROCKSERVER_CLEANUP_ENV=staging` guard, and the deployment wrapper exposes it only through the
existing root-scoped operator command path.

The RM-012-A migration adds account-owned personal data: `favourite_records` and
`history_records` keyed by `(user_id, record_id)` with nullable station and deletion markers,
plus one shared monotonic revision sequence used as the per-account sync cursor. Pushes apply
last-writer-wins on `updated_at` inside the synchronize transaction; live collections stay
bounded at 500 rows each, history past 90 days expires at push time, tombstones are
garbage-collected after 90 days, and rows of accounts deleted more than 30 days ago are purged
by the retention sweep. `POST /api/v1/sync` authenticates with the native device-session Bearer
and returns the account delta plus echoes of pushed records, so a rejected stale push never
loops.

When `DATABASE_URL` is absent, startup selects `InMemoryStationRepository`; both choices are logged by backend name without logging a DSN or password. SQL uses runtime-checked queries rather than compile-time query macros, so normal builds do not require a live database. `compose.yaml` uses a pgvector-capable PostgreSQL 17 image with development-only defaults and a healthcheck. Import and embedding backfill always require `DATABASE_URL` and never fall back to memory.

Hybrid scoring normalizes cosine similarity with `1 - cosine_distance / 2`, clamps it to `[0,1]`, and uses `0.70 * metadata_score + 0.30 * semantic_score` for compatible pairs. A station missing a compatible embedding keeps its unscaled metadata score. Without a valid query embedding, the existing metadata-only inclusion and score remain unchanged. Hard filters and exclusions are in the candidate CTE, before scoring and final limit; station ID ascending is the final deterministic tie-break.

The Radio Browser field selection follows the official [API usage guidance](https://docs.radio-browser.info/#using-the-api), [Station structure](https://docs.radio-browser.info/#station), [advanced search parameters](https://docs.radio-browser.info/#advanced-station-search), and [mirror discovery guidance](https://docs.radio-browser.info/#server-mirrors). RS-006 consumes `stationuuid`, `name`, `url_resolved`, `homepage`, `tags`, `languagecodes`, `countrycode`, `codec`, `bitrate`, and `lastcheckok`. The canonical UUID is the provider identity and resolved URL is the only accepted stream field. The official implementation is maintained in the [Radio Browser API repository](https://gitlab.com/radiobrowser/radiobrowser-api-rust).

The standalone [service diagrams](service-diagrams.html) summarize the current, next, and future architecture in Russian and work locally without a server or network access.

## Planned modules

- **API:** Axum health and search routing, DTO validation, error mapping, and request IDs are present.
- **Domain:** normalized queries, filters, station candidates, scoring, and stable ranking are present.
- **Persistence:** PostgreSQL migrations, development seed, SQL search repository, pgvector embeddings with provenance, controlled embedding store, provider ownership, import-run history, import-only upsert store, startup backend selection, and in-memory fallback are present.
- **Providers:** bounded Radio Browser import, replaceable query-parser/LLM/embedding traits, deterministic parser fallback, Yandex AI Studio structured intent parsing, and an explicit development-only deterministic embedder are present outside inappropriate catalog boundaries. SpeechKit STT remains a separate future adapter.
- **Operations:** tracing, import and embedding backfill logs, process liveness, repository-aware readiness, configurable binding, pgvector-capable local Compose, Ctrl+C shutdown, and the preview-first staging account-cleanup binary are present; metrics, broader shutdown triggers, and stream probing remain planned.

Dependencies should point inward toward domain concepts. HTTP, database, and provider-specific types must not leak into the core ranking model.
