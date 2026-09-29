# Project status

Last updated: 2026-09-29

## DOCS: agent instruction guidelines for modularity, file size limits, and test decomposition (implemented locally, 2026-09-29)

Formalized architectural and test organization guidelines across agent guidance files (`AGENTS.md`, `docs/codex-project-context.md`, `src/ARCHITECTURE.md`) to prevent monolithic files and ensure clean separation of concerns upfront:
- Mandated modularity and bounded file sizes: target under 400 lines (hard ceiling ~500 lines); prohibited accumulating domain, storage, transport, and tests in single files.
- Codified immediate layer separation: pure domain models (`domain.rs`), traits, stateful orchestration (`service.rs`/`router.rs`), transport protocols/DTOs (`protocol.rs`/`routes.rs`/`builder.rs`), persistence/fakes (`in_memory.rs`/`fake.rs`/`persistence/`), and thin re-export facades (`mod.rs` or `<name>.rs`).
- Codified test suite organization: unit tests isolated in sibling `tests.rs` submodules; integration tests decomposed thematically under `tests/<suite>/` with shared helpers in `common.rs` and single test binary targets when serialization is required.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` all green.
Next step: Follow the new modularity rules for all upcoming features and fixes.

## ARCH: thematic modular decomposition of integration test suites (tests/openapi_contract and tests/postgres_integration) (implemented locally, 2026-09-29)

The two large integration test monoliths, `tests/openapi_contract.rs` (1783 lines) and `tests/postgres_integration.rs` (2412 lines), were cleanly decomposed into thematic submodules under `tests/openapi_contract/` and `tests/postgres_integration/`, reducing the top-level test files to concise facades and improving navigation and compilation:
- `tests/openapi_contract/`:
  - `common.rs` (325 lines): fixture specifications, schema validation helpers, golden JSON loaders, and OpenAPI document constants.
  - `surface.rs` (302 lines): public surface validation and search endpoint registration tests.
  - `device_control.rs` (466 lines): device control v1 specification bounds, schema linking, and golden fixture validation.
  - `device_catalog.rs` (425 lines): stream-free catalog bounds, play stream constraints, and native session gating.
  - `voice_stream.rs` (143 lines): voice stream specification alignment with runtime and device flows.
  - `personal_sync.rs` (158 lines): personal data sync specification constraints and session checks.
  - `tests/openapi_contract.rs` (14 lines): minimal facade declaring thematic submodules.
- `tests/postgres_integration/`:
  - `common.rs` (98 lines): shared test database inspection pools (`repository_pool`), import mock providers (`OnePageProvider`, `imported_station`), run assertions (`assert_run`), and result helpers (`station_ids`).
  - `device_control.rs` (310 lines): device control state persistence and ownership isolation.
  - `admin.rs` (323 lines): administrator identity, Argon2id credentials, atomic bootstrap, and durable session revocation.
  - `account.rs` (477 lines): passkey registration, WebAuthn challenge handling, desktop pairing, and session rotation.
  - `account_cleanup.rs` (210 lines): preview inspection, operator cascade deletion, and audit logging.
  - `yandex_home.rs` (72 lines): OAuth state lifecycle and encrypted token persistence.
  - `migrations.rs` (416 lines): database migrations, pgvector extension verification, semantic search, and live readiness probes.
  - `shared_catalog.rs` (160 lines): transactional release activation, tombstones, and rollback safety.
  - `personal_data.rs` (267 lines): last-writer-wins synchronization, cursor delta scoping, retention sweeps, and account purge.
  - `tests/postgres_integration.rs` (31 lines): minimal facade maintaining a single test target with serial execution guarantees (`--test-threads=1`).

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all 9 OpenAPI contract tests pass; all 12 PostgreSQL integration tests discovered and compiled cleanly; 184 library tests pass).
Next step: Review remaining large modules or test files.

## ARCH: domain models, repository implementations, and search service decomposition for search (implemented locally, 2026-09-29)

The monolithic root file `src/search/mod.rs` (849 lines) was decomposed into focused architectural submodules under `src/search/`, reducing the root to a clean 37-line facade:
- `src/search/domain.rs` (211 lines): defines domain models (`SearchQuery`, `SearchConstraints`, `Station`, `StationHealth`, `RankedStation`, `SearchOutcome`), relevance thresholds (`MIN_RELEVANCE_SCORE`), the `StationRepository` abstraction trait, and `RepositoryError`.
- `src/search/in_memory.rs` (298 lines): implements in-memory catalog storage (`InMemoryStationRepository`), release preflight integration (`from_pinned_catalog`), pagination/filtering, `UnavailableStationRepository`, and deterministic test fixture constructors.
- `src/search/service.rs` (343 lines): orchestrates the multi-stage search pipeline (`SearchService`), concurrent parsing and local embedding generation, fallback intent preservation, confidence-gated language filtering, progressive genre hierarchy broadening (`broaden_tags`), and catalog readiness checks.
- `src/search/mod.rs` (37 lines): clean facade re-exporting domain types, in-memory repositories, and search orchestration with zero breaking changes for internal or external callers.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all 49 search unit tests and 184 library tests pass).
Next step: Review remaining large modules (e.g. `src/search/query.rs` or `src/http/auth.rs`).

## ARCH: catalog resolution, target validation, and command router decomposition for device_control_command (implemented locally, 2026-09-29)

The monolithic `src/device_control_command.rs` (817 lines) was decomposed into focused architectural submodules under `src/device_control_command/`, leaving a 13-line facade:
- `src/device_control_command/catalog.rs` (91 lines): defines `StationCatalog`, `ResolvedStation`, `ResolutionFailure`, `resolve_station`, and the default catalog integration for `crate::search::SearchService`.
- `src/device_control_command/target_validation.rs` (136 lines): defines target manifest, capability, and role preconditions (`validate_target`, `require_role_capability`, `display_supported`).
- `src/device_control_command/router.rs` (592 lines): stateful in-flight command router (`CommandRouter`), command admission, reservation, duplicate/stale detection, timeout expiry, delivery over connection registry, and terminal result recording.
- `src/device_control_command/tests.rs` (933 lines): focused regression and lifecycle unit tests covering target capabilities, catalog resolution, error codes, and idempotent delivery.
- `src/device_control_command.rs` (13 lines): clean, minimal facade re-exporting all catalog, validation, and router types without breaking changes for external callers.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all 6 device_control_command unit tests and 184 library tests pass).
Next step: Address remaining modules from the codebase refactoring plan (e.g., `src/search/mod.rs` or `src/http/handlers.rs`).

## ARCH: transport and test isolation for http/control (implemented locally, 2026-09-29)

The monolithic `src/http/control.rs` (1088 lines) was reduced to 442 lines by isolating unit/transport tests, wire error framing, and state admission decisions:
- `src/http/control/tests.rs` (601 lines): extracted isolated unit and transport tests covering controller-only snapshot exemptions, stale reconnect handshakes, fake resolver lookup, heartbeat and TTL disconnect lifecycle, forward revision overwrite, malformed binary/oversized frames, and graceful server shutdown.
- `src/http/control/protocol.rs` (355 lines): added wire scope naming (`scope_name`) and command rejection envelope formatting (`send_command_error`).
- `src/http/control/state.rs` (148 lines): added controller role observation checks (`requires_full_state`) and full snapshot persistence outcome acceptance (`accepted_full_snapshot`).
- `src/http/control.rs` (442 lines): focused WebSocket upgrade ingress (`connect`) and main message loop (`run`).

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (14/14 control unit tests and all integration suites pass).
Next step: Review remaining candidates from roadmap.

## ARCH: domain store and test isolation for personal_data and admin (implemented locally, 2026-09-29)

The monolithic domain modules `src/personal_data.rs` (1281 lines) and `src/admin.rs` (885 lines) were refactored by isolating domain models, persistence traits, in-memory/fake store implementations, and unit test suites:
- `src/personal_data/domain.rs` (414 lines): defines pure sync records (`PersonalSyncBatch`, `FavouriteRecord`, `HistoryRecord`, `TombstoneRecord`), validation limits (`CollectionLimits`), timestamp handling, error types (`PersonalDataError`), and the storage trait (`PersonalDataStore`).
- `src/personal_data/in_memory.rs` (352 lines): deterministic in-memory store (`InMemoryPersonalDataStore`), revision allocation, last-writer-wins resolution (`delta_and_echo`), and retention window rules.
- `src/personal_data/tests.rs` (535 lines): isolated unit tests covering validation, account isolation, delta cursor advancement, loser echoes, retention expiry, and resurrection semantics.
- `src/personal_data.rs` (26 lines): clean facade re-exporting domain and in-memory types with zero breaking changes for callers.
- `src/admin/domain.rs` (394 lines): administrator lifecycle status (`AdminPrincipalStatus`), principal (`AdminPrincipal`), Argon2id password hash (`AdminPasswordHash`), session models (`AdminSession`), login/audit records, and persistence contract (`AdminStore`).
- `src/admin/fake.rs` (273 lines): deterministic in-memory test store (`FakeAdminStore`), security event and login attempt logs, and session rotation logic.
- `src/admin/tests.rs` (152 lines): isolated unit tests for password hash redaction, Argon2id validation, and fake store session lifecycle.
- `src/admin.rs` (14 lines): clean facade re-exporting domain and fake store contracts.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all personal_data, admin, and admin_bootstrap tests pass cleanly).
Next step: Step 5 of the refactoring roadmap (`src/http/control.rs` WebSocket framing and admission split).

## ARCH: modular layering refactoring for http/endpoints (implemented locally, 2026-09-29)

The monolithic `src/http/endpoints.rs` (1083 lines) was refactored by introducing a fluent `RouterBuilder`, isolating the Axum routing tree, and extracting unit tests:
- `src/http/endpoints/builder.rs` (298 lines): implements fluent `RouterBuilder` providing default offline test fixtures and chainable dependency injectors (`with_repository`, `with_search_service`, `with_speech_recognizer`, `with_speech_recognizers`, `with_voice_command_interpreter`, `with_voice_command_timeout`, `with_api_bearer_token`, `with_postgres_account_stores`, `with_admin_store`, `with_postgres_admin_store`, `with_trusted_proxy_token`, `with_local_admin_origin`, `with_control_registry`, `with_control_commands`, `with_control_state_hub`, `with_control_store`, `with_control_session_resolver`, `with_personal_store`, `with_icon_import`, `with_yandex_home`).
- `src/http/endpoints/routes.rs` (189 lines): declarative Axum route assembly (`build_router`) mapping public and administrative route handlers and attaching tracing layers.
- `src/http/endpoints/tests.rs` (374 lines): isolated unit tests covering health endpoints, route security headers, admin session authorization, login, refresh, pairing fail-closed behavior, and proxy verification.
- `src/http/endpoints.rs` (358 lines): clean facade orchestrating module declarations and convenience constructors (`router()`, `router_with_*`), each simplified to a 1–5 line builder chain.
- Re-exported `RouterBuilder` from `rockserver::http` for ergonomic caller configuration, and updated `control.rs` tests to use the builder.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (184 unit/doc tests and 9 integration test suites passed).
Next step: Step 4 of the refactoring roadmap (`src/device_control.rs` models and domain decomposition).

## ARCH: modular layering refactoring for http/voice (implemented locally, 2026-09-29)

The monolithic `src/http/voice.rs` (1353 lines) was decomposed into focused transport and device orchestration submodules under `src/http/voice/`:
- `src/http/voice/protocol.rs` (357 lines): wire DTOs (`VoiceStreamStartDto`, `VoiceCommandRequestDto`, etc.), JSON server events (`VoiceStreamServerEvent`), 24 typed error codes (`VoiceStreamErrorCode`), streaming timeouts, rate limits, `VoiceSlot`, and WebSocket frame serialization helpers.
- `src/http/voice/device.rs` (421 lines): device voice capability checks (`validate_device_voice_start`), intent planning via `device_control_intent`, voice-to-device intent translation (`device_user_intent`), command submission via `CommandRouter`, and command lifecycle polling over `control_store`.
- `src/http/voice.rs` (556 lines): focused transport handlers (`voice_stream`, `voice_command`), WebSocket audio ingestion loop (`run_voice_stream`), speech recognizer session lifecycle, and catalog search completion (`finish_stream_search`).

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (including all `voice_stream_api` and `voice_command_api` tests).
Next step: Step 3 of the refactoring roadmap (`src/http/endpoints.rs` builder pattern and test extraction).

## ARCH: modular layering refactoring for station_icons (implemented locally, 2026-09-29)

The monolithic `src/station_icons.rs` (1310 lines) was refactored into focused, single-responsibility architectural layers under `src/station_icons/`:
- `src/station_icons/domain.rs` (211 lines): domain models (`PreparedIcon`, `ReadyIcon`, `IconJobProgress`), value types (`IconStorageKey`), error types, WebP raster preparation algorithm (`prepare_icon`), and abstract traits (`IconStorage`, `IconSourceFetcher`).
- `src/station_icons/storage.rs` (70 lines): atomic filesystem storage implementation (`FilesystemIconStorage`) using atomic tempfile write-and-rename mechanics.
- `src/station_icons/fetcher.rs` (310 lines): bounded external HTTP icon fetching (`SafeIconFetcher`) with SSRF protection (`validate_public_url`), redirect destination validation, and HTML `<link rel="...icon...">` / `/favicon.ico` discovery.
- `src/station_icons/coordinator.rs` (325 lines): PostgreSQL import job coordination (`IconImportCoordinator`), item plan resolution (`resolve_item_plan`), SQL queries, and manual admin icon override workflows.
- `src/station_icons/tests.rs` (326 lines): isolated unit tests covering filesystem storage, key validation, WebP normalization, HTML favicon candidate discovery, and item planning.
- `src/station_icons.rs` (27 lines): clean root facade preserving 100% backward compatibility for existing callers (`http/endpoints.rs`, `http/state.rs`, `http/station_icons.rs`, `lib.rs`).

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (15/15 station_icons unit tests passed).
Next step: Step 2 of the refactoring roadmap (cleaning up `src/http/voice.rs` transport and device intent orchestration).

## RM-012-A: server-side personal-data sync (implemented locally, 2026-09-28)

RockServer now stores account-owned favourites and playback history and exposes one
authenticated push+pull round trip, `POST /api/v1/sync` (OpenAPI 0.6.0), behind the native
device-session Bearer. Changes merge last-writer-wins per client `record_id` on `updated_at`
(equal instants keep the stored row); a shared monotonic revision sequence gives each device a
per-account delta cursor, and tombstones carry deletions to devices that were offline when the
deletion happened. `since_revision` 0/absent returns the full account snapshot; responses echo
pushed losers, so a stale push converges instead of looping. Server bounds mirror the RM-007-A
client contract: 500 live favourites, 500 live history entries, 300 batch items per collection,
4 KiB metadata, 256 KiB body budget, 90-day history retention (enforced at push time and by an
hourly per-process fleet sweep), 90-day tombstone GC, and personal-row purge for accounts
deleted more than 30 days ago. Station IDs stay opaque; unresolved references are preserved,
never rejected. Cursors are per device. Known v1 limitations: two devices recording the same
listening session produce two history rows (five-minute coalescing remains a client rule), and
the browser console has no personal-data view.
New code: `src/personal_data.rs` (domain + in-memory store fake), 
`src/persistence/personal_data_postgres.rs` (migration `0025_add_personal_data_sync.sql`),
`src/http/sync.rs`; the native-endpoint auth/throttle helper was extracted into
`src/http/control_auth.rs` and is now shared with the device catalog routes.
Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
and `cargo test` all green. The opt-in
`postgres_personal_data_sync_is_last_writer_wins_and_cursor_scoped` test passed against a
disposable PostgreSQL 16 (pgvector) container covering LWW, per-device cursors, tombstone
propagation, push-time and sweep retention, and deleted-account purge. Five other opt-in
PostgreSQL tests (device-control manifest, admin bootstrap/identity, account cleanup, session
rotation) had been failing since before this change on clean `master`; they are repaired in
the follow-up test commit and the full opt-in suite now passes 12/12 when run serially
(`--test-threads=1`, documented in the test file header — the tests share one database and
truncate its admin tables, so parallel runs race by construction).
Deployed as commit `2d8e27f` on 2026-09-28 (the release also carries the integration-test
repair and the Windows tar deploy fix); remote readiness passed, live `GET /health/ready`
returned 200, and `POST /api/v1/sync` answered the contract 401 `authentication_required`
without a credential, confirming the route is served and auth-gated. Migration 0025 applied
at container startup. Next step: client integration in the RockCast and RockMobile repositories
(RM-012-B).

## Opt-in PostgreSQL integration test rot repaired (2026-09-28)

Five opt-in tests had drifted from deliberate production behavior or were never runnable
in isolation; all fixes are test-side, no production code changed:

- Device-control full state snapshots with a forward revision gap now overwrite the
  projection (`Accepted`) instead of answering `Resync` — the integration test still expected
  the pre-RS semantics that the `full_state_outcome` unit test already documents.
- Admin bootstrap relied on a refresh audit record another test happened to leave behind;
  it now records its own request through the store and asserts the audit projection,
  removing the cross-test coupling.
- Admin identity foundation created a principal without truncating first and died on the
  migration-0019 singleton (`admin_principals ((true))`); it now establishes the same clean
  slate as its sibling admin tests.
- Account session rotation asserted the caller's `created_at: "unused"` placeholder came
  back from the store; `create_device` stamps creation server-side, so the test now checks
  the projection fields and that `created_at` is the server-stamped RFC 3339 instant.
- Account cleanup bound `[45; 32]` (an `integer[]`) to a `bytea` column — `[45u8; 32]` — and
  expected deactivation counters to include a device and session the test itself had already
  revoked mid-flight; the counters report rows flipped by the deactivation transaction, and
  the raw state check below still proves the cascade leaves nothing active.

Verification: fresh disposable pgvector container, `cargo test --test postgres_integration --
--ignored --test-threads=1` → 12 passed, 0 failed; the standard offline ritual
(fmt/clippy/test) also green. Parallel execution of this suite remains unsupported by design.

## Device playback track metadata (2026-09-26)

`PlaybackRuntimeState` accepts an optional bounded `track_title` from a player and
preserves it in the owner-scoped directory `runtime_state` projection. The
station ID remains the playback identity; the title is display metadata only.
RockCast publishes its observed stream title when available, and RockMobile
reads the field for its remote player. Verified with `cargo fmt --check`, strict
Clippy and `cargo test` (database and live service integration tests remain
ignored without their external prerequisites). Deployed as commit `a3f2a13` on 2026-09-26; readiness passed. A connected RockMobile switched the running RockCast between two stations and displayed their distinct titles. Spoken microphone input and the full physical control matrix remain unverified after this release.

## RS-ICON-012 follow-up: homepage prefix inspection (deployed 2026-09-25)

Production sampling of the remaining permanent errors showed ~17% were
homepages heavier than the 1 MiB bound whose declared icon links sit in the
document head; the bounded fetch discarded collected bytes on oversize. The
homepage inspection now truncates to the prefix and parses it instead of
failing. The operator also requested a from-scratch import on the new host,
so the station_icons metadata and job items are cleared before the fresh
run, and 3 GiB of swap was added to the memory-tight VPS. Verification:
`cargo fmt --check`, strict Clippy, `cargo test` (227 passed).
## DOCS-002: README and active backlog refresh (2026-09-24)

Updated the repository front page to describe the implemented service, production deployment path, API areas, repository boundaries, and current search limitation. Replaced the original early-stage checklist in TODO.md with the open search P0s, the RockMobile/RockCast playback-state handoff, voice reliability work, and dependent device-control milestones. The backlog distinguishes the implemented Yandex Smart Home sensor cabinet from the planned Home Assistant adapter and records that ESP32 sensor-display acceptance is paused pending hardware.

Verification: cargo fmt --check; cargo clippy --all-targets --all-features -- -D warnings; cargo test; git diff --check. All passed. PostgreSQL, Yandex LLM, SpeechKit, and local-asset/database-gated integration cases remained ignored by their explicit guards.

## SRCH-001: non-blocking ONNX inference and session pool (implemented locally, 2026-09-24)

Made local ONNX Runtime inference non-blocking for the Tokio async runtime and added concurrent multi-session inference support:
1. `src/providers/onnx_e5.rs`: added `session_pool_size` (defaulting to CPU parallelism clamped between 1 and 4, configurable via `ROCKSERVER_ONNX_SESSION_POOL_SIZE`). Implemented `SessionPool` with semaphore backpressure (`tokio::sync::Semaphore`) and an RAII `PooledItem` guard that returns the session to the pool before releasing the semaphore permit (even on error or thread panic).
2. Offloaded CPU-heavy tokenization, tensor construction, `Session::run(...)`, and mean-pooling to `tokio::task::spawn_blocking`, leaving Tokio worker threads completely unblocked for WebSocket audio streaming and HTTP request handling.
3. Hardened inference error handling by mapping task panics and task cancellation to safe `EmbeddingProviderError` summaries without exposing sensitive query text in error logs.
4. Added unit test suite in `src/providers/onnx_e5.rs` covering bounded pool defaults, environment parsing/lookups, config validation, high-concurrency bounded execution, panic recovery/session recycling, input prefixing, and mean pooling L2 normalization. Extended `tests/onnx_e5_live.rs` with concurrent session pool smoke coverage.
Verification passed: `cargo fmt --check`, strict all-targets/all-features Clippy (`cargo clippy --all-targets --all-features -- -D warnings`), `cargo test` (172 tests passed), and `cargo test --all-features` (179 tests passed, 0 failed).

## SEARCH-ROADMAP-001: voice search and vector retrieval roadmap (recorded, 2026-09-24)

Audited search architecture and voice pipeline following the comprehensive technical review from 2026-08-29. Decomposed 14 findings into an actionable multi-phase task matrix for autonomous agents and published [`docs/roadmap/voice-search-improvements.md`](roadmap/voice-search-improvements.md):
1. Phase 1 (P0): Runtime unblocking and ANN retriever correctness (`SRCH-001` non-blocking ONNX inference via `spawn_blocking`/pool, `SRCH-002` HNSW ANN retriever CTE in PostgreSQL, `SRCH-003` prefilter ordering fix, and `SRCH-004` lexical gate for language classifier).
2. Phase 2 (P1): Ranking fusion and intent resilience (`SRCH-005` Reciprocal Rank Fusion (RRF), `SRCH-006` conversational noise query stripping, `SRCH-007` dynamic taxonomy synchronization from DB, and `SRCH-008` LLM circuit breaker / LRU intent cache).
3. Phase 3 (P1, RockCast repository boundary): `RC-VOICE-001` Polyphase/windowed anti-aliasing resampling (48->16 kHz) and client-side VAD.
4. Phase 4 (P2): Operations and evaluation (`SRCH-009` incremental hashed embedding backfill, `SRCH-010` model version migration, `SRCH-011` golden evaluation dataset with Recall@K/MRR, and `SRCH-012` log privacy and telemetry).
Documentation-only planning task; no runtime or schema behavior changed.

## SEARCH-PERF-001: voice search timeout latency optimization (deployed, 2026-09-24)

Resolved voice search timeout failures (504 `search_timeout`) in RockCast and RockMobile:
1. `src/providers/yandex_llm.rs`: switched default LLM model from `"yandexgpt"` to `"yandexgpt-lite"` to reduce generation round-trip latency, set `DEFAULT_TIMEOUT_MS` to 4,500 ms (4.5s), and hardened environment parsing against empty string overrides.
2. `src/search/mod.rs`: parallelized query parsing (`self.query_parser.parse`) and local text embedding (`self.query_embedding`) via `tokio::join!`, completely hiding the ~150ms ONNX embedding computation behind the external network round-trip.
3. `src/persistence/postgres.rs`: marked `candidates` and `scored` CTEs in `SEARCH_SQL` as `MATERIALIZED`, preventing PostgreSQL 17 from re-evaluating correlated subqueries (`regexp_split_to_table`, `similarity()`, FTS matching) across candidate rows during ranking and filtering. Cuts catalog search query time from ~850–1060ms down to ~147ms (~7x speedup).
4. `deploy/compose.yaml`, `deploy/ops-001-d.psm1`, `deploy/remote-ops-001-d.sh`: allowlisted `YANDEX_LLM_MODEL` and `YANDEX_LLM_TIMEOUT_MS` for optional operator overrides through `release.env`.
Verification passed: `cargo fmt --check`, strict all-targets/all-features Clippy, `cargo test` (172 tests passed), and `deploy/tests/ops-001-d-tests.ps1`.
Production release `db3066d` succeeded on `rockplatform.win` (`readiness=passed`). Live production latency dropped from >5,000ms (timeout) down to 800–850ms per search request.

## YANDEX-HOME-005: all-property sensor extraction and multi-metric device cards (deployed, 2026-09-24)

Broadened Yandex Smart Home sensor extraction from single-metric temperature filters to all devices and sensor properties with available readings:
1. `src/providers/yandex_home.rs`: removed the `retrievable: true` restriction (which discarded battery-powered Zigbee sensors like Climate and Temperature sensors that report `retrievable: false` but hold valid cached measurements in `state.value`), and removed the `instance == "temperature"` filter. Implemented `parse_home_devices` and `home_devices`, extracting temperature, humidity, battery level, voltage, power, amperage, CO₂, pressure, and formatting units and localized property names.
2. `src/http/yandex_home.rs`: extended `/api/v1/browser/yandex-home/sensors` to return structured `devices` with their full property lists alongside a backward-compatible flat `sensors` array.
3. `api/openapi.yaml`: updated contract schemas (`YandexHomeSensors`, `YandexHomeDevice`, `YandexHomeDeviceProperty`, `YandexHomeSensor`).
4. `web/src/api.ts` & `web/src/app.tsx`: updated web client types and user cabinet rendering to display device cards with room labels, all sensor metrics (temperature, humidity, battery, etc.), and latest update timestamps.
Verification passed: `cargo fmt --check`, strict Clippy, `cargo test` (including openapi contract checks), `pnpm typecheck`, `vite build`, and `ux-regression.mjs` (11 passed). Production release `489c2a0` succeeded and public `https://rockplatform.win/health/ready` returned `{"status":"ok"}`.

## YANDEX-HOME-004: cross-site OAuth callback cookie relaxation and web status notifications (deployed, 2026-09-24)

Fixed the Yandex Smart Home OAuth callback failure (`/?yandex_home=failed`):
1. `src/http/auth.rs`: updated browser session cookie `rockserver_browser` from `SameSite=Strict` to `SameSite=Lax`, enabling browsers to send session cookies on top-level cross-site navigation redirects back from `oauth.yandex.ru`.
2. `src/http/yandex_home.rs`: relaxed callback cookie requirement; the single-use, unguessable cryptographic OAuth state consumed from the database authoritatively binds the callback to the owner account. If a cookie is present, it verifies that it matches the owner. Added structured logging for callback failures.
3. `web/src/app.tsx`: added handling of `yandex_home` query parameter (`connected` / `failed`), cleaned up the URL via `history.replaceState`, and displayed a clear Russian-language status message in the Yandex Home section of the account cabinet.
Verification passed: `cargo fmt --check`, strict Clippy, `cargo test`, `pnpm typecheck`, `vite build`, and `ux-regression.mjs` (11 passed). Production release `d977ded` succeeded and public `https://rockplatform.win/health/ready` returned `{"status":"ok"}`.

## YANDEX-HOME-003: fix PostgreSQL string concatenation syntax error in yandex_home store (deployed, 2026-09-24)

All 6 SQL statements in `src/persistence/account_postgres/yandex_home.rs` previously contained literal `+` string-concatenation characters, causing PostgreSQL syntax errors (`syntax error at or near "WHERE"`) during `GET /api/v1/browser/account` when evaluating `store.has_yandex_home_connection(user_id)`. Replaced the stray `+` symbols with standard Rust string line continuations (`\`) and added an opt-in integration test in `tests/postgres_integration.rs` covering all 6 methods of the Yandex Home connection store. Verification passed: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`. Production release `bc15226` succeeded and public `https://rockplatform.win/health/ready` returned `{"status":"ok"}`.

## YANDEX-HOME-002: deployment configuration (deployed, 2026-09-24)

The protected OPS-001-D release path now carries `YANDEX_HOME_CLIENTID` and
`YANDEX_HOME_SECRET` from the ignored root `.env` into the root-only VPS
`release.env`, and Compose passes them only to the RockServer service. Values
remain allowlisted, absent when unset, replaced on a later deploy, and omitted
from summaries. Verified by the deployment-script regression suite, `cargo fmt
--check`, strict all-target/all-feature Clippy, and `cargo test` (172 library
tests; PostgreSQL and live-provider tests remain explicitly ignored).
Production release `a6e029b` succeeded and public
`https://rockplatform.win/health/ready` returned `{"status":"ok"}`.

## YANDEX-HOME-001: user-linked temperature sensors (deployed, 2026-09-24)

Signed-in browser accounts can now connect Yandex Smart Home through OAuth and
read only retrievable `devices.properties.float` temperature properties. The
link uses a one-time, account-bound state, and the user is returned to the
account cabinet after consent. OAuth access tokens are AES-256-GCM ciphertext
at rest, never returned by the API or rendered in the browser; a rejected or
invalid provider token is revoked and requires a new connection. The cabinet
shows only device/room labels, current temperature, unit, and source update
time. The implementation requires `YANDEX_HOME_CLIENTID` and
`YANDEX_HOME_SECRET`, plus the exact registered callback
`https://rockplatform.win/api/v1/browser/yandex-home/callback`.

Verified: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo
test` (172 library tests; PostgreSQL and live-provider tests remain explicitly
ignored), `pnpm build`, and `pnpm test` (11 passed).

## RS-ICON-012: false icon-import failure reduction (implemented locally, 2026-09-24)

Production sampling of the first import job (3,919 processed: 44% ready,
23% retryable, 27% permanent) showed that about two thirds of the errors
were artifacts of the fetcher: homepages heavier than the 256 KiB
inspection bound and sites that block requests without a User-Agent. The
fetcher now sends a browser-like User-Agent, the homepage limit is 1 MiB,
and oversized sources classify as permanent rather than infinitely
retryable; transport failures remain the only retryable class. Dead domains
(~17% of the sample) stay permanent by design. Verification passed:
`cargo fmt --check`, strict all-target/all-feature Clippy, and full
`cargo test` (225 passed). Deployment and a rerun job are pending.

## WEB-ADMIN-UI-001: desktop-first administrator console (implemented, 2026-09-24)

The `/admin` console now uses an isolated desktop-first layout with a persistent
sidebar, top-level session controls, wide table views, clearer station import
controls, and a dedicated administrator login card. The user account cabinet,
pairing screens, API calls, and interaction behavior were left unchanged.
The implementation intentionally uses scoped CSS instead of adding Tailwind or
another runtime dependency. Verification passed: `pnpm build` and
`pnpm test` (11 UX regression tests).

## RS-DOMAIN-001: production move to rockplatform.win (deployed, 2026-09-24)

The production deployment relocated from `alex.vault57.ru` to `rockplatform.win`
on a new host with fresh credentials and a fresh database; the old host and its
data remain untouched. The domain-bound constants are updated: the WebAuthn RP
id/origin, the first-party origin accepted for browser state changes, and the
OpenAPI server URL. Origin-dependent tests reference the constants.
`rockplatform.win` resolves through a Cloudflare proxy; the Let's Encrypt
certificate was issued through it without changes. Verification passed:
`cargo fmt --check`, strict all-target/all-feature Clippy, and full
`cargo test` (225 passed). **Production deploy:** host `217.60.60.57` was
bootstrapped and released with commit `a8f21aa`, then `84cb4eb` (adds the
previously missing `bootstrap_admin` binary to the deployment image), both
`status=succeeded` with `readiness=passed`. The fresh database seeded 16,825
stations with 0 failures. Public checks on `https://rockplatform.win`:
`/health/ready` 200, search 200, unknown-station icon `404`, `/admin` 200.
The administrator was bootstrapped and login verified (200 Bearer). The first
station-icon import was started through the protected admin API and homepage
favicon discovery is producing ready artifacts: after the first ~150 processed
items the persisted counters showed `ready=52` with `missing=30`,
`retryable_error=32`, `permanent_error=37`; a ready icon is served as
`200 image/webp` with a strong ETag and `304` on `If-None-Match`. The import
job continues in the background over all 16,825 stations; its saved progress
is visible on the Stations tab.

## Station-icon direction (replanned, 2026-09-22)

The approved icon plan has no CLI, cron, startup, or deployment sync path. A
protected administrator at `https://rockplatform.win/admin` starts a durable
background import job, polls its persisted counters, and sees server-hosted
thumbnails or placeholders in the station list. Manual upload, replace, and
confirmed removal preserve manual precedence over future catalog refreshes.
Production mounts persistent storage but never starts an import.

## RS-ICON-011: homepage favicon discovery (implemented locally, 2026-09-22)

The first production import correctly reported 16,825 missing and 0 ready:
the pinned production catalog release carries no explicit favicon URLs
(`full_release` imports set no icon source), and the worker previously used
only an already-stored `station_icons.source_url`, which was empty. The
import now also applies the roadmap's second-priority source for stations
with a homepage (15,810 of 16,825 in production): a bounded SSRF-checked
homepage fetch, the first `<link rel="icon">`/`apple-touch-icon` target, or
the `/favicon.ico` fallback, recorded as priority 1 below explicit catalog
sources. Ready metadata is published through an upsert so stations without a
metadata row become ready atomically; unsuccessful HTTP statuses classify as
permanent and transport failures as retryable. Verified by `cargo fmt
--check`, strict all-target/all-feature Clippy, and full `cargo test` (225
passed; 15 PostgreSQL/live tests ignored as designed).

## RS-ICON-007: admin import and server delivery (implemented locally, 2026-09-22)

`POST /api/v1/admin/icons/import` now requires an active administrator Bearer
session and the trusted first-party Origin before it creates and detaches one
durable import worker. `GET /api/v1/admin/icons/import` restores the latest
safe persisted counters after a page reload, and `GET` with a job UUID polls
one job. The first-party station list can start the job, shows its live counts,
and renders a server URL thumbnail with a local placeholder on 404.

`GET /api/v1/stations/{station_id}/icon` reads only ready prepared WebP files:
it does not fetch a source, redirects nowhere, uses a content-hash ETag, and
returns non-cacheable 404 for absent/corrupt storage. The importer is available
only when production config supplies `ROCKSERVER_STATION_ICON_DIR`. Process
startup makes an unfinished persisted worker `interrupted` through a metadata
update only; startup and deployment never initiate network import. Required
local checks passed: `cargo fmt --check`, strict all-target/all-feature Clippy,
and full `cargo test` (all regular tests; 10 PostgreSQL and 5 live-provider
tests remain ignored because no disposable `TEST_DATABASE_URL` or live
credentials were provided). `pnpm typecheck` and `pnpm test` also passed.

## RS-ICON-008: manual administrator override (implemented locally, 2026-09-22)

The Stations tab now offers a native file picker for a specific station and a
confirmed removal action. `PUT /api/v1/admin/stations/{station_id}/icon` accepts
only a bounded raw raster body, applies the existing signature/decode/WebP
normalizer, and sets a manual override without discarding the automatic source.
`DELETE` requires JSON confirmation `DELETE`, detaches only a manual override,
and deliberately retains its content-addressed artifact for a later safe orphan
cleanup. Both endpoints require the active administrator session and trusted
first-party Origin. Required local checks passed: `cargo fmt --check`, strict
all-target/all-feature Clippy, full `cargo test`, `pnpm typecheck`, and `pnpm
test`. PostgreSQL integration tests remain ignored because no disposable
`TEST_DATABASE_URL` was supplied.

## RS-ICON-009: catalog and search favicon URL (implemented locally, 2026-09-22)

The station domain and public/search/voice DTOs now always serialize nullable
`favicon_url`. PostgreSQL adds a `LEFT JOIN station_icons` and supplies the
same-origin `/api/v1/stations/{id}/icon` path only for `status = 'ready'`; it
does not alter ranking, ordering, streams, source URLs or offline catalogs.
The prepared endpoint remains the authority that verifies artifact presence at
delivery time. `cargo fmt` and `cargo check` passed; full verification is
complete: `cargo fmt --check`, strict all-target/all-feature Clippy and full
`cargo test` passed. The 10 PostgreSQL integration tests remain ignored because
no disposable `TEST_DATABASE_URL` was supplied.

## RS-ICON-010: persistent production icon storage (deployed, 2026-09-22)

The standard deployment creates `/home/rockserver/station-icons` with UID
`10001` ownership, mounts it to `/var/lib/rockserver/station-icons`, and sets
`ROCKSERVER_STATION_ICON_DIR` only for the RockServer service. Thus prepared
artifacts survive a container replacement while the catalog seed does not need
the volume. The deployment process does not create or run an import job.

Final whole-change verification passed 2026-09-22: `cargo fmt --check`,
strict all-target/all-feature Clippy, full `cargo test` (216 passed; 15
PostgreSQL/live-provider tests ignored as designed), `pnpm typecheck`,
`pnpm test` (11 passed), the OPS-001-D local regression suite, and
`ops-001-d.ps1 -Action deploy -DryRun`. This closes the per-stage deferred
verification of RS-ICON-002 through RS-ICON-010.

**Production deploy:** commit `179c9a8` was released with
`deploy/ops-001-d.ps1 -Action deploy` on 2026-09-22; the VPS deploy worker
returned `status=succeeded` with `readiness=passed`. Independently
confirmed: public `/health/ready` returned `HTTP 200 {"status":"ok"}`,
unknown-station `GET /api/v1/stations/{id}/icon` returns the contract `404`
with `Cache-Control: no-store`, and public search serializes
`favicon_url: null` for all stations because no import job has run. The next
step is the operator's explicit first icon import from the Stations tab at
`https://alex.vault57.ru/admin`; deployment itself never starts it.

## RS-ICON-002: metadata schema (implemented locally, 2026-09-22)

Migration `0022` adds only the `station_icons` metadata table and a due-import
index. It keeps file bytes outside PostgreSQL, enforces the ready-WebP metadata
invariant, and performs no network I/O. No icon endpoint, importer, job or UI
behavior is enabled by this schema-only step.

## RS-ICON-003: catalog icon sources (implemented locally, 2026-09-22)

The shared-catalog `faviconUrl` and Radio Browser `favicon` fields now enter
the internal `favicon_source_url` import path after HTTP(S) normalization.
PostgreSQL records the source and marks changed automatic sources for refresh
without overwriting a manual override or removing a ready artifact.

## RS-ICON-004: persistent artifact storage (implemented locally, 2026-09-22)

`station_icons` now provides a content-addressed WebP storage boundary and a
filesystem implementation rooted outside PostgreSQL. It accepts only canonical
hash keys, publishes through same-directory rename, and has no downloader or
HTTP route attached yet.

## RS-ICON-005: source validation and normalization (implemented locally, 2026-09-22)

The new boundary accepts only bounded PNG/JPEG/WebP/ICO source bytes, decodes
them within fixed pixel limits and stores only a square WebP result. External
fetching uses bounded redirects and rejects localhost/private/link-local DNS
results before each request. It is not exposed through HTTP and no job starts
it yet.

## RS-ICON-006: durable import coordinator (implemented locally, 2026-09-22)

Migration `0023` snapshots eligible stations into one active durable icon job.
The coordinator claims items, records terminal counters and starts no work by
itself. The admin route/UI integration is recorded separately as RS-ICON-007.

## Administrator station-list page size (implemented locally, 2026-09-22)

`GET /api/v1/admin/stations` and the first-party administrator console now
default to 50 stations per page, which is also the existing maximum. The
station pager advances by 50; administrator device and audit pages retain
their independent default of 25.

## RS-9: bounded station presentation in server-to-target playback (implemented locally, 2026-09-17)

The `station.play_stream` delivery created only after RockServer resolves a
controller's `station.play_station` now includes
`station: { name, icon_url }`. The exact `station_id` remains the authoritative
playback identity and is the only station fact that RockCast publishes in its
runtime state. `name` repairs a target-local fallback entry so `Now playing`
shows the selected station rather than an opaque catalog UUID.

This is deliberately target-only presentation, not a catalog transfer or a
new controller response: the server resolves the current catalog record,
attaches its bounded name, and sends the existing resolved stream only to the
target. The original controller command remains the only persisted/fingerprinted
input; no stream URL or presentation object appears in controller lifecycle
frames, directory state, logs, or error text. The current catalog has no icon
field, therefore the truthful server value is `icon_url: null`. The OpenAPI
schema and validation reserve a bounded public HTTP(S) `icon_url` for the
future catalog field without another wire change.

Rollout is target first, then server: the updated RockCast accepts an older
delivery with no `station` object, while its older strict parser would reject a
new field. Server checks passed: `cargo fmt --check`, strict
`cargo clippy --all-targets --all-features -- -D warnings`, full `cargo test`
(all regular tests passed), and `git diff --check`. **Production deploy:**
commit `509ea0c` was released with `deploy/ops-001-d.ps1 -Action deploy` on
2026-09-17; the VPS deploy worker returned `status=succeeded` and public
`/health/ready` independently returned `HTTP 200 {"status":"ok"}`. The
explicit server-only rollout occurred before RC-4b is installed on the Windows
target, so do not send a catalog selection to the old strict RockCast parser;
update/restart RockCast first, then perform the physical USB acceptance.

## RS-8: owner-scoped `runtime_state` projection in the directory (2026-09-17)

`DeviceControlDirectoryEntry` now carries an optional, owner-scoped
`runtime_state` field holding the revisioned `DeviceStateSnapshot`
(`state_revision`, `observed_at`, `received_at`, playback/volume/output/display).
It is served by the two existing transports only — REST
`GET /api/v1/device-control/directory` and the control WSS
`directory.snapshot`/`directory.upsert` fan-out, which share one DTO builder —
with no new endpoint, transport, pairing, or polling. The value comes from the
persisted latest-snapshot projection (`DeviceControlStore::load_device_state`),
never from the command lifecycle, and `state_revision` stays the device's own
monotonic revision.

Scope gating matches entity states: the field is serialized only for callers
holding `entity.state.read` (controller-role manifests via `granted_scopes`).
A target that never published state keeps the field absent from the JSON — not
`null`, not a fabricated `stopped`/`0%` — and `state_freshness` remains a
separate always-present fact. Cross-account isolation is unchanged: a foreign
account still does not see the entry at all.

Contract and fixtures: `api/openapi.yaml` 0.5.0 declares `runtime_state` as an
optional `$ref` to the existing `DeviceStateSnapshot`; the golden
`directory-snapshot-server.json` now shows two entries with `runtime_state`
(RockCast playing `station-rock-001` at 62%, ESP32 at revision 8 consistent with
the state-full/delta fixtures) plus a legacy offline entry demonstrating absence,
and the new golden `directory-upsert-server.json` demonstrates a local volume
change propagating with a strictly higher `state_revision`.

Verification: 156 library unit tests and 8 OpenAPI contract tests pass,
including the new directory projection tests, the strengthened stale-revision
guard test, and the hub latest-snapshot test. `cargo fmt --check`, strict
all-target/all-feature Clippy, full `cargo test` (211 passed overall), and
`git diff --check` pass. PostgreSQL-gated and live tests remain ignored as
usual. **Production deploy:** commit `511dd7b` (RS-8) was released through
`deploy/ops-001-d.ps1 -Action deploy` on 2026-09-17; the VPS applied embedded
migrations and the release gate returned `readiness=passed`, independently
confirmed as `HTTP 200 {"status":"ok"}` on the public `/health/ready`.
Physical acceptance (Phase 4, USB RockCast + RockMobile) is not claimed;
RockCast publishing (RC-4/Phase 1) and the RockMobile state-driven UI
(RM-4/Phase 3) are implemented in their repositories — see
[`roadmap/rockmobile-rockcast-live-control.md`](roadmap/rockmobile-rockcast-live-control.md).

## RC-4 / RM-4: authoritative live playback UI — implemented locally, physical handoff pending (2026-09-17)

The deployed RC-3 path has live acceptance: RockMobile can explicitly send
`station.play_station` and standard playback commands to a paired online
RockCast. RockServer already resolves that station command to a catalog-validated
`station.play_stream`; no client needs or may receive a raw stream URL.

The user-facing model is now present locally: RS-8 projects revisioned playback
and volume state through the existing owner-scoped directory, RC-4 publishes
truthful target facts, and RM-4 uses them rather than optimistic command success.
RS-9/RC-4b add the bounded display name needed for a station absent from the
local RockCast cache; its future nullable icon URL remains target-only. The
implementation record, UI states, API amendment, acceptance criteria, and
cross-repository deployment order are recorded in
[`roadmap/rockmobile-rockcast-live-control.md`](roadmap/rockmobile-rockcast-live-control.md),
with interactive mockups and embedded Hi-Res artwork consolidated in
[`live-control-mockups.html`](live-control-mockups.html).
`media.chromecast` and `media.relay` remain explicitly out of this scope until
RS-7 supplies server routing.

## RS-5: canonical terminal command errors (2026-09-09)

Server-generated terminal `command.result` failures now use the canonical v1
`DeviceControlError` shape: `code`, `message`, the original `device.command`
envelope `message_id` as `request_id`, and an empty object for `details`.
The ID is retained through asynchronous timeout and disconnect completion, so a
replayed stored result retains the originating request correlation. No fixture
or other wire shape changed.

Rockmobile's device-control JSON codec is forward-compatible
(`DirectoryDtos.kt:9` sets `ignoreUnknownKeys = true`), so it safely accepts
the added `details` field. Focused command-router and OpenAPI fixture checks,
formatting, strict Clippy, and the full Rust test suite passed.

## RS-4: device voice sessions route typed radio intents through device control (2026-09-09)

`/api/v1/voice/stream` now authenticates native device sessions with the same server-derived
principal boundary as control ingress. A device session must announce its declared `voice.main`
surface; `ready` echoes that surface and the server-derived `source_device_id`. Missing or invalid
device credentials fail before upgrade, while the anonymous and legacy station-search behavior and
result shape remain unchanged.

After final recognition, the device branch classifies only the bounded radio operations, converts
them to the existing `UserIntent` model, resolves an explicit current target equal to the source
device, and submits the resulting command through `CommandRouter.submit`. Play selects a unique
top-ranked station and submits `station.play_station`; RS-3 alone performs the
`play_station -> play_stream` resolution. Stop submits `playback.stop`; absolute volume submits
`volume.set_volume`. Other intents fail as `unsupported_intent`, tied top stations as
`clarification_required`, and target/capability/lifecycle failures retain deterministic frozen
codes. Terminal command status returns through the device-only voice result.

Cancel is implemented from start until commit and emits exactly one terminal `cancelled` error
after releasing recognition state. The frozen transport limits remain unchanged. The authorized
minimal RS-1 amendment adds the closed `VoiceDeviceCommandResult` and a complete
`VoiceStreamErrorCode`; only the cancel planned marker and planned wording for RS-4 fields were
removed. No session tail is persisted, and raw media or recognized text is absent from logs.

Verification: `tests/voice_stream_api.rs` 9/9 and `tests/openapi_contract.rs` 8/8 pass. The voice
suite covers routed play/stop/volume commands on the authenticated source target, router-only
play-stream resolution, succeeded/failed terminal status, cancel/resource release, invalid native
401 plus anonymous compatibility, ambiguity, unsupported intent, unchanged bounds, silence,
recognition failure, offline target, search timeout, and captured-log privacy.
`cargo fmt --check` and strict all-target/all-feature clippy pass; full `cargo test` reports 206
passed and 15 ignored external integration/live tests.

## RS-3: station.play_station resolves to server-validated play_stream (2026-09-09)

Task RS-3 of the rockcast-device-plan (coordinated from `rock-esp32/docs/plan-control.md`)
implemented the Step-3 command-router resolution frozen by RS-1 in `api/openapi.yaml` 0.5.0:

- `CommandBody` now carries the typed `station.play_stream` body with the two frozen variants:
  server-resolved `source=rockserver_catalog` (echoing the resolved `station_id`) and
  controller-supplied `source=direct_stream` (no station claim). Wire parsing enforces the
  strict variant shapes of the `StationCommand` schema.
- On `station.play_station` the router validates the target (player role plus `media.station`
  advertising `rockserver_catalog`), reserves the lifecycle, then resolves the station through
  the new `StationCatalog` boundary (`SearchService::public_station`, the same catalog as the
  public/device routes, 5 s budget), validates the resolved `stream_url` with the new literal
  SSRF gate, and dispatches `station.play_stream` to the target **under the same command_id**.
  The idempotency fingerprint and the persisted reservation keep the ORIGINAL controller
  command, so duplicate submissions replay the stored lifecycle without re-resolution, and the
  stream URI never enters persistence, controller frames, logs, or error texts.
- Deterministic failures terminate the lifecycle with `invalid_payload` and fixed messages
  (unknown station; station without a playable stream; invalid or non-public stream address);
  transient catalog errors/timeouts terminate with retryable `persistence_unavailable`; a
  controller sending the server-only variant, or a direct stream to a target that does not
  advertise `direct_stream`, is rejected with `invalid_payload`/`capability_not_supported`.
- The SSRF gate validates URI form only (scheme, host, no userinfo/fragment, port 1..=65535
  or scheme default, ≤2048 chars) plus literal IPv4/IPv6 and always-local names (loopback,
  private, shared, link-local, unique-local, multicast, benchmarking, documentation and
  reserved ranges; `localhost`; purely numeric hosts). DNS resolution and per-redirect checks
  are a documented limitation: the repository has no shared egress layer, and the frozen
  contract assigns redirect checking to the target's bounded stream client (RE-5).
- `x-rockserver-status: planned` was removed from the `StationStreamUri` schema (now
  `implemented`) and the info description was updated; the voice planned markers (RS-4) are
  untouched. All production router builders (including PostgreSQL) now wire the catalog into
  the command router.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
and `cargo test` pass. Router tests (6) cover the happy path (same command_id, resolved body
delivered only to the target, echo of station_id), five deterministic failures with
idempotent replay and URI-leak assertions on serialized controller frames, transient catalog
failure, controller-supplied play_stream gating, and capability/wiring rejections. Command
unit tests cover both wire variants and a 40+-case `validate_stream_uri` battery. Contract
tests stay green (`tests/openapi_contract.rs` 8/8 now asserts the implemented stream-URI
schema and that the voice cancel frame stays planned; `tests/device_catalog_api.rs` 12/12
unchanged).

Known limitations: DNS-level SSRF (a hostname that only resolves to a private address passes
the literal gate) and server-side redirect checking are deferred to a future egress layer;
terminal `command.result` errors serialize as `{code, message}` without the
`request_id`/`details` fields shown in the golden failed-result fixture — a pre-existing
divergence since DC-003, unchanged by RS-3 and reported to the control center. Direct
`playback.*`/`volume.*` behavior is unchanged. RS-4 (voice device-session/cancel) is pending.

## RS-2: device-facing catalog runtime implemented (2026-09-09)

Task RS-2 of the rockcast-device-plan (coordinated from `rock-esp32/docs/plan-control.md`)
implemented the two Step-3 device-catalog routes frozen by RS-1 in `api/openapi.yaml` 0.5.0:

- `GET /api/v1/device-control/catalog/stations` (browse): cursor = stable station ID of the
  last page item (`^[A-Za-z0-9-]+$`, ≤512), limit 1..=20 (default 20), `next_cursor` null
  exactly when no further page may exist. Reuses `SearchService::public_catalog`.
- `GET /api/v1/device-control/catalog/search`: GET query parameters — `q` (1..=128 chars,
  non-whitespace-only), `locale` (BCP 47-like, default `en-US`), `limit` 1..=20 — one ranked
  page with no cursor. Reuses `SearchService::interpret_and_search` under the same 5 s budget
  as the public search route; a timeout maps to the contract's 503 (the device contract has
  no 504).
- Authentication reuses the existing device-session mechanism: the short-lived native
  RockserverBearer from `POST /api/v1/auth/device-session` is resolved through
  `authenticate_control_ingress`/`NativeSessionResolver` (same path as
  `/device-control/directory`). Missing, unknown, expired, revoked, or non-native
  credentials return 401; an unavailable session store returns a retryable 503. No new token
  format and no extra scope: v1 needs only a valid non-revoked session (RS-1 decision).
- Responses are `DeviceCatalogPage`/`DeviceSearchPage` of `DeviceStationDto`: the mapping
  from domain stations drops `stream_url`, ranking fields, and provider/persistence
  identifiers by construction; `Cache-Control: no-store` on 200s.
- Rate limits mirror the public routes numerically (browse 60/20, search 30/10) but buckets
  are per authenticated device, so one device cannot exhaust the fleet quota; 429 carries
  `Retry-After` and `details.limit_scope = "device"`.
- Unknown or oversized parameters (including unknown query keys) are rejected
  deterministically with 400.
- Side fix surfaced by the cursor-walk test: `InMemoryStationRepository::list_public` and
  `list_admin` now sort by stable station ID like the PostgreSQL `ORDER BY s.id`; previously
  the unsorted pinned catalog could repeat stations across cursor pages.
- `x-rockserver-status: planned` was removed from exactly these two operations (now
  `implemented`); the play_stream (RS-3) and voice (RS-4) planned markers are untouched.
- New public builder `router_with_search_service_and_native_session_resolver` allows tests
  and firmware-facing harnesses to run the router with a deterministic session resolver.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
and `cargo test` pass. New `tests/device_catalog_api.rs` (12 tests) covers happy paths,
cursor pagination across all 41 builtin stations, parameter bounds (limit/cursor/q/locale,
unknown keys), 401 without/expired session, retryable 503s, search timeout mapping,
stream-URL-free responses (including stations with invalid stream URLs), and per-device
rate isolation. `tests/openapi_contract.rs` (8/8) now asserts the implemented status and
that both routes are registered and require a native device session.

Known limitations: the catalog is account-independent and requires no device-control scope;
the contract's 403 remains reserved (revocation is observed as 401 today). RS-4
(voice device-session/cancel) is pending; the RS-3 resolution this section awaited is
implemented above.

## RS-1: RockCast-radio contracts frozen before implementation (2026-09-09)

Task RS-1 of the rockcast-device-plan (coordinated from `rock-esp32/docs/plan-control.md`)
froze the Step-2 contracts in `api/openapi.yaml` 0.5.0 without any runtime change:

- Two `x-rockserver-status: planned` device-facing catalog paths behind the RockserverBearer
  device session — cursor-paginated browse (`GET /api/v1/device-control/catalog/stations`,
  explicit stable-ID cursor, at most 20 per page) and one-page ranked search
  (`GET /api/v1/device-control/catalog/search`, query at most 128 characters, at most 20
  results, no cursor by the F10 two-path decision). Both return the new `DeviceStationDto`
  without `stream_url`; the public anonymous catalog and search routes are unchanged.
- `station.play_stream` is now structurally enforceable: a server-only
  `source=rockserver_catalog` variant (requires the resolved `station_id`) and a controller
  `source=direct_stream` variant; SSRF bounds live in the shared `StationStreamUri` schema.
- The voice stream contract matches the enforced runtime exactly (16 kHz mono pcm_s16le,
  32 KiB chunks, 2 MiB/60 s session, 10 s idle, 75 s wall, 15 s provider, result limit 10)
  and documents the planned RS-4 device-session extension (`voice.main` surface,
  server-derived `source_device_id`, explicit `VoiceStreamCancel`).
- Decision recorded: `station_list`, `search`, `loading`, `offline` and `playback_error` are
  local device UI states; remote presentation views stay exactly `text`, `now_playing`,
  `sensor_grid`.
- Canonical fixtures updated: the ESP32 register fixture is the truthful radio shape
  (player/controller/display_surface/voice_endpoint, `media.station` = rockserver_catalog
  only, volume 0/100/1, `display.main` + `voice.main`, no sensors), manifest revision 2 and
  the directory snapshot stay coherent, and `device-catalog-response.json` demonstrates a
  stream-URL-free catalog page. The fixture README freezes the "accepted immediately, result
  async within 30 s" command semantics and the Rockmobile volume-compatibility rule.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
and `cargo test` pass; `tests/openapi_contract.rs` grew to 8 tests covering positive and
negative schema validation and proving the planned catalog routes are not yet registered by
the runtime router. RS-2/RS-3/RS-4 (implementation) remain pending.

## DC-019/DC-020 deferred; DC-039 selected (2026-09-09)

The owner has deferred DC-019 (ESP32 sensor modules) and DC-020 (sensor-to-display end to
end) because no physical sensors are currently available. They are not cancelled: their scope,
dependencies and acceptance criteria remain recorded, but no firmware or RockServer work should
begin until the hardware is available. The owner has instead selected DC-039 for execution,
authorizing the product decision required for its on-device interactive station browse GUI.
This does not change the protocol-v1 sensor scope or claim any sensor behavior as implemented.

## DC-018 GUI stack confirmed: LVGL v9 via esp_lvgl_port (2026-09-08)

The owner confirmed the DC-018 display-stack decision: LVGL v9 through the C `esp_lvgl_port`
component. MIT licensing, ESP32-P4 PPA hardware acceleration, built-in kinetic lists/scrolling
and ESP-IDF 6.1 Component Registry availability outweigh Slint's Rust-first developer
experience; Slint remains a documented fallback only if the cost of the owned thin binding
layer exceeds the paid embedded license. Rust owns protocol, state and presentation mapping;
the binding layer is owned firmware code, with the immature third-party v9 binding crates used
as references only. embedded-graphics stays limited to the DC-017 provisioning screen. The
decision is recorded in the roadmap task list (DC-018), the architecture roadmap Phase 5, and
the rock-esp32 `docs/rust-direction.md`. Documentation-only change; no runtime, OpenAPI,
migration or client behavior changed.

## External delegation via Antigravity CLI and first GUI-stack comparison (2026-09-08)

`docs/codex-project-context.md` now records a standing convention for delegating bounded
subtasks to Google Antigravity through the local headless CLI (`agy`, `%LOCALAPPDATA%\agy\bin`),
including allowed/forbidden task classes, secret-exclusion rules, model selection per call,
and the requirement that every load-bearing fact from an external answer is re-verified against
an independent source before any decision uses it.

The first delegated run compared LVGL v9 (esp_lvgl_port + Rust bindings), Slint, and
embedded-graphics as the DC-018 GUI stack for the JC4880P443C_I_W display, executed on
Gemini 3.1 Pro with a JSON-schema-bounded response. The external answer ranked Slint 8.5,
LVGL 7, embedded-graphics 4, and flagged Slint's embedded licensing as the deciding risk.
Independent verification confirmed and sharpened the three load-bearing claims: Slint's
royalty-free license excludes embedded devices, so a proprietary ESP32 product needs the paid
commercial plan (about $72 per developer seat per year) or GPLv3; LVGL v9 Rust bindings exist
but are new and API-unstable (`oxivgl`, `lightvgl-sys`), while the classic `lvgl` crate tracks
v8; Slint is officially distributed in the Espressif Component Registry with working ESP32-P4
demos but renders in software. The synthesized recommendation for DC-018: LVGL v9 through the
C `esp_lvgl_port` component with the Rust side owning protocol/state and a thin owned binding
layer, with Slint as the fallback if FFI friction proves costlier than the license; the final
stack decision stays with the owner as DC-018 requires. No runtime, OpenAPI, migration or
client behavior changed.

## Roadmap audit — Milestone D realigned with the connected ESP32 hardware (2026-09-08)

The device-control roadmap was audited from Milestone D onward against the live rock-esp32
checkout (JC4880P443C_I_W: ESP32-P4 v1.3 with no radio, ESP32-C6 Wi-Fi over ESP-Hosted/SDIO,
4.3-inch display, 16 MB flash; Rust hosted by ESP-IDF 6.1). The task list now matches verified
state: DC-010 and DC-012–DC-016 are marked complete with pointers to the client-side logs
(RockMobile → RockServer → RockCast physical command E2E accepted 2026-09-07; the Chromecast
hardware smoke remains unperformed). DC-017 records the board bring-up preconditions
(ESP-Hosted/SDIO Wi-Fi, SNTP, one HTTPS request), the minimal pairing screen (short code,
verification phrase, QR), the WSS transport decision under ESP-IDF 6.1, the RockCast code-reuse
strategy (transport-agnostic protocol layer and pairing state machine as future shared-crate
candidates in a separate repository; the egui GUI is not portable), and board-specific
acceptance (ESP32-C6 reset via GPIO54, SDIO power-cycle recovery, secret-free logs). DC-018 now
requires an explicit GUI-stack decision (LVGL, Slint or embedded-graphics) and bounds the v1 GUI
scope to `text`/`now_playing`/`sensor_grid` in a RockCast-like theme. A new product-gated
Milestone D2 records the deferred decisions: DC-039 interactive on-device station browse and
DC-040 local audio output; neither blocks Milestones E–G. The architecture roadmap's Phase 5,
repository-boundary and ordering sections were aligned with this order. This is a Markdown-only
planning change: no runtime, OpenAPI schema, migration or client behavior changed, so no build
or test run was needed.

## DC-016 forward full-state revision resync (2026-09-07)

Staging evidence showed RockCast trapped in a non-converging reconnect loop (one
`devices/connect` upgrade every ~51 s): the player's monotonic state counter had run ahead
of the server projection (client 19, stored 4) because facts may be published while the
socket is down, and both admission layers classified the reconnecting full snapshot as a
revision gap. A complete snapshot is the protocol's resync primitive, so a forward revision
now overwrites the stored projection in the in-memory state hub and in the PostgreSQL
latest-snapshot upsert instead of requesting another resync the client cannot satisfy.
Stale/replay/conflict handling, strictly ordered deltas, entity telemetry, manifests,
authentication, scopes and commands are unchanged; `api/openapi.yaml` documents the rule.
Verification passed: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test`
(146 library tests, 22 suites; PostgreSQL integration remains opt-in). Staging deployment
and the physical RockCast/RockMobile retest are recorded below after they complete.

## DC-016 stale full-state reconnect gate (2026-09-07)

A player reconnect may submit a syntactically valid but stale full snapshot after an earlier
connection has already advanced the server projection. The snapshot still cannot replace that
projection, but it now satisfies the reconnect state gate; both the in-memory and PostgreSQL
`Stale` outcomes are valid acknowledgements. Otherwise the first heartbeat closes a healthy
player roughly 20 seconds later. Revision gaps and conflicts still request a resync, and no
authentication, scope, command, directory or wire-schema behavior changed.

Focused control-plane tests passed locally. Staging deployment and physical retest are recorded
only after they complete.

## DC-016 E2E controller-state gate (deployed, 2026-09-07)

The control WebSocket now distinguishes a controller-only manifest from a device that publishes
runtime facts. A role set containing only `controller` can heartbeat immediately after registration;
it has no playback/volume/output state to fabricate. Player and hybrid manifests still require the
existing complete state snapshot and retain the resync gate. This is a lifecycle-only correction:
no public schema, authentication, pairing, scope, directory, command or persistence contract
changed. Verification passed with formatter, strict all-target/all-feature Clippy and `cargo test`;
PostgreSQL/live-provider suites remain opt-in. Staging commit `5c4698b` deployed successfully and
its external HTTPS readiness returned 200. Server WebSocket lifecycle tests, RockCast device-control
tests and RockMobile unit tests passed. Physical paired-device E2E remains pending.

## DOCS-001 — source navigation map (complete, 2026-09-04)

`src/ARCHITECTURE.md` now provides a compact, code-adjacent map of every root module, dependency
direction, focused control/persistence submodules, test locations, and safe reading order. The
crate root points to it and the persistent Codex context lists it among the reference documents.
It intentionally describes responsibilities and boundaries rather than duplicating implementation
or HTTP schemas, keeping code-navigation context small. No runtime, public API, persistence,
provider, authorization, or service-diagram behavior changed.

## REFACTOR-003 — layered domain and persistence boundaries (complete, 2026-09-04)

The largest remaining source modules were reduced to focused private units while preserving their
public Rust APIs. `src/device_control.rs` is now a compatibility facade over identities and
validation, manifest metadata, runtime state, commands, lifecycle values and store contracts.
`src/persistence/account_postgres.rs` is an account-store facade over account, browser, passkey,
pairing, rate-limit, cleanup and native-session implementations, with private row mappings kept in
one location. The existing unit tests for device control, command routing, intent resolution and
search orchestration now live beside their respective domain modules rather than among production
logic.

No endpoint, WebSocket/OpenAPI message, JSON serialization, authentication, scope, owner-scoping,
presence, state, command, idempotency, persistence, migration, configuration, logging or audit
behavior changed. The service diagram was reviewed and needs no update because the change only
introduces internal source boundaries. Verification passed: `cargo fmt --check`, strict
all-target/all-feature Clippy, `cargo test` (141 library tests plus regular suites), the four
OpenAPI contract tests, and `git diff --check`. PostgreSQL integration remains skipped because no
safe disposable `TEST_DATABASE_URL` was supplied.

## REFACTOR-002 — cohesive control and intent module boundaries (complete, 2026-09-04)

The device-control WebSocket lifecycle remains in `src/http/control.rs`, while its private v1
wire DTOs, frame bounds, serialization, parsing and protocol-close helpers now live in
`src/http/control/protocol.rs`; revision admission and state merge helpers live in
`src/http/control/state.rs`. `src/device_control_intent.rs` remains the transport-free resolver
facade, and its public schema, validation, contextual inputs and result types now live in the
private `src/device_control_intent/model.rs` module and are re-exported unchanged.

No endpoint, WebSocket/OpenAPI message, JSON serialization, authentication, scope, owner-scoping,
presence, state, command, idempotency, persistence, migration, configuration, logging or audit
behavior changed. The service diagram was reviewed and needs no update because this is an internal
code-boundary refactor only. Verification passed: `cargo fmt --check`, strict all-target/all-feature
Clippy, `cargo test` (141 library tests plus regular suites), OpenAPI contract fixtures, and
`git diff --check`. PostgreSQL integration remains skipped because no safe disposable
`TEST_DATABASE_URL` was supplied.

## DC-011 — typed intents and presentation builder (complete, 2026-09-03)

`device_control_intent` is a transport-free, versioned typed boundary for future text/voice
parsers. It accepts only bounded `UserIntent` variants (`play_radio`, sensor reads, media/volume,
now-playing, and narrow actuator proposals); it contains no raw command, provider call, SQL, URL,
or direct execution path. A caller supplies a server-derived actor, account-owned DC-010 directory
projection, server command ID/time, and request-local context. The resolver first validates the
intent, owner namespace and explicit scopes; it then checks live presence, role, capability,
manifest allowlist, target/entity/surface existence and state freshness before returning a typed
plan, clarification, confirmation or error. Roles/capabilities never substitute for scopes.

Resolution ordering is explicit IDs, then request-local current target/surface, then one compatible
candidate. No persistent preference was added. Directory v1 has no canonical home/area model, so
an area is rejected unless a caller supplied one explicit canonical mapping for that one request;
names and IDs are never guessed. There is no broadcast or offline fallback. `show_sensors` builds
only canonical `sensor_grid` cards and a specific display command. Fresh values remain current;
stale values preserve their value and `stale` freshness, while unavailable and missing values are
rendered as explicit `null` with unavailable/unknown state, never zero. `query_sensor` has no
display command and returns a bounded text presentation only for exactly one readable state.
`now_playing` requires known active observed playback and shows the truthful station ID because v1
state has no catalog-title field. Allowed actuator requests require explicit device/entity,
`actuator.control`, live allowlisted actuator capability and return confirmation only; lock/climate-
like or non-allowlisted entities are denied. Commands produced for media/display use the existing
DC-009-compatible typed model, including its declared volume/mute commands, but DC-009 remains the
sole dispatch/lifecycle router. No voice endpoint, LLM integration, persistent target preference,
Home Assistant adapter, client work, or public endpoint was added; CommandInterpreter integration
remains DC-025 and sensor-display delivery remains DC-020.

## DC-010 — controller directory (complete, 2026-09-03)

`GET /api/v1/device-control/directory` is now a runtime native-control projection, not a
replacement for the existing inventory/list/revoke route. It resolves only the existing native
`RockserverBearer` to server-derived account/device identity, returns 401 for invalid native
credentials and retryable 503 for unavailable control storage, then requires the server-computed
`device.directory.read` scope before reading any account-owned projection. The bounded v1
projection joins existing active account devices with their DC-005 manifest/latest state and DC-007
presence; entity values are omitted unless `entity.state.read` is granted. It exposes no pairing
or credential material, provider IDs, or user IDs. Only canonical `domain` and `device_class`
filters exist; `home` and `area` are not modelled and are rejected rather than treated as wildcards.

Registered controllers subscribe to the existing account-local bounded hub before their directory
snapshot is built. The returned `directory_revision` is the server watermark; every later owned
presence, manifest, device-state, or entity-state transition rebuilds only the affected permitted
entry and emits `directory.upsert`. A lagging receiver is closed with
`directory_resync_required`, so it reloads the HTTP snapshot rather than continuing from an
unknown gap. Reconnect replaces the old DC-007 socket generation, which also ends its subscription.
DC-011 intents/presentation and DC-015 client UI remain out of scope.

## DC-009 — live command routing (complete, 2026-09-03)

The authenticated control WebSocket now admits commands only from the active server-derived
principal whose registered manifest carries the controller role and an explicitly server-granted
command scope. Before durable reservation it checks same-owner namespace, exact active target
generation, target role/capability and the required entity/surface plus bounded typed payload.
The registry has a bounded per-connection server delivery channel, so only the one active target
generation receives a command; it never builds an offline queue or blocks a producer.

The lifecycle is `command.received`, target-only `command.accepted`, then exactly one terminal
`command.result`. Target timestamps are replaced with server observation time; a result is accepted
only from the matching owner/device/connection generation. Target loss/replacement and timeout
persist a terminal failure; duplicate or late result cannot alter it. The existing DC-005 command
store remains the single idempotency/audit mechanism: SHA-256 fingerprints cover the client-visible
canonical request for 24-hour replay, while a server-selected default deadline does not make an
otherwise identical retry conflict. Reused IDs with another owner, target, or fingerprint conflict.
The v1 contract has no explicit cancel message, therefore deadline/loss are the implemented
cancellation policy. DC-010's public directory/subscriptions and DC-011's user-intent/presentation
routing remain out of scope.

Verification passed: `cargo fmt --check`, strict all-target/all-feature Clippy, full `cargo test`
(132 library tests plus regular suites), OpenAPI contract fixtures, and `git diff --check`.
PostgreSQL integration remains skipped because no safe disposable `TEST_DATABASE_URL` was supplied.

## Graphify agent policy removed (2026-09-02)

The Cursor rule `.cursor/rules/graphify-usage.mdc` was deleted, and the matching Graphify
section was removed from `AGENTS.md`. The global Graphify agent skill under
`~/.claude/skills/graphify` was also removed earlier in the same cleanup. Historical mentions
in older status/task entries are unchanged archival text.

## DC-004 — device-control domain model (complete, planned runtime, 2026-09-02)

`src/device_control.rs` now exposes the transport-independent protocol-v1 domain layer: separate
device/command/message/connection/event/operation/delivery IDs; devices, entities, surfaces,
roles/scopes, capabilities/manifests, runtime/entity state, freshness/quality, presentations,
commands and command lifecycle values. It uses checked RFC3339 instants through the lightweight
`time` crate, with no wall-clock tests. Unknown valid namespaced capabilities round-trip with their
extra fields; unknown namespaced commands are preserved but explicitly return `unsupported_command`.
Known variants remain validated, as do manifest bounds/uniqueness, numeric/unit consistency,
deadline bounds, revision ordering and terminal results. No route, WebSocket, persistence,
migration, credential, provider, client or voice-contract behavior was added or changed. The next
device-control step is DC-005.

## API namespace consolidation (deployed to staging, 2026-09-02)

All first-party product, account, browser, device, administrator, search, and voice routes now use
`/api/v1/*` exclusively. The former non-namespaced compatibility registrations were removed rather
than redirected. Voice remains a single endpoint per transport: requests without `Authorization` use
anonymous limits, while a supplied Bearer is validated as a legacy deployment token or a paired
native-session token. RockCast, the administrator web client, OpenAPI, Caddy matchers, and active
documentation use the same namespace. External SpeechKit provider URLs are unchanged.

OPS-001-D deployed commit `75f0fc5` to `alex.vault57.ru` with `status=succeeded` and public
readiness `200`. `POST /api/v1/search` returned `200`; the removed `POST /v1/search` alias returned
`405` as expected.

## Codex workflow policy (2026-09-02)

RockServer Codex tasks must use only the primary checkout `C:\repos\rockserver` on
branch `master`. Separate Git worktrees for this repository are no longer allowed by
policy. The rule is recorded in `.cursor/rules/master-workflow.mdc` and
`docs/codex-project-context.md`.

## DC-000 — Windows-first readiness decision (GO, 2026-09-02)

The device-control protocol may start at DC-001.  A read-only audit of the current
`C:\repos\rockserver` and `C:\repos\rockcast` checkouts confirms that RockCast's
text-search path is operationally coherent: releases call anonymous `POST /api/v1/search`
at `https://alex.vault57.ru`, and a failed or empty service result preserves the embedded
catalog plus Radio Browser fallback and existing playback.  The account binding needed by
a future registered player is also present (`device_id`/device secret protected by DPAPI
and `POST /api/v1/auth/device-session`), but it is not a device-control connection.

Voice route selection is now stable: an anonymous RockCast session uses
`wss://.../api/v1/voice/stream`, while a paired PC renews its native access token through
`POST /api/v1/auth/device-session` and uses the same WebSocket path with Bearer authentication.
The voice routes accept either the legacy deployment Bearer or an active paired native-session
Bearer, resolving the prior token rejection. A transient token-renewal failure preserves the
durable binding and uses the anonymous mode. RockServer
provides bounded buffered SpeechKit v1 and selectable v3 recognizers when `YANDEX_AI_API_KEY` is
configured; absent configuration deliberately returns `speech_provider_unavailable`, leaving
text search and local playback usable.  A second STT provider and retry/circuit-breaker policy
remain future hardening, not a protocol blocker.

Current RockCast control entry points are documented for the later protocol adapter:
`app/actions/playback.rs` calls `PlaybackController::play`, `stop`, and `set_volume`;
`playback/mod.rs` owns generation/state events and delegates local output to `LocalPlayer`
and receiver output to `CastService`; `cast/client/mod.rs` owns discovery, CASTV2
play/stop/current-volume; and `relay/mod.rs` owns the LAN HTTP relay lifecycle selected by
the persisted `cast_relay` toggle.  No device-control client, capability manifest, or
remote command adapter exists in RockCast.

The removed `docs/windows-production-roadmap.md` and unperformed Windows E2E/failure
coverage are explicitly out of this decision.  The voice/authentication route and degraded
STT behaviour are now defined sufficiently for DC-001; no ESP32 firmware was started by
DC-000.

RockCast changed only at the existing voice/session client boundary.  RockServer now documents
and enforces native-session Bearer acceptance on the canonical voice command and stream routes;
no database schema or deployment changed.  RockCast verification passed: `cargo fmt --check`,
strict all-target/all-feature Clippy, and `cargo test` (97 unit tests plus non-live integration
coverage).  RockServer `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
passed; external provider and database cases are opt-in by design.

## DC-003 — device-control v1 golden fixtures (complete, planned runtime, 2026-09-02)

`tests/fixtures/device-control/v1/` is now the single canonical raw-JSON fixture set for
RockServer, RockCast, RockMobile and future firmware contract tests. It includes negotiation,
truthful RockCast and ESP32 registration/manifests, full/delta state, sensor telemetry,
controller directory projection, `sensor_grid`, command lifecycle success/failure and semantic
rejection flows. Home Assistant is represented only as source-neutral, allowlisted normalized
entity directory/state projections: it is neither registered as a paired device nor given a
provider-native ID, credential, WebSocket or service-call contract.

`tests/openapi_contract.rs` registers every JSON fixture, rejects unregistered/missing fixtures,
and validates each against its named OpenAPI component with dev-only `jsonschema` 0.30 (JSON Schema
2020-12 plus the OpenAPI component `$ref` graph). The test also checks message/command correlation,
one terminal lifecycle result, revision progression, explicit target, registration identity
exclusion and semantic error replies. The fixture review exposed a compatibility gap: an unknown
namespaced command was prohibited by the command union despite the v1 rule requiring a structured
`unsupported_command`. `UnknownCommandBody` now accepts only unknown namespaced command envelopes;
known command branches retain their strict payload schemas.

Focused fixture validation (4 tests), `cargo fmt --check`, strict all-target/all-feature Clippy,
and full `cargo test --jobs 1` pass. External PostgreSQL/provider tests remain ignored behind
their explicit gates; no database query changed. Runtime WebSocket/HTTP routes, persistence,
clients, firmware and a Home Assistant adapter remain unimplemented, and `/api/v1/voice/stream`
is unchanged. The next device-control step is DC-004.

## DC-002 — device-control protocol v1 contract (complete, planned runtime, 2026-09-02)

`api/openapi.yaml` now defines the contract-first, machine-marked planned operations
`GET /api/v1/devices/connect` and `GET /api/v1/device-control/directory`. Both accept only the
short-lived native-session `RockserverBearer`; the server derives `user_id`/`device_id`, and the
registration payload cannot assert identity or carry pairing/device secrets. Existing
`GET /api/v1/devices` remains the implemented account inventory/list-revoke contract, and the
existing voice WebSocket is unchanged.

Protocol major 1 defines a shared JSON envelope, negotiation/registration, truthful roles and
capabilities, bounded entity/surface manifests, heartbeat/presence, full/delta device state,
entity telemetry/freshness, directory snapshot/upsert/removal events, explicit-target typed
commands, received/accepted/one-terminal-result lifecycle and structured errors. Exact limits are
65,536-byte frames, 61,440-byte payloads, 32 capabilities, 256 entities, 16 surfaces, 50 directory
devices, 20-second heartbeat, 60-second offline TTL, 10-second registration, 16/8 in-flight
commands per connection/target, 10-second default and 30-second maximum command deadlines, and a
24-hour command-id idempotency window; stream URIs allow at most five fully revalidated redirects.
Reconnect requires full manifest/state resync and never
re-executes a retained duplicate command.

The focused OpenAPI contract test and full required verification pass: `cargo fmt --check`, strict
all-target/all-feature Clippy and `cargo test --jobs 1`. Contract checks cover parsing, planned
markers, native-only security, bounded constants, identity/secret exclusions, local references,
discriminator mappings and unique operation IDs. External PostgreSQL/provider tests remain ignored
behind their explicit gates; no database query changed. No WebSocket route, directory handler,
persistence, router, client, firmware or DC-003 fixture was implemented. The next device-control
step is DC-003.

## DC-001 — device-control identity and authorization model (complete, 2026-09-02)

The future device-control plane is now constrained to the existing account-owned model:
an active native session resolves to the existing `(user_id, devices.id)` boundary, while the
durable device secret only renews that short-lived session. Device roles are functional
capabilities, not permissions; a separate minimum policy vocabulary covers directory/presence
read, entity-state read, media, display and actuator control. Every future dispatch must verify
actor identity, same-owner target, scope, role/capability/schema, availability/freshness and
confirmation policy. Cross-user access, hidden broadcast and device-control access through admin
principals are denied by default.

The roadmap records the permission matrix for RockMobile, RockCast, ESP32, a future automation
principal and the Home Assistant adapter. Automation identity, shared homes/delegation and
fine-grained policy remain explicitly deferred; Home Assistant uses a separate least-privilege
integration credential rather than account-device pairing. Intent safety distinguishes fresh,
unambiguous reads; ordinary media/display control; clarification for ambiguous resolution; and
explicit confirmation for side-effecting/risky actuator operations. No Rust, migration, OpenAPI
or runtime behavior changed. This is the historical DC-001 decision; the current next
device-control step is DC-004.

Verification: targeted migration, auth, HTTP, persistence and OpenAPI inspection; documentation
consistency review; `git diff --check`; and searches for stale non-canonical device-control
non-canonical endpoint references and prohibited duplicate pairing terminology. As a Markdown-only
decision record, no build or test run was needed.

## RS-ADMIN-006 SPA administrator console (implemented locally, 2026-09-01)

The former Axum server-rendered `/admin` shell is superseded: Caddy now serves the existing
first-party Preact/Vite bundle at `/admin` in the same local and production topology, while
`/api/v1/admin/*` remains reverse-proxied to RockServer before the SPA fallback. The administrator
Bearer exists only in Preact state and is cleared on logout or `401`; reload requires login.
The read-only tabs are Devices (safe paired-device projection), Audit (request records plus security
events) and Stations (repository-side name/tag filtering, stable bounded pagination). No mutation,
credential lifecycle, token persistence, or new frontend dependency was added. Local Compose now
binds Caddy to loopback port 80 so the intended entry point is `http://localhost/admin`.

Verified serially: web typecheck/lint/test/build, `cargo fmt --check`, strict Clippy and `cargo test`
passed. A loopback-only disposable Docker Desktop PostgreSQL run passed the admin durable-query
integration test, including the unified audit query; the container was removed. `docker compose
config --quiet` accepted the merged local Caddy/RockServer topology. No Caddy image was locally
available, so Caddyfile validation and a browser `http://localhost/admin` smoke were not run.

## Local administrator launcher (2026-09-01)

`run-rockserver-admin-local.ps1` starts host `cargo run` for RockServer and a disposable Caddy
container that serves `web/dist` at `http://127.0.0.1:3080/admin` while proxying `/api/v1/*`,
`/api/v1/*`, and `/health/*` to the host listener. It sets `ROCKSERVER_LOCAL_ADMIN_ORIGIN` to the
Caddy loopback origin (default port `3080`, overridable with `-AdminUiPort`). It builds `web/dist`
when missing. `run-rockserver-local.ps1` remains the API-only launcher without SPA hosting.

## Local launcher alignment (2026-09-01)

`run-rockserver-local.ps1` now creates a non-logged process-local trusted-proxy credential when
the ignored local `.env` omits it, matching the production router's startup requirement. It enables
admin login only from its exact `http://127.0.0.1:<port>` loopback origin; arbitrary LAN origins
remain rejected and production keeps the approved HTTPS first-party proxy origin. Administrator
bootstrap remains an explicit protected command.

## RS-ADMIN-005 operational hardening (implemented locally, 2026-09-01)

Administrator Bearer refresh is now an atomic PostgreSQL replacement: the new opaque-token hash is
inserted and the prior session is revoked in one transaction, so an error leaves the existing
session usable and never creates a second active session. Authenticated admin routes now write a
bounded operational record containing only request ID, principal/session UUIDs, static endpoint,
safe outcome, duration and timestamp. Request bodies, query/search text, transcripts, headers,
passwords and Bearers are excluded. The write path removes records older than 30 days. The new
operator runbook documents backup/restore, incident revocation and deployment acceptance; a restore
must revoke admin sessions before reuse. RockCast product routes remain public and user identity
uses pairing; no RockCast machine-credential lifecycle was added. Request-log UI/read models and a
bulk revocation operator remain later explicitly scoped work.

Verified locally: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
(113 library tests plus all non-ignored target/contract suites) passed serially, as did the
OpenAPI contract suite and `git diff --check`. A disposable local Docker Desktop
`pgvector/pgvector:pg17` database then passed
`postgres_admin_auth_queries_are_durable_and_revocable`; it covered every new or changed
administrator persistence query, including atomic rotation and request-record retention. The
loopback-only disposable container was removed after the test.

## RS-ADMIN-004 administrator console (implemented locally, 2026-09-01)

`GET /admin` is now a first-party Axum server-rendered login shell with locally served CSS and
JavaScript. Successful RS-ADMIN-003 login hands its opaque administrator Bearer token only to a
JavaScript variable in the current page; the shell never serializes it and the helper does not use
cookies, storage, URL state, DOM insertion, logs, or third-party code. Logout and any `401` clear
the authenticated view and token. The protected, read-only `GET /api/v1/admin/stations` presentation
model is bounded to 50 records and reuses the current catalog read service without new SQL.
All `/admin` assets and `/api/v1/admin/*` responses carry `Cache-Control: no-store`, restrictive
same-origin CSP, nosniff/referrer/permissions protections; existing login/refresh/logout Origin
validation remains the state-change boundary. No import, credentials, logs, password changes, or
other operator controls were added. Verification results are recorded in the current task log;
the next scope is RS-ADMIN-005.

## RS-ADMIN-003 administrator sessions (implemented locally, 2026-09-01)

`/api/v1/admin/auth/login`, `/refresh`, `/logout`, and the deliberately minimal protected
`GET /api/v1/admin/session` now form a separate administrator boundary. Login validates the existing
Argon2id PHC password, issues a 15-minute opaque Bearer value whose SHA-256 hash alone is stored,
and records safe durable attempt/audit classes. Refresh replaces and revokes the prior session;
logout revokes it immediately. Login uses the durable username+trusted-source-IP correlation key
and locks after five failed attempts in fifteen minutes. State-changing admin requests require the
existing trusted first-party Origin/proxy boundary; forwarded identity is ignored without that proof.
Public `/api/v1/*`, legacy `/api/v1/*`, passkey/browser-session, and local `/admin` preview behavior
are unchanged. The disposable `pgvector/pg17` run passed the RS-ADMIN-003 durable-query test and
the RS-ADMIN-002 bootstrap concurrency test when each used the disposable database serially.
The pre-existing all-ignored suite is not safe to run concurrently against one database: four
unrelated tests collide on shared cleanup/fixtures. The next scope is RS-ADMIN-004.

## RS-ADMIN-002 protected administrator bootstrap (implemented locally, 2026-09-01)

The new `bootstrap_admin` one-shot binary accepts `ROCKSERVER_ADMIN_BOOTSTRAP_USERNAME` and
`ROCKSERVER_ADMIN_BOOTSTRAP_PASSWORD` only from its process environment. It does not load `.env`
files and accepts no secret command-line arguments or HTTP input. The password is validated,
hashed with Argon2id, and only its PHC representation reaches PostgreSQL. Migration `0019` adds
the bootstrap username metadata and a singleton index. One CTE creates the principal, credential,
and `admin_created` audit event atomically; an existing principal, including a concurrent winner,
returns a no-op and never replaces its credential. Config, hash and store errors use safe messages,
and secret-bearing config/hash debug output is redacted.

Verified locally: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test` (109
library tests plus regular integration suites), `git diff --check`, and `.env` ignore verification
passed. A temporary local loopback `pgvector/pgvector:pg17` PostgreSQL database was available for
the required disposable test; `postgres_admin_bootstrap_is_atomic_and_idempotent` passed and proved
that concurrent attempts produce exactly one principal, password credential, and creation event.
The temporary container was removed. No HTTP login/session middleware, UI, deployment, staging, or
external provider test was added. The next scope is RS-ADMIN-003: administrator login/session
authentication and abuse resistance.

## RS-ADMIN-001 durable administrator identity foundation (implemented locally, 2026-09-01)

Migration `0018` adds administrator-only principals, Argon2id password-hash credentials, opaque
token-hash sessions, hashed account/source-IP login attempts, and safe security events. The tables
are separate from `users`, passkeys, devices, browser/native sessions, and future RockCast
machine-client credentials. The new domain/persistence boundary accepts no raw password or token
material; it includes an in-memory deterministic fake and a PostgreSQL implementation. No routes,
middleware, bootstrap command, UI, OpenAPI behavior, staging data, or deployment were changed.
Verified locally: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
passed. The full ordinary suite includes 106 library tests. A disposable PostgreSQL 17 database was
available through local Docker; `postgres_admin_identity_foundation_rejects_invalid_hash_state`
passed against it, exercising every new administrator persistence query and its password/token hash
constraints. No staging, deployment, or external-network test ran. The next implementation task is
RS-ADMIN-002.
## Codex cross-project context (documentation, 2026-09-01)

`docs/codex-project-context.md` is the canonical persistent starting context for
future bounded Codex work across RockServer, RockCast and RockMobile. It records
the actual current boundaries, the approved admin Bearer-session decision, and
the station-icon delivery order. It explicitly distinguishes implemented
behaviour from the icon roadmap: the icon endpoint and public `faviconUrl` are
not yet live. Future Codex tasks use the current master checkout by default,
without creating a worktree. No Rust, API, migration, deployment, staging data
or client behaviour changed.

The RS-ADMIN-005 backlog does not include obsolete RockCast machine credentials:
current RockCast product routes are public and user identity is handled through
pairing.

## Station icons (deferred, 2026-09-01)

The approved future design remains in `docs/roadmap/station-icons.md` and the
ordered `RS-ICON-*` backlog in `docs/codex-project-context.md`. The owner has
deferred implementation; no icon task may begin until a new explicit request.
No icon endpoint, server-owned `faviconUrl`, storage, downloader or sync job is
implemented by this decision.

## Documentation cleanup (2026-09-01)

The active `docs/` root now contains only current context, architecture, active
roadmaps/runbooks, release documentation and the historical status/task logs.
Superseded MVP/RM-004/shared-roadmap proposal artifacts were removed; historical
reports, diagnostics and explanatory diagrams were moved to
`docs/archive/2026-08/` with an archive README. Remaining links were updated.
No code, API, migration, deployment, staging data or client behaviour changed.

## RM-011 durable device-secret sessions (implemented locally, 2026-08-30)

Native pairing is now a durable device binding rather than a refresh-token family. Pairing returns
one 256-bit opaque `device_secret`; PostgreSQL stores only its hash. `POST /api/v1/auth/device-session`
accepts the device UUID and secret, retires the previous short-lived access session, and returns a
new 15-minute access token. Only a revoked, unknown, or deleted-account credential returns
`401 device_credential_invalid`; transient store failures return `503` and never revoke a device.
The old refresh and native-logout routes are absent. Migration `0017` drops refresh-token state and
resets existing native device/session rows, so staging clients pair once under the new model.
Verified locally: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
passed (103 unit tests; PostgreSQL and live-credential suites remain opt-in/ignored).
Staging deployment completed through OPS-001-D as commit `7916cee`; the remote worker reported
`status=succeeded` and public readiness passed. The first rollout of `74589c7` exposed a migration
ordering error before readiness; `7916cee` corrects it by dropping the dependent refresh table
before removing sessions.

## RM-011 — fresh-passkey pairing approval repair (verified locally, 2026-08-30)

The account-centre availability regression reported during the physical flow is traced to literal
backslashes emitted in six multiline PostgreSQL browser-session/device queries. PostgreSQL rejected
them with SQLSTATE `42601`, while the HTTP boundary correctly exposed only the safe
`auth_unavailable` response. The query continuations now emit whitespace instead; the existing
disposable PostgreSQL browser-account ownership regression passes after the fix. Staging deployment
completed through OPS-001-D as commit `c36e652`; the remote worker reported `status=succeeded`
and public readiness returned `200`. Renewed physical pairing remains pending.

The live browser evidence identified a second Web state defect: a restored browser cookie was shown
as ready to approve a pairing even though the server correctly requires a passkey assertion no
older than two minutes. The server returned `409 pairing_not_approvable`, which the UI rendered as
an expired/used request. The Web client now treats a restored cookie as account context only and
requires an explicit fresh passkey before it renders the approval action. It retains terminal,
explicitly authenticated, and in-flight approval states against late session restoration.

Verified locally: Web regression tests 11/11, typecheck/lint/build, `cargo fmt --check`, strict
all-target/all-feature Clippy, and `cargo test` (103 tests; the existing disposable PostgreSQL and
credential/live tests remain intentionally ignored) passed. Commit `82382b1` was deployed through
OPS-001-D; the detached worker reported `status=succeeded`, public readiness returned `200`, and
the published bundle contains the fresh-passkey guard and reauthentication control. Renewed
physical completion remains pending; no new E2E success is claimed by this entry.

The pairing page also now explains the distinct recovery boundary when a browser cookie names an
account but its passkey was deleted: the cookie cannot approve a new device, the deleted key cannot
be restored, and the user may create a separate Rock account for the current pairing. It neither
alters nor merges the original account.

Commit `a247ffc` was verified with the same 11/11 Web regressions, typecheck/lint/build, Rust
formatting, strict Clippy, and 103 Rust tests. OPS-001-D deployed it with `status=succeeded`;
public readiness returned `200` and the published bundle contains the distinct-account recovery
control. A user-held passkey ceremony is still required for a new account, so no completion E2E is
claimed by this verification.

## RM-011 — live E2E regression found; web race fixed locally (2026-08-30)

The first permitted disposable staging attempt exposed a real regression: an initial browser-session
request could complete after pairing approval and overwrite the `approved` screen with the pending
confirmation screen. `web/src/app.tsx` now preserves `approved` against that late response, with a
new regression assertion. Commit `82521f7` was deployed through OPS-001-D; the detached worker
reported `status=succeeded`, and public readiness returned `200`. Web `pnpm test` passes 11/11 and
`pnpm build` passes locally.

The live E2E is **not passed**: the browser account centre also showed its recoverable unavailable
state, and neither native client has yet produced verified persisted-session and device-list
evidence. The investigation is continuing without collecting account, session, credential, proof,
QR, or device identifiers. The correct public release certificate SHA-256 is
`92:E7:BE:49:13:A9:4A:4B:50:E3:75:87:BA:BA:13:F3:60:D4:94:7C:48:30:CD:FD:E1:04:02:13:E6:34:25:39`.

On 2026-08-30, staging containers, PostgreSQL readiness and all account migrations were verified
healthy, and anonymous probes through Caddy returned the expected `401 authentication_required`
for both browser account endpoints. A clean disposable Android emulator running the official
RockMobile release created a pairing request and reached its confirmation state without an
availability error. RockMobile `0.1.3`/`versionCode=4` now removes stale endpoint overrides and
reports its actual build version; its signed APK was verified against the public App Link
fingerprint. RockCast release started locally and its 93 tests passed. A passkey-approved native
completion and device list still need an available authenticator; no E2E pass is claimed.

## RM-011 — final integration (staging deploy complete; E2E blocked, 2026-08-29)

RockServer commit `e171096e55ce1b5912fb76615c584775313a8fb9` now publishes the Android
Digital Asset Links statement at `/.well-known/assetlinks.json`. The asset has one target only:
`com.rockmobile`, relation `delegate_permission/common.handle_all_urls`, and the SHA-256
fingerprint `92:E7:BE:49:13:A9:4A:4B:50:E3:75:87:BA:BA:13:F3:60:D4:94:7C:48:30:CD:FD:E1:04:02:13:E6:34:25:39`.
It was obtained by verifying the authorized local signed release APK with `apksigner`; no private
signing material was read or recorded. Vite copies this checked-in public asset into `web/dist`,
and the existing Caddy file server owns its deployment.

The documented `deploy/ops-001-d.ps1` workflow completed for that immutable commit. Its detached
remote worker reported `status=succeeded`; unauthenticated public checks then returned `200` for
`/health/ready` and `200 application/json` for the exact assetlinks target above. No destructive DB
operation, push, account/device/session mutation, QR payload, credential or proof inspection was
performed. The source contract remains `?code=<code>#secret=<proof>`; the web client immediately
scrubs a fragment or permitted legacy query secret, and legacy query support ends
2026-09-29T00:00:00Z. RockMobile accepts only the no-query/no-fragment HTTPS App Link
`/return/rockmobile`.

Current source identities are RockServer `e171096`, RockMobile `3b0c47e` (version `0.1.1`,
`versionCode=2`) and RockCast `00f6d1e` (crate `0.1.0`). Completion compatibility is the current
unversioned `202 pairing_pending` contract; no `pairing_protocol_version` was found. Static source
and regression evidence cover secret scrubbing/no browser console output, exact client handoff
shape, one-prefix presentation and narrow Android return handling. The fresh server checks passed:
web `pnpm test` (10/10) and `pnpm build`; `cargo fmt --check`; strict all-target/all-feature
Clippy; and `cargo test` (103 library tests; disposable PostgreSQL and credential/live tests remain
intentionally ignored).

Disposable staging E2E was not run: this host has neither `adb`/a connected Android device nor a
running RockCast process. Passkey, physical-device, QR-camera, installed-build and browser-return
outcomes therefore remain unverified; no security boundary was bypassed. The integration audit also
found one user-owned dirty detached RockMobile worktree (its `AndroidManifest.xml` and
`build.gradle.kts`) and a stale Wave 2 RESULT implementation hash (`a1d7acb` is not an ancestor of
RockMobile master; the current equivalent master change is `98b7e09`). Those items were preserved,
not changed. Full evidence and all RM-011 commit/report references are in `RM-011-FINAL-RESULT.md`.

## RM-011-09 — Wave 9 A4 secure pairing handoff (local, 2026-08-29)

The web pairing handoff now takes the approval secret from `#secret` while retaining the short
code in `?code`. It parses that fragment only in memory and replaces the current history entry
immediately with a URL that has no fragment or `secret` query parameter, before pairing lookup or
other effects run. The parsed handoff is retained only in the first render's memory, so later
rerenders cannot recover it from the cleaned URL. The former query-secret shape is accepted only
until 2026-09-29T00:00:00Z and
is removed from the address bar in the same way. The secret is not rendered, logged, persisted or
sent in a navigation/referrer; approval continues to submit it only in the existing same-origin
request body.

No API/OpenAPI, server lifecycle, deployment, staging data or account/device state changed. The
repository has no `assetlinks.json`/`.well-known` asset: production Caddy serves only the web build
from `/srv`. Therefore production Android App Link verification remains an external release
blocker: the operator must publish the domain-owned `/.well-known/assetlinks.json` with package
`com.rockmobile` and the SHA-256 certificate fingerprint of the private release signer. No such
signer fingerprint was read; `keystore.properties` is deliberately untracked.

Verified locally: web `pnpm test` (10/10) and `pnpm build`; `cargo fmt --check`; strict
`cargo clippy --all-targets --all-features -- -D warnings`; and `cargo test` (103 library tests,
with six disposable PostgreSQL and six credential/live tests intentionally ignored). No push,
deploy, staging mutation, browser ceremony, account/device operation or production App Link claim
was made.

## RM-011-08 — authenticated web cabinet UX (local, 2026-08-29)

RM-011 web A8–A12 is complete locally. The existing Preact cabinet now has exclusive loading,
anonymous, authenticated, browser-session-expired, and service-unavailable views, so it does not
flash anonymous content while session recovery is pending and removes stale device data after a
confirmed authentication failure. A retry remains available for service failures.

The authenticated cabinet identifies the browser session separately from native RockMobile and
RockCast devices; shows the current `N из 10` count, distinct empty and limit guidance, human
readable activity, rename/revoke confirmations, account-data refresh and inline success. Device
and logout operations have independent busy states. Device display names receive at most one
product prefix, and the UI renders no device/user/session identifiers, tokens, or proofs.

Pairing success can open this cabinet in the same Preact instance and marks the matching card
`Только что подключено` only in memory. It neither persists the marker nor re-enables approval.
The layout retains the existing dependency-free Preact/CSS approach, adds visible keyboard focus,
semantic alert/status messaging, labelled phrase/code text, and mobile full-width actions.

Local verification passed: `cd web && pnpm test` (9/9), `cd web && pnpm build`, `cargo fmt
--check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` (103
library tests plus all available target suites). Six disposable PostgreSQL, four billable Yandex
LLM, one SpeechKit, and one ONNX live test remain intentionally ignored. No staging, deploy,
push, account/device operation, API/OpenAPI, A4 URL-contract, RockMobile, or RockCast change was
performed.

## RM-011-07 — anonymous landing and separate registration (local, 2026-08-29)

RM-011 web A6+A7 is complete locally. Anonymous `/` now makes the existing-account passkey
sign-in primary and separate Rock-account registration secondary, with a short explanation of the
difference and no manual pairing input. The existing Preact root uses `location.pathname` and
`location.search` for `/register` and pairing-context return; no router, UI, or state-machine
dependency was added.

Registration has a dedicated labelled form, explicit default-name selection, click-only WebAuthn
ceremony, disabled busy controls, and a ref guard against double submit. Cancelling a passkey
keeps the typed name and shows recovery text without creating an account. Verified pairing-context
registration returns only to that pairing confirmation; standalone `/register` shows signed-in
success and leads to the account centre. Pairing URL/proof handling and the A4 contract were not
changed.

Local verification passed: web regression tests (11/11), typecheck/lint and Vite production build;
`cargo fmt --check`; strict `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test`
(103 library tests and all available target suites). Six disposable PostgreSQL and six live external
tests remained intentionally ignored. `git diff --check` passed. No staging access, deployment,
push, account/device data operation, OpenAPI/server change, RockMobile, or RockCast change was
performed. Implementation commit: `d26fde0c170fd1db5524d3fcf8929d8de4441d40`.

## RM-011-G7 — cross-repository account/pairing E2E verification (2026-08-28)

The current RockServer, RockCast, and RockMobile checkouts were reviewed at their recorded
`master` revisions. Server G1/G2, browser pairing context, RockCast G4, and RockMobile G5 expose
the intended passkey-only, named-device flow without normal UUID or manual bearer-token entry.
RockServer now distinguishes pending pairing (`202`) from invalid (`401`), device-limit (`409`),
and expired/consumed/revoked (`410`) completion states. New-account registration creates no user
until a successful passkey assertion and commits challenge, account, passkey, and browser session
atomically. Both native clients cancel pending pairing on UI dismissal and no longer retry terminal
responses.

Local verification passes for RockServer formatting, strict Clippy, all-target/all-feature tests,
OpenAPI contract tests, web typecheck/regression/build, RockCast formatting/check/unit tests/strict
Clippy, and RockMobile unit tests/lint/debug assemble. Six disposable PostgreSQL account/pairing
tests remain ignored without `TEST_DATABASE_URL`.

RM-011-G7 is not accepted yet: real passkey/WebAuthn, Windows GUI, physical Android, staging, and
live account/device-centre checks were intentionally not run. The remaining work is the separately
authorized disposable-DB and physical-device smoke checklist in
`docs/rm-011-g7-e2e-verification.md`.

The deploy hardening follow-up preserves the requested on-VPS backup retention behavior: after a
new dump is copied and checksummed, older `rockserver-*.dump` files are removed while the new dump
is retained; any earlier failure leaves the previous dump untouched. The launcher also records the
portable image config ID from the exact saved archive, and the remote wrapper retains compatibility
with the former deploy argument shape. The deploy-script suite passes locally. Commits `392d8b4`
and `26906ef` reached `origin/master`; OPS-001-D reported `status=succeeded` and readiness passed
for image `sha256:bf80ed529b0a7c1ffc14c2333730851fdaddfadf401a90a1c03a6052e190cd3f`.

Follow-up verification found that the previous rollout did not activate the uploaded root-side
operator script, so staging retained older backup files. Commit `27beb6c` now validates and installs
the bundle operator script after acquiring the deploy lock and before starting the detached worker.
OPS-001-D local tests pass; the fix was deployed successfully and staging now has exactly one
`rockserver-*.dump` rollback backup.

## RM-011-G6 — preview-first staging account cleanup (local, 2026-08-28)

The staging-only `account_cleanup` operator binary and the root-scoped remote wrapper are
implemented. Preview is the default and enumerates account, passkey, identity, device, native
session, refresh-token, browser-session, pairing, WebAuthn-challenge, and safe audit dependencies
without names, credential material, hashes, tokens, or arbitrary audit details. Apply accepts one
exact UUID and action-specific confirmation phrase; account deactivation retains the tombstone and
revokes every account-owned access dependency, device revoke cascades native sessions and refresh
tokens, credential revoke retains the last active passkey, and active admin identities protect an
account. No account ownership is inferred from display names or age, and browser/Google Password
Manager passkeys remain a manual owner action. The account center and read-only admin preview now
state this boundary explicitly.

Local verification passed: `cargo fmt --check`, strict all-target/all-feature Clippy, and
`cargo test --all-targets --all-features --jobs 1` (103 library tests, 4 cleanup CLI tests, 6
deployment/security tests, 2 OpenAPI tests, 9 search API tests, 5 voice-command tests, and 1
voice-stream test passed; 6 disposable PostgreSQL, 1 ONNX, 4 billable Yandex LLM, and 1
SpeechKit integration tests remain ignored). Web `typecheck`, `lint`, `test` (5 tests), and
`build` also passed, as did `git diff --check`. This historical local verification preceded the
later deploy commits; it did not perform staging cleanup or production operation.

## SECURITY-001 — Rust TLS dependency remediation (deployed, 2026-08-28)

GitHub Dependabot reported one high and two low advisories for `rustls-webpki 0.101.7`,
reachable through `yandex-cloud 2025.4.14 → tonic 0.9`. The published Yandex crate has no
compatible release that removes this chain, so the unused parts of its generated SpeechKit API
were not vendored. RockServer now owns a small, checked-in SpeechKit v3 protocol boundary for
the existing bidirectional `Recognizer/RecognizeStreaming` RPC and uses `tonic 0.14.6`,
`tonic-prost 0.14.6`, and `rustls 0.23.43` with native roots and ring TLS support.

The streaming endpoint, API-key metadata, PCM16 mono configuration, explicit EOU event, partial
and final transcript handling, endpoint validation, and all HTTP/security behavior are unchanged.
Wire-tag regression tests cover the SpeechKit audio request and partial response. `cargo tree`
contains no `rustls-webpki 0.101.7`, `tonic 0.9`, or `yandex-cloud`; the remaining
`rustls-webpki` is `0.103.14` through the maintained `rustls 0.23` graph. Local `cargo fmt
--check`, strict all-target/all-feature Clippy, locked compilation, full all-target/all-feature
tests (102 passed; five disposable PostgreSQL, four billable Yandex LLM, one SpeechKit, and one
ONNX test remain explicitly ignored) and `git diff --check` passed. Commit
`266b9740ae2f0489952f46a15e80a18791d09a7e` was pushed to `origin/master` and deployed through
the OPS-001-D rollout. The remote worker reported `status=succeeded`; the immutable server image
`sha256:a3a9fcad59fd6ef464261d29dd5409aa411f3ee7e9add5c65689078d8adddd5b` passed the public
readiness gate.

## REFACTOR-001 — HTTP transport module extraction (deployed, 2026-08-28)

The behavior-preserving HTTP extraction is complete for the first slice. The former 3,704-line
`src/http/endpoints.rs` is now a 392-line composition root. Route handlers and their local
transport types live in `auth.rs`, `account.rs`, `pairing.rs`, `catalog.rs`, `search.rs`, and
`voice.rs`; shared request/response and trust-boundary helpers live in `transport.rs`, admission
state remains in `state.rs`, and liveness/admin behavior lives in `health.rs`.

All existing routes, OpenAPI files, migrations, serialization, authentication, CSRF, proxy trust,
pairing ownership, audit classifications, rate limits, and voice limits are unchanged. Security
and DTO regressions now sit beside their transport boundaries. Verified locally with formatting,
strict Clippy, full Rust tests (100 passed; five disposable PostgreSQL tests remain opt-in without
`TEST_DATABASE_URL`), OpenAPI contract tests, and `git diff --check`. No web files were changed.

Commit `2afcae0` was pushed to `origin/master` and deployed through the OPS-001-D staging
rollout. The immutable server image was accepted by the remote worker and the public readiness
gate passed.

The next reasonable extractions are `src/persistence/account_postgres.rs` (1,150 lines),
`src/search/mod.rs` (1,103 lines), and the PostgreSQL integration test module (1,184 lines); they
are intentionally left for separate, independently verifiable tasks.

## HTTP transport maintainability (2026-08-28)

Added the always-applied project rule `.cursor/rules/http-module-boundaries.mdc`: new HTTP work
must enter a domain transport module, keep router composition thin, and include a post-change
review for misplaced responsibilities and dead code. The existing `src/http/endpoints.rs` remains
a legacy mixed transport module and needs a dedicated, behavior-preserving extraction of account,
pairing, catalog, and voice domains before further broad endpoint work.

## RM-011-G3 — browser account and device centre (local, 2026-08-28)

The authenticated first-party browser view now presents one named Rock account and its active
RockCast/RockMobile devices without rendering UUIDs, credentials, tokens, or audit data. It shows
the human device name/type, connection time, most recent activity when available, an aggregate
active/inactive native-session status, and the 10-device limit with an explanation for freeing a
slot. Anonymous landing remains a sign-in/pairing entry point rather than an account centre.

`GET /api/v1/browser/account` is cookie/proxy-bound and returns only the safe browser projection.
Rename, revoke, and browser logout use separate first-party trusted-proxy, Origin and CSRF checks;
the server derives ownership from the cookie, writes only safe audit classifications, and owner
checks every target device. Browser logout revokes only its cookie session. Device revoke revokes
the target's native sessions/refresh tokens and explicitly cannot terminate the current browser
session. Existing native refresh and revoke semantics are unchanged.

Verified locally: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
(100 tests passed; five disposable PostgreSQL tests stayed opt-in without `TEST_DATABASE_URL`).
The checked-in web dependencies also passed TypeScript typecheck/lint, Vite production build, and
five deterministic UX regressions. Remaining acceptance is the requested staging/physical-phone
smoke test; no staging deployment, account creation, credential ceremony, commit, or push was run.

## RM-011-G2 — browser account and pairing UX (deployed to staging, 2026-08-28)

The ordinary page is now a safe account landing page: it states whether the browser is signed in,
offers username-less passkey sign-in when anonymous, and has no general pairing-code or device
flow. A secure link carrying the existing opaque code and approval secret opens only that pending
request. It displays the human device name/product, verification phrase, short code and expiry;
it never renders request IDs, account UUIDs, credential IDs or native tokens.

Anonymous pairing makes the two choices explicit: sign in with an existing passkey, or create a
new Rock account with an unambiguous warning. The current URL remains unchanged through both
ceremonies, so the original request survives login/registration and reloads. An authenticated
browser sees the exact confirmation question for its account, with connect and cancel actions.
`POST /api/v1/auth/browser-session` rotates a tab-local CSRF value for a live cookie session; it is
first-party/proxy-bound, no-store, returns only the display name and CSRF value, and lets the
specific pairing screen recover safely after reload without browser token storage.

User-facing statuses cover missing key, cancelled prompt, expired/already-completed request,
already-connected device, and server unavailability without HTTP details. Existing approval owner
derivation, CSRF/cookie, RP/origin, trusted Caddy proxy, and native-token boundaries are unchanged.
Local checks passed: Rust format, strict all-target/all-feature Clippy, `cargo test` (99 regular
tests; four disposable PostgreSQL tests remain ignored without `TEST_DATABASE_URL`), web source
regressions, typecheck/lint and production build. A clean browser automation session confirmed no
UUID/code field on the landing page, then used a local deterministic API harness (no account,
credential or staging request) to verify both the anonymous target-device screen and the signed-in
named account confirmation. Staging deploy of `efc405a` completed through the standard detached
worker with readiness passed; a fresh read-only browser session shows the new anonymous landing
without a UUID or general pairing-code field.

## RM-011-F P0 blocker fix — username-less discoverable passkey login (deployed, 2026-08-27)

The browser login flow now starts WebAuthn authentication without an account UUID and sends an
empty `allowCredentials` list, so the browser/OS can offer a saved discoverable passkey for the
fixed `alex.vault57.ru` RP ID. The server requires the assertion's `userHandle`, decodes it as the
16-byte account UUID used during registration, checks any legacy challenge owner binding, and
loads the credential only within that derived owner. The client-supplied `user_id` field is no
longer accepted by authentication verify, and no account identifier is rendered by the UI.

The existing registration, Secure/HttpOnly/SameSite browser cookie, CSRF, trusted-Caddy proxy,
WebAuthn origin/RP checks, rate limits, audit/device policy, and pairing completion contract are
unchanged. Native tokens remain outside the browser flow. Missing/invalid handles, unknown or
cross-account credentials, and invalid assertions receive the same neutral rejection; the UI
reports a missing-or-cancelled passkey, user cancellation where the browser distinguishes it, or
a generic server error without exposing internal details.

Local verification passed: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test`
(97 regular tests; four disposable-PostgreSQL tests remain ignored), and web `typecheck`, `lint`,
and production `build`. The standard deploy completed successfully for commit
`7b4f306635e95297ac3cd8b2d99063500d90bc0f`; the deploy worker reported image
`sha256:531f1d5051260a3e008f36c96c2dfd3ce84bcc8158d8ccc5c7d09fa7f7f63de8`, two Yandex keys,
and `readiness=passed`. A read-only staging browser inspection shows no UUID field and the
discoverable-login buttons; no staging account was created and no passkey assertion was made.
The remaining acceptance item is a manual physical-device passkey smoke test.

## RM-011-F — staging security acceptance (in progress, 2026-08-27)

Local acceptance has verified the RockServer, RockCast, and RockMobile security boundaries and
their relevant test/build checks. During review, the QR approval URI was found to be protected by
the weaker Caddy `Referrer-Policy: same-origin` header. Both Caddy configurations now use
`no-referrer`, preventing the short-lived approval secret in the QR URI from being sent as a
same-origin request referrer; `tests/deploy_security.rs` locks this down and both configurations
validate in Caddy 2.10 with placeholder-only environment values.

The approved staging deploy script refuses to build from this intentionally uncommitted fix, so
deployment of the secure configuration is pending an approved immutable commit. Safe staging
checks on 2026-08-27 returned HTTPS readiness `200` and refused direct TCP port `3000`, but `/`
and the two negative RM-011 route probes returned `404`; the live site therefore predates the
RM-011 auth router and could not be accepted as this release before the corrected immutable
commit was deployed.

The first immutable RM-011-F deploy attempt uploaded its verified server image and Caddyfile but
stopped before backup/migration because the protected VPS `release.env` was missing the required
Caddy-to-server proof variable. Bootstrap now creates that root-only random value just as it
already does for the database password and legacy API credential; it is neither printed nor copied
through the desktop environment. A second bootstrap is required to install that operator-script
fix and provision the value before retrying deploy.

The next deploy diagnosis found that the existing VPS Caddy container was the upstream base image,
while the script neither transferred the built web bundle nor supplied the required immutable
`ROCKSERVER_CADDY_IMAGE`. The rollout now builds, labels, checksums, transfers, and verifies a
Caddy image tagged with the same source commit as RockServer. Bootstrap must install this updated
root-side deploy script before the final retry.

The first clean Caddy image build also exposed an unfinished pnpm allowlist value for `esbuild`.
It is now explicitly `true`, allowing only that already locked build dependency in the container;
the lockfile and dependency set are unchanged.

The Caddy Docker build also now sets `CI=true` only for its pnpm build command, so its clean,
non-interactive image build cannot wait for a modules-directory prompt after source copy.

Post-deploy probing found that Caddy's SPA `try_files` rewrite was taking precedence over the
API matcher, returning the static page for `/health/*` and `/api/v1/*`. Both Caddyfiles now use an
exclusive API `handle` before the static fallback; the regression test requires that order.

Final staging deployment `f960e323a2b9e06e0281bf144bb368dc244e54c9` succeeded through its
catalog and readiness gates. HTTPS `/health/ready` returns JSON `200`; the public Caddy response
sets CSP, `Referrer-Policy: no-referrer`, `nosniff`, `DENY`, and Permissions-Policy headers; and
direct public TCP port `3000` is unreachable. Browser passkey options without first-party Origin
return `403`; malformed native completion returns `422`. One disposable staging pairing request
returned `201` with the expected in-memory-only proof shape, while completion carrying a forged
`user_id` was rejected with `422`. RM-011-F is now blocked only on physical phone passkey/Keystore
evidence; no secret, pairing proof, code, URI, ID, or protected log was printed.

## RM-011-B2 — browser/pairing persistence implementation (2026-08-26)

Владелец разблокировал проектирование и реализацию `RM-011-B2`, приняв рекомендуемые значения:
WebAuthn RP ID и first-party origin — `alex.vault57.ru`; синхронизированные passkey разрешены;
автоматического восстановления после потери всех passkey нет; максимум 10 устройств на аккаунт;
audit retention — 90 дней. Также приняты рекомендованные сроки access/refresh/browser/pairing
сессий, лимиты короткого кода, несколько passkey на аккаунт и clone/sign-count policy.
Единственный trusted proxy — Caddy; прямые подключения fail-closed; состояние rate limits — в
PostgreSQL.

Реализованы migration `0013`, browser sessions, pairing requests, одноразовые WebAuthn challenges,
атомарные PostgreSQL rate-limit buckets, passkey sign-count advancement и транзакционное
approve/complete pairing с лимитом 10 устройств. Добавлены нейтральные WebAuthn context/sign-count
проверки и детерминированные unit/integration tests. Полный cryptographic WebAuthn verifier и HTTP
routes остаются частью RM-011-C; B2 не меняет публичный runtime API.

Проверка: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
`cargo test` (88 unit/library tests и обычные integration suites) и отдельный disposable
PostgreSQL B2 integration test прошли. Исходный compose PostgreSQL был остановлен после проверки;
его persistent volume не изменялся.

## RM-011-B — approved persistence subset (2026-08-26)

На момент выполнения B1 были реализованы только persistence boundaries, не выбирающие policy: a
passkey-only user schema, reserved but unused account-identity boundary, owner-scoped devices,
hashed native access/refresh-token records, atomic refresh rotation/replay family revocation, and
safe-classification audit events. Raw credentials, passwords, email/phone identities,
cryptographic WebAuthn verification, HTTP routes и client UI оставались вне B1; browser/pairing
и rate-limit persistence добавлены последующей B2. Migration `0012_add_account_session_persistence.sql` contains no plaintext
secret columns; the Rust store accepts only opaque fixed-size secret hashes.

Deterministic unit coverage verifies hash-debug redaction and the audit vocabulary. An opt-in
PostgreSQL integration test covers duplicate users, ownership isolation, refresh rotate/replay,
and account deletion revocation when `TEST_DATABASE_URL` names a disposable database. Its live run
remains pending that explicitly provided database; B2 has its own disposable PostgreSQL verification.

## RM-011-A — account/device contract and threat model (2026-08-26)

The approval-only design record is archived at
`docs/archive/2026-08/rm-011-a-auth-device-contract.md`. It defined passkey/WebAuthn mobile-browser approval of a
desktop QR/short-code pairing request, opaque short-lived access bearer and rotating refresh-token
families, account deletion, and owner-scoped device list/revoke. Email, password, SMS and required
RockMobile installation are deliberately absent.
MVP-001 anonymous `/v1` endpoints remain separate: no shared token, credential or changed behavior
is proposed for them. The current runtime `api/openapi.yaml`, Rust code, database migrations,
clients, deployment, secrets and external services were not changed.

The entry records owner confirmation of MVP-001 and RM-007-D as supplied task context, and records
that OPS-001 staging exists; it does not overwrite the historical RM-007-D technical evidence.
RM-011-A contract/threat model was **approved by the owner on 2026-08-26**. The owner subsequently
selected the RM-011-B2 values recorded above. Trusted-proxy CIDRs, deployment wiring and the live
PostgreSQL test environment remain operational prerequisites and must be configured fail-closed;
RM-011-C must wait for B2 review and runtime OpenAPI reconciliation.

The proposal separately records phone-first browser registration: a passkey may create an account
before any desktop/device exists. It reserves an additive `AccountIdentity` boundary so a later
explicit login page or optional verified email/phone/password method can attach to the same user
without changing user/device/session ownership; none of those methods is currently proposed.

## MVP-001-A — public API contract and abuse model (2026-08-26)

The original approval-only MVP-001 proposal documents were removed from active documentation
during the 2026-09-01 cleanup. They specified the exact five-endpoint anonymous `/v1` allowlist for bounded catalog,
search and voice flows; account, device, sync, admin, operations, health and legacy routes remain
protected or infrastructure-only. The proposal explicitly bans shared Bearer tokens and client
secrets for public users, and fixes draft rate/audio/session/concurrency limits, error semantics,
aggregate metrics and redacted logging. Existing `api/openapi.yaml`, router/runtime behavior,
RockCast and RockMobile were not changed; current protected routes remain protected until the
separately reviewed MVP-001-B implementation.

Владелец одобрил proposal 2026-08-26: в MVP входят paged public catalog, text и streaming voice;
draft quotas являются стартовыми и требуют отдельного review для увеличения. До публичного rollout
proxy CIDRs, rate-limit store и retention keyed-IP hashes должны быть заданы в deployment
configuration; в implementation до этого действует fail-closed direct-peer policy. Existing
`/api/v1` aliases остаются protected. MVP-001-B разрешена к реализации в утверждённых границах.
Local checks are recorded in `docs/tasks.md`.

## Station icons — шаг 0: контракт (2026-08-26)

Документационный шаг 0 завершён в `docs/roadmap/station-icons.md`. В нём явно как
**планируемое**, а не текущее поведение, зафиксированы RockServer-first URL
`GET /api/v1/stations/{id}/icon`, nullable `faviconUrl`, внутренний `source_url`, приоритет
источников, формат/лимиты, `200`/`304`/`404`, cache policy и rollout/rollback. Проверка текущего
`api/openapi.yaml` подтвердила, что endpoint и поле ещё не входят в действующий HTTP-контракт.
Маршруты, Rust-код, SQL migrations, OpenAPI и service diagram не менялись; текущий сервис не
получил выдачу или загрузку station icons.
Проверки документации и `git diff --check` (включая no-index check нового roadmap-файла) прошли.

## OPS-001-D automated VPS bootstrap and staging rollout (2026-08-25)

`deploy/ops-001-d.ps1` now provides a registry-free owner-facing bootstrap and staging launcher.
The ignored inventory contains only SSH user/host/domain; no password field exists. First access is
an explicit OpenSSH password prompt used only to install an ignored generated key, and bootstrap
allocates a TTY for a possible one-time interactive `sudo` prompt. It installs a command-scoped
non-interactive deploy rule; normal updates use `sudo -n` and fail with a repair instruction when
that privilege is unavailable.

One subsequent `-Action deploy` command requires a clean worktree, resolves current `HEAD`, builds
`rockserver:sha-<full SHA>` locally, verifies its OCI revision label and immutable image ID, transfers
a checksummed `docker save` artifact, and revalidates ID/label after remote `docker load`. Staging
does not require GitHub, GHCR, another registry, remote Git pull, or mutable `latest`. Protected
runtime env updates are newline-correct and preserve generated DB/API secrets; only the four
documented Yandex names can transfer from ignored root `.env`. Backup precedes the importer, whose
embedded migrations and checksum-pinned idempotent full-catalog activation precede HTTPS readiness.
ONNX staging assets are fetched automatically from the committed checksum-pinned lock; focused local tests
and dry-run, production Compose rendering, PowerShell parsing, Rust fmt/strict Clippy/full tests,
and diff checks passed. The final all-features Docker build downloaded dependencies and reached Rust
compilation but was stopped at the owner's request before completion; it is not recorded as passed.
No VPS, DNS, registry, credential, ONNX download, or public readiness operation was run.

## RM-007 local personal-data contract

### RM-007-A common model and station-ID migration (2026-08-25)

The proposed cross-client contract is archived in
[`archive/2026-08/rm-007-a-local-personal-data-contract.md`](archive/2026-08/rm-007-a-local-personal-data-contract.md). It defines
versioned local `LocalProfile`, `Favourite`, and `PlaybackHistoryEntry` shapes; local-only privacy,
dedupe/order/retention, safe rollback, and RM-004-compatible identity lifecycle rules. It specifies
automatic resolution only for verified canonical IDs, reviewed legacy mappings, and `merged`
tombstones; `split`, removed, and unknown references remain local unresolved records. The document
does not claim existing favourites/history: verified sources show only RockMobile unavailable-voice
ID migration and RockCast's catalog/legacy adapters. No code, database, API/OpenAPI, catalog, sync,
or authentication behavior changed. Human approval remains required for the explicitly listed
ordering/limits, split UX, RockCast legacy mapping, lifecycle artifact, and future-sync decisions.

## RM-004 shared catalog

### RM-004-I cutover and legacy cleanup (2026-08-25)

RM-004-I designates RockCatalog `2026.08.2` as the approved immutable curated baseline:
SHA-256 `3fa20dca94fc059bd433a47b9fba9bb6d5e5e1aa2957a5ffb58b2a7b20b1d74d`. RockServer, RockCast,
and RockMobile pin the same JSON release; RockMobile additionally pins the separate extended
SQLite `2026.08.2-mobile.1` (schema 1, 16,825 stations, SHA-256
`ad469d405f177d7e476cf9b3d9985497d0e2c6132ac0f3ce14485f4eab402073`). RockCatalog is the only
baseline authoring source. `release_sync.py` is the only supported consumer-update route; ordinary
builds/startup neither copy nor fetch catalog data. Rollback selects retained immutable artifacts
through the same offline sync/verify workflow.

RockCast's TXT adapter remains the sole cutover legacy exception: owner RockCast maintainers,
reason existing offline user overrides, removal date 2026-10-31. RockMobile has no TXT path. The
service diagram reflects the verified authoring/release/baseline/extended flow. Offline release
tool tests (12/12), `release_sync.py verify` for all consumers, RockServer fmt/Clippy/full tests,
RockCast fmt/Clippy/full tests, and a loopback RockServer readiness smoke passed on 2026-08-25.
RockMobile Gradle checks remain unrun because this environment cannot read the configured Android
API-36 SDK package metadata and its SDK licenses are unaccepted; no SDK download or license change
was performed. Device, stream-playback, live provider, and live PostgreSQL checks remain outside
this offline cutover verification. The RM-004-I acceptance gate is therefore not fully passed:
Android offline-client execution remains an environment handoff item.

### RM-004-H High-issue remediation (2026-08-25)

RockServer now preserves canonical `tombstones` through preflight, activation, PostgreSQL
persistence, and internal lifecycle lookup. `removed` records have no successor, `merged` records
may resolve automatically to their sole replacement, and `split` records remain explicitly
ambiguous so no caller can silently move user state. Migration `0011_add_catalog_tombstones.sql`
stores the active `rockcatalog` lifecycle view separately from soft-retired station rows. Shared
catalog activation replaces that view, provider-scoped retires missing RockCatalog records, and
commits both changes with the import run; reimporting a release is idempotent and reactivating a
previous release removes its newer lifecycle view. Radio Browser rows remain outside these writes.
The existing public HTTP/OpenAPI response remains unchanged.

Pinned catalog failure is now fail-closed. The production in-memory path returns a preflight error
for missing, malformed, checksum-invalid, schema-invalid, or semantically invalid pins; startup
does not select the old six-station fixture. PostgreSQL preflights the immutable catalog before
activation, so such a failure leaves the prior successfully activated database snapshot untouched
and causes startup/readiness to fail when no active snapshot exists. The compact fixture is compiled
only for isolated Rust unit tests and has no production selection path. This behavior is offline and
does not fetch a replacement catalog.

RM-004-G is complete at its release-and-synchronization acceptance gate. The local immutable
baseline release layout is `C:\repos\rockcast-station-catalog\release\2026.08.2`, containing
canonical JSON, schema, SHA-256 manifest, release metadata, and changelog. The explicit offline
sync/verify tool now updates RockServer, RockMobile, and RockCast only after checksum and
schema/version validation; ordinary builds and startup remain download-free. All three consumer
snapshots verified against baseline `2026.08.2` SHA-256
`3fa20dca94fc059bd433a47b9fba9bb6d5e5e1aa2957a5ffb58b2a7b20b1d74d`. RockMobile additionally
validated its independent SQLite package `2026.08.2-mobile.1`, schema 1, count 16,825, SHA-256
`ad469d405f177d7e476cf9b3d9985497d0e2c6132ac0f3ce14485f4eab402073`. RM-004-H/I have not started.

RM-004-B and RM-004-C have passed their acceptance gates in the separate local repository
`C:\repos\rockcast-station-catalog`. The approved 41-station release candidate is version
`2026.08.2`, with SHA-256 `3fa20dca94fc059bd433a47b9fba9bb6d5e5e1aa2957a5ffb58b2a7b20b1d74d`.
RockServer now vendors that immutable artifact and schema, validates it before use, selects it as
the in-memory fallback, and activates it transactionally under provider source `rockcatalog` when
the PostgreSQL repository initializes. Records use canonical station IDs and `<station-id>:<stream-id>`
stream identities; missing baseline rows are provider-scoped soft-retired. Public HTTP/OpenAPI
result fields and single selected primary stream semantics are unchanged. RM-004-E onward remain
out of scope. RM-004-D also includes a deterministic PostgreSQL-to-SQLite exporter for a future
RockMobile/Room bundle, documented in `docs/rockmobile-extended-catalog.md`. Its release gate
requires at least 16,000 active/playable stations with exactly one active primary HTTP(S) stream.
The verified local `rockserver` database contained 16,825 eligible stations on 2026-08-21 and
produced the complete `2026.08.2-mobile.1` release SQLite, manifest, and eligibility report under
`release/mobile-catalog/`. Its exact-file SHA-256 is
`ad469d405f177d7e476cf9b3d9985497d0e2c6132ac0f3ce14485f4eab402073`. The report records
provider-safe dedupe, primary selection, and server-only exclusions.

Verification on 2026-08-21 passed `cargo fmt --check`, strict all-target/all-feature Clippy, the
regular `cargo test` suite, the opt-in PostgreSQL integration test against a disposable local
PostgreSQL/pgvector container, and the catalog repository's offline validator, formatting check,
and nine-tool-test suite. The mobile SQLite unit fixture additionally passed `PRAGMA integrity_check`,
schema/version, metadata/count, FTS count, search-index, and exact-byte SHA-256 checks.

## Architecture refactor review

The package layout was reviewed and the first safe refactor was completed. Catalog import code now
lives under `src/catalog/`, and voice-command plus streaming speech boundaries live under
`src/voice/`. Compatibility facades preserve the former `rockserver::speech` and
`rockserver::voice_command` paths for existing clients and tests. HTTP and search remain the next
large seams for incremental extraction: `src/http/mod.rs` still owns several transport concerns,
and `src/search/mod.rs` still combines domain models, the in-memory repository, and orchestration.

Verification for this refactor: `cargo fmt --check`, strict all-target/all-feature Clippy, and
`cargo test` passed. Tests used an isolated `target-codex` directory because an existing local
RockServer process held the normal debug executable open. No public route or OpenAPI behavior was
changed.

## Local ONNX semantic search

`onnx-local` provides CPU-only `intfloat/multilingual-e5-small` inference (384 dimensions) using local ONNX Runtime and tokenizer assets. Migration 0005 persists normalized station text and adds an E5-provenance cosine HNSW index. On 2026-08-17 the local PostgreSQL database contained 1,005 stations (999 imported from Radio Browser), all 1,005 searchable documents were backfilled with matching E5 embeddings, and live Russian queries completed through `POST /api/v1/search`.

## Semantic language and country intent filters

When the local E5 semantic embedding provider is configured, startup now prepares 28 compact,
multilingual E5 language-label vectors. The query vector is reused for both the language decision
and station ranking, so a request performs no duplicate query inference. A language becomes a hard
filter only when its cosine score is at least 0.72 and exceeds the next candidate by at least 0.04;
otherwise the search remains broad. `ROCKSERVER_SEMANTIC_LANGUAGE_FILTERS=off` disables only this
hard-filter layer while preserving semantic station ranking.

The parser distinguishes language wording from an explicit country request. For example,
`Включи английский рок` can yield `language=en`, while `Включи рок из Англии` yields
`country_code=GB`. The deterministic parser recognizes `англии`, and a validated LLM country is
preserved only after an explicit `из` or `from` country phrase. This prevents the UI/STT locale or
a cultural adjective from becoming a country hard filter. The E5 label set currently covers Arabic,
Chinese, Czech, Danish, Dutch, English, Finnish, French, German, Greek, Hebrew, Hindi, Hungarian,
Indonesian, Italian, Japanese, Korean, Norwegian, Polish, Portuguese, Romanian, Russian, Spanish,
Swedish, Thai, Turkish, Ukrainian, and Vietnamese; additional labels require a deliberate quality
evaluation before they are added.

Latest verification for this change: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` passed. The regular suite ran 67 library tests plus HTTP/contract tests; database and credential-dependent live tests remained explicitly ignored.

## Current state

The first API-access security slice is implemented. Startup requires the configured application
Bearer credential; `POST /api/v1/search`, both voice-command
routes, and both WebSocket voice-stream handshakes reject malformed or invalid
`Authorization: Bearer` credentials with structured `401 authentication_required` responses.
`/health/live` and `/health/ready` remain unauthenticated for process supervision. The initial
token is deployment configuration only, compared without prefix-based early exit, and is not
logged. Durable, per-RockCast client credentials, administrator sessions, and the HTMX console
remain the next deliveries described in `docs/admin-security-plan.md`.
`GET /admin` now provides a local, read-only administration-console preview: a browser operator
enters the existing application Bearer token, which stays only in the current tab's memory, then
can inspect readiness and search catalog stations. It is not an administrator-account, session,
audit, or catalog-management implementation.
`run-rockserver-local.ps1` loads `ROCKSERVER_API_BEARER_TOKEN` and `DATABASE_URL` from the ignored
local `.env`, rejects missing/short credentials, and never prints the token. Its startup output
reports the configured bind address and derives localhost plus active LAN admin-preview URLs from
the actual port and network interfaces instead of printing a fixed `127.0.0.1` address.
`docs/service-diagrams.html` now distinguishes this deployed single deployment credential from
the planned persisted RockCast clients and Bearer-based administrator sessions, and records the
current RockCast Bearer voice-handshake integration.

The active product direction is Windows-first: RockCast text search, Windows microphone capture, the RockServer voice/STT path, and production hardening will be completed and validated before any ESP32 work. The ordered plan is recorded in `docs/windows-production-roadmap.md`. ESP32 is future backlog, not a current delivery stage.

RS-007 was re-verified on 2026-08-14 against the disposable local `rockserver` database on PostgreSQL 18.1 with pgvector 0.8.6. A clean `public` schema accepted migrations 1–4, the ignored real integration test passed, and two deterministic development backfills produced seven unique 32-dimensional station embeddings with no duplicate provenance keys. A live HTTP smoke check reported both health endpoints ready and returned hybrid-ranked `jazz` results. Credentials remain environment-only.

Stages 0–10 are complete in the current working tree: repository bootstrap, Axum HTTP skeleton, OpenAPI search contract, deterministic in-memory search, PostgreSQL persistence, controlled Radio Browser import, RS-007 semantic ranking foundation, RS-008 voice-command JSON contract, the provider-neutral streaming voice API, and Yandex AI Studio intent parsing.

`POST /api/v1/search` keeps the existing request and response schemas. `SearchService` now owns request-only query interpretation and optional query embedding before calling `StationRepository`. The in-memory repository remains metadata-only. PostgreSQL uses exact pgvector cosine similarity when the query and station embedding provenance match, and otherwise preserves metadata fallback.

RS-008 stabilizes the Windows voice-command transport contract without introducing audio upload or an STT provider. The canonical route is `POST /api/v1/voice/command`; `POST /api/v1/voice/command` is a deprecated compatibility alias with identical behavior. Both accept only an already-recognized `transcript` plus the established locale, limit, and station-exclusion controls, then delegate to the existing `SearchService`. A successful response returns the trimmed transcript, normalized query, full deterministic result list, and `selected_station` equal to the first result or `null` when there is no match. Existing `POST /api/v1/search` remains unchanged.

The JSON voice route retains its existing limits and error behavior. Canonical WebSocket `GET /api/v1/voice/stream` (deprecated alias `/api/v1/voice/stream`) now accepts a validated start event and bounded PCM16 mono binary chunks, emits partial/final transcript events, and resolves the final transcript through `SearchService`. Chunks are limited to 65,536 bytes, sessions to 10 MiB, provider operations to ten seconds, and search to five seconds. Terminal WebSocket errors retain `code`, `message`, `request_id`, and `details`.

The `StreamingSpeechRecognizer`/`SpeechStreamSession` traits keep recognizers replaceable without exposing credentials or upstream protocol details to clients. With `YANDEX_AI_API_KEY`, startup exposes both `YandexSpeechKitRecognizer` (buffered v1) and `YandexSpeechKitStreamingRecognizer` (v3 gRPC); otherwise both modes use `UnavailableSpeechRecognizer`. A client selects `recognizer_mode` in its start event: omitted or `buffered_v1` preserves commit-time recognition, while `streaming_v3` forwards chunks upstream and emits partial/final updates.

An ignored `yandex_speechkit_live` integration test covers a real, pre-recorded mono Ogg/Opus command against SpeechKit's synchronous endpoint. Its committed fixture and expected phrase are generated explicitly by `generate_speechkit_fixture` through Yandex TTS; both network operations are billable and only run when selected. The recognition test is gated by explicit credentials, while optional variables can override the committed audio and phrase. Service-account API-key requests intentionally omit `folderId`, because SpeechKit derives the folder from the service account. The generator's opt-in `YANDEX_SPEECHKIT_DEBUG=1` mode prints non-secret request metadata, response status and headers, plus a 16 KiB bounded redacted error body; it also removes key-like fragments echoed by the provider. For explicit mismatch diagnosis, `TEST_YANDEX_STT_DEBUG=1` logs the non-secret STT request metadata and actual/expected transcript; its authorization is always redacted. It does not make the streaming `SpeechStreamSession` production-ready. After correcting local authentication, the 2026-08-14 live TTS generation and STT recognition succeeded: the committed 18,341-byte Ogg fixture produced HTTP 200 and matched the expected transcript. Earlier 401 attempts were retained only as safe diagnostic history.

`diagnose_speechkit_pcm` is an opt-in local diagnostic binary. It synthesizes a short Russian command as 48 kHz PCM16 and sends that same in-memory audio directly to SpeechKit v1, printing only text, byte count, duration, timings, and transcript. It writes no audio and helps separate provider behaviour from RockCast microphone capture and WebSocket transport.

The query parser, LLM provider, and embedding provider are traits. `LlmQueryParser` turns a bounded provider JSON response into the existing `QueryIntent`; it receives no catalog. The deterministic metadata parser is the default and the parser failure fallback. `YandexLlmProvider` is enabled only when both `YANDEX_AI_API_KEY` and `YANDEX_FOLDER_ID` are present; absent configuration preserves deterministic startup, while partial configuration fails safely without revealing values. It uses the documented synchronous AI Studio completion API, `Api-Key` authorization, `gpt://<folder>/yandexgpt/latest` by default, JSON Schema output, a 3-second timeout, and response/token bounds. Malformed/oversized/non-2xx/timeout responses and invalid hard filters all degrade through the existing deterministic path. Optional local `.env` loading is provided by `dotenvy`; `.env` and `.env.*` stay ignored. This is intent parsing for text and already-recognized voice transcripts, not SpeechKit STT. Ordinary tests use loopback mocks and never contact Yandex or another external provider.

RS-015 adds a provider-neutral `CommandInterpreter` above the same replaceable `LlmProvider`. `LlmCommandInterpreter` sends bounded STT text and locale with a strict Yandex-compatible JSON Schema, deserializes directly into serde-backed `VoiceCommand`, `Intent`, and `RadioQuery`, and then performs only semantic validation and normalization. It supports radio play/search, stop, next/previous station, relative volume change, and unknown intent without an heuristic command parser. The separate ignored `yandex_llm_live` target has four exact tests; its primary calm-jazz case passed live on 2026-08-14 and safely logged the POST endpoint, redacted authorization/folder, request body, HTTP 200 response body, and final typed command.

Station embedding generation is a separate `backfill_embeddings` command. It is never called by HTTP startup or `POST /api/v1/search`. Radio Browser import ownership and update semantics remain unchanged.

## Configuration and behavior

- HTTP listener: `ROCKSERVER_BIND_ADDR`, default `0.0.0.0:3000`.
- Verification on 2026-08-20: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed; `graphify update .` refreshed the local code graph.
- Application access gate: clients send `Authorization: Bearer <token>`; the process requires
  `ROCKSERVER_API_BEARER_TOKEN` and rejects missing/blank values at startup. Persisted users and
  revocable client tokens remain future work.
- Logging filter: `RUST_LOG`, default `info` when unset or invalid.
- HTTP catalog backend: `DATABASE_URL` is required and selects PostgreSQL; the in-memory repository
  is limited to deterministic tests and is not selected by the service process.
- Optional query embeddings: `ROCKSERVER_SEMANTIC_PROVIDER=deterministic-dev` for tests/development or `onnx-e5-local` with the `onnx-local` Cargo feature and local model/tokenizer/runtime paths; absence means metadata-only search.
- Optional LLM parser: `YANDEX_AI_API_KEY` and `YANDEX_FOLDER_ID` together enable Yandex AI Studio; absence of both selects deterministic parsing, and a partial configuration is a safe startup error.
- Yandex parser tuning: `YANDEX_LLM_MODEL=yandexgpt` by default and `YANDEX_LLM_TIMEOUT_MS=3000`, bounded to 100–10,000 ms.
- Development embedding dimension: `ROCKSERVER_EMBEDDING_DIMENSION`, default 32, valid range 1–16,000.
- Development embedding provenance: model `rockserver-deterministic-dev`, version `1`, plus configured dimension.
- Embedding command: `cargo run --features onnx-local --bin backfill_embeddings`; it requires `DATABASE_URL`, explicit provider selection, and local assets for the E5 provider.
- Radio Browser import command and configuration remain as documented in `README.md`; no import provider is called by search.
- Local database: `compose.yaml` uses `pgvector/pgvector:pg17` with development-only credentials and a PostgreSQL healthcheck.
- Migrations: embedded versioned files run for PostgreSQL search, import, and embedding store connections.
- Public HTTP/OpenAPI schemas: unchanged by RS-007; the description now records optional semantic ranking and metadata fallback.

## Query, ranking, and failure semantics

`QueryParser` receives only validated query text and locale. It returns terms, tags, language, and country intent and has no catalog parameter. Provider intent is normalized and hard filters are validated before repository use. A parser error or invalid intent uses the deterministic metadata parser.

`EmbeddingProvider` receives one query string during search or one station document during the controlled backfill. It never receives the full catalog. Invalid embeddings are rejected before persistence/search: model and version must be non-empty, dimension must be 1–16,000 and equal to the vector length, values must be finite, and the vector must be non-zero. Embedding provider failure omits semantic input and continues with metadata search.

PostgreSQL applies language/country hard filters and station exclusions in the candidate CTE, before score ordering and final limit. Metadata score retains the RS-003/RS-004 exact-token formula. Compatible cosine similarity is normalized and clamped with `1 - cosine_distance / 2`. Final hybrid score is `0.70 * metadata_score + 0.30 * semantic_score`. A station without compatible model/version/dimension provenance retains its full metadata score. Results order by final score descending and station ID ascending; station ID is the last tie-break.

Liveness remains process-only. Readiness calls only the selected repository, so optional parser/embedding failure does not report false unready. PostgreSQL loss still returns 503/`not_ready`; in-memory readiness remains 200/`ok`.

## Persistence and backfill

Migration `0004_add_station_embeddings.sql` enables pgvector and adds `station_embeddings` with station ownership, model, version, dimension, unbounded `vector`, and created/updated timestamps. CHECK constraints enforce declared dimension and non-zero norm. The table key is `(station_id, model, version)`; changing dimension for the same development model/version replaces that station embedding.

The unbounded vector column remains dimension-neutral for tests and future providers. Exact search filters compatible provenance before the cosine operator; migration 0005 adds a partial 384-dimensional HNSW cosine index only for `intfloat/multilingual-e5-small:onnx-v1`.

`backfill_embeddings` reads station documents in stable station-ID pages and upserts one embedding at a time. Repeating the workflow is idempotent for the same provenance and updates existing rows. It does not change Radio Browser ownership, delete catalog rows, probe streams, or make network requests.

## Known limitations

- `deterministic-dev` remains a repeatable hash fixture; production-like semantic behavior requires explicitly provisioned local E5/ONNX assets.
- Yandex AI Studio is the only production LLM query parser; it has bounded per-request fallback but no retry policy or circuit breaker.
- The deterministic parser currently derives a station-language hard filter from the request locale. Live checks therefore used `en-US` to inspect cross-language E5 ranking over the broader English catalog; `ru-RU` intentionally restricts candidates to Russian-language stations.
- The backfill currently visits all stations and upserts every embedding; it does not yet skip unchanged station/model inputs or resume a failed run from durable workflow state.
- A model/version is expected to imply one dimension, but RS-007 records/enforces compatibility per row rather than introducing a separate model registry.
- Existing Radio Browser pagination, stale-run, language-code, and upstream-health limitations from RS-006 remain.
- RockCast calls the protected canonical voice WebSocket route, records from the default microphone, maps server results into playback, and distinguishes missing/invalid token and unavailable-server outcomes. Input-device selection/testing, deterministic end-to-end coverage, and clearer upload/recognition/cancellation states remain.
- Yandex SpeechKit supports selectable buffered v1 and streaming v3 modes when its API key is present. Provider retries/circuit breaking, a second provider, and live end-to-end streaming coverage remain.
- Per-client credential persistence/revocation, administrator authentication, login throttling,
  request audit storage, admin console state-changing operations, rate limiting, metrics, scheduler,
  stream probing, and deployment hardening remain future work.

## Verification

The regular suite passes with 67 deterministic unit, HTTP, contract, and WebSocket tests; the real PostgreSQL, SpeechKit, and four Yandex LLM integration cases remain opt-in and ignored by default. Coverage includes typed command serde/schema/semantic validation, loopback Yandex LLM mocks for header/model/schema, non-2xx, timeout, malformed/oversized response, and secret-safe errors; a real loopback WebSocket upgrade with fake recognition; application Bearer rejection while health stays public; the unauthenticated admin-preview shell; and existing query, provider-fallback, embedding, ranking, and persistence boundaries.

Docker Engine 29.7.2 and Compose 5.3.1 ran isolated project `rockserver-rs007-test` on host port 55438 with `pgvector/pgvector:pg17`. The real integration test passed for extension/migrations, embedding insert and repeat update, provenance/dimension storage, exact cosine similarity, hard filters, exclusions, final limit, semantic tie-break, metadata fallback, HTTP search, and repository-only readiness. The real deterministic backfill command also completed twice; inspection showed seven unique development rows for `rockserver-deterministic-dev:1:8`. Only that project's container, network, and volume were removed afterward.

For RS-015, `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed. The exact ignored live command test also passed against Yandex with a typed calm-jazz result. `graphify update .` then rebuilt the local graph to 774 nodes, 1,692 edges, and 37 communities; it reported the pre-existing zero-node `hooks.json` warning and stale community labels.

For RS-016, `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test --all-features` passed with 50 regular tests; six credential/asset/database-dependent tests remained ignored. The real local import produced 999 Radio Browser stations alongside six built-ins, and E5 backfill produced 1,005 `intfloat/multilingual-e5-small:onnx-v1:384` rows. Live search ranked `Midnight Jazz Lounge` first for `спокойный джаз` and `# RdMix Classic Rock 70s 80s 90s` first for `рок 80-х`. `классическая музыка без слов` returned weaker ambient/jazz results, documenting a current catalog/locale-filter quality limitation. `graphify update .` rebuilt 809 nodes, 1,774 edges, and 42 communities with the known zero-node `hooks.json` and stale-label warnings.

## Genre hierarchy and search fallback

RS-019 introduces a genre taxonomy hierarchy and progressive search fallback. `CANONICAL_TAGS` now includes subgenres present in the real Radio Browser catalog (`black metal`, `death metal`, `alternative rock`, `classic rock`, `pop rock`, `smooth jazz`, etc.). A `genre_parent` function maps each subgenre to its nearest parent (`black metal` → `metal` → `rock`, `smooth jazz` → `jazz`). `station_matches_requested_genre` accepts stations whose tags match any ancestor of the requested genre, so a `"heavy metal"` query matches stations tagged `"rock"`.

`SearchService::search` applies progressive fallback when the exact genre filter yields no results: first broadening to parent genres, then dropping the genre constraint entirely while keeping the `MIN_RELEVANCE_SCORE` gate. The LLM system prompt automatically includes the expanded tag vocabulary. A new builtin catalog station (`Iron Forge Radio`, tags `heavy metal`/`metal`, language `en`) validates the hierarchy. Deterministic tests confirm that `"english heavy"` finds English rock/metal stations, that subgenre queries match parent-tagged stations, and that unrelated genres remain rejected.

## Genre database and stream probing

RS-020 moves the genre hierarchy from compiled-in constants to a PostgreSQL `genre_hierarchy` table seeded with ~250 genres across 25 root categories. `GenreTaxonomy` loads from the database at startup with a builtin fallback for in-memory mode. A new `probe_streams` binary connects to each stream URL (8-second timeout, 50 concurrent), marking unreachable streams as `degraded` with the error stored in `last_probe_error`. The PostgreSQL search CTE now excludes `degraded` streams. The Radio Browser importer supports `RADIO_BROWSER_TAGS` for per-tag import passes, enabling broader genre coverage. Migrations 0006 and 0007 are applied automatically on connect.

## Station name search improvements (RS-021)

RS-021 improves station name search accuracy for voice queries such as "включи радио диджей" → `radioDJ`:

- **Stop-word filtering**: Command verbs (`включи`, `поставь`, `найди`, `play`, `find`, etc.) are removed from query terms before matching, preventing score dilution.
- **CamelCase splitting**: `tokenize()` now splits `"radioDJ"` into `["radio", "dj"]` and `"XMLParser"` into `["XML", "Parser"]`. This affects both query tokenization and `searchable_text()` at import time.
- **Transliteration expansion**: A word-level mapping (`радио`↔`radio`, `диджей`↔`dj`, `фм`↔`fm`, `рок`↔`rock`, ~50 entries) expands query terms so that Cyrillic queries match Latin station names and vice versa.
- **Substring matching**: Both in-memory ranking and the PostgreSQL search CTE now award partial credit (0.5 per term) for substring matches in station names (terms ≥ 3 chars), so "радио" finds stations with "radio" in their name even without exact token match.
- **Backfill script**: `fill-database.ps1` now includes `backfill_embeddings` as the final step after import and stream probing.
- **LLM intent normalization + fallback**: Provider `terms` are normalized into atomic tokens (`tokenize()`), expanded with transliteration aliases, and if the provider returns empty `terms` and empty `tags` the system switches to deterministic parsing for more reliable station-name matching.
- **Search performance**: Added a PostgreSQL `pg_trgm` + trigram index on `lower(stations.name)` to accelerate `LIKE '%term%'` prefiltering/substrings.

## Full-text search and similarity scoring (RS-023)

RS-023 replaces the expensive sequential `LIKE '%term%'` prefilter with three index-backed search layers:

1. **PostgreSQL FTS (tsvector)**: Migration 0009 adds a `searchable_tsv` tsvector column derived from `searchable_text` with a GIN index and auto-update trigger (config `simple` for language-neutral exact tokens). The prefilter uses `plainto_tsquery('simple', $14)` for O(log n) candidate selection.
2. **pg_trgm similarity**: The prefilter now uses the `%` operator (`lower(s.name) % qt.term`) instead of `LIKE '%term%'`, leveraging the existing GIN trigram index for fuzzy name matching. A `trgm_score` (max similarity) contributes 0.3 weight to the metadata score.
3. **Embedding cosine (unchanged)**: Semantic fallback via pgvector HNSW index remains as before.

`raw_query` (stop-words removed) is passed through `QueryIntent` → `SearchQuery` → `PostgresSearchParameters` → `$14` for FTS matching.

## Correct name ranking when LLM omits terms (RS-024)

RS-024 fixes incorrect ranking and extra latency when the LLM returns `tags` but omits `terms` (e.g. "включи радио рокс"). Previously this caused name-token matching to be skipped and ranking fell back to mostly genre-tag + semantic similarity.

Now in `SearchService::interpret_and_search`:

- if `intent.terms` is empty, we run `DeterministicQueryParser` and:
  - keep provider genre `tags` (when present),
  - but replace `terms`, `core_term_count`, and `raw_query` with deterministic tokenization output.
- the transliteration vocabulary was extended with common voice tokens:
  - `ультра`↔`ultra`, `рокс`↔`roks`, `викер`↔`viker`.

## Narrow prefilter candidate set before heavy scoring (RS-025)

RS-025 reduces slow broad queries by making the `prefiltered` CTE `MATERIALIZED`, assigning each row a cheap `prefilter_score` (exact tag hits + `ts_rank_cd` + max trigram similarity), and limiting the candidate pool to `GREATEST(limit * 20, 200)` before the expensive exact-token, substring, and embedding scoring phase.

## Indexed candidate-branch search (RS-030)

RS-030 addresses slow PostgreSQL station searches observed in request logs. The search SQL now obtains candidate IDs through three independent, index-backed branches (tag GIN, FTS GIN, and name trigram GIN), unions and de-duplicates those IDs, and only then calculates the unchanged prefilter and final ranking scores. Full-text term queries are generated once per search term, and each candidate station name is normalized and tokenized once before its exact-name, substring, and trigram scores reuse those values. This removes repeated parsing and regex tokenization for expanded/transliterated voice terms without changing matching semantics. Query interpretation, matching rules, limits, hard filters, exclusion handling, score formulae, and deterministic ordering are unchanged. Debug logs now record parser, embedding, repository, and database-search elapsed milliseconds plus result count. Live `POST /api/v1/voice/command` checks against the configured PostgreSQL database returned 30 stations in 1,387 ms for `Включи немецкий рок или хэви метал` and in 1,388 ms for the STT transcription `Включи немецкий рок или хейли метал`; the latter spent 1,097 ms in parsing and 233 ms in PostgreSQL, below the five-second voice timeout. A production `EXPLAIN (ANALYZE, BUFFERS)` comparison remains useful for broader capacity monitoring.

## Explicit world-country filters (RS-031)

RS-031 expands deterministic country recognition from three countries to the ISO 3166-1 alpha-2 country set. It recognizes common Russian and English country names plus demonyms, including `немецкий`/`Germany` -> `DE`. The parser still derives `country_code` only from an explicit word sequence in the original request, never from locale or an LLM guess. If a phrase is ambiguous (for example, `Конго` without a republic qualifier), it emits no hard country filter.

## Remove per-candidate regex tokenization from exact matching (RS-026)

RS-026 replaces the exact-term part of `matched_count` with `searchable_tsv @@ plainto_tsquery('simple', qt.term)` so PostgreSQL reuses the precomputed normalized document instead of re-running `regexp_split_to_table(...)` over station names and tags for every candidate row. The cheap prefilter ordering also now avoids early full-table trigram scoring and keeps trigram similarity only in the limited candidate phase.

## Fast empty result when prefilter has no candidates (RS-027)

RS-027 removes semantic-only fallback when metadata prefilter returns zero rows. For unknown or unsupported requests this makes search return empty quickly instead of spending extra time on embedding-neighbor fallback and returning low-value semantic matches.

## Generic bidirectional transliteration (RS-028)

RS-028 adds generic bidirectional transliteration for query terms (Russian Cyrillic ↔ Latin) as a standard step in `expand_transliterations`, instead of relying on point-by-point station-brand mappings. This improves cross-script station-name recall (for example, spoken Russian name forms matching Latin station names and vice versa) while keeping existing dictionary-based genre/radio-word normalization.

## Name-priority mode for "включи радио ..." (RS-029)

RS-029 introduces a station-name priority path for commands that explicitly start with `включи радио` / `поставь радио` (and English equivalents). The parser derives an ordered station-name hint phrase from the tail of the command, the query carries that hint through to PostgreSQL, and SQL ranking gives a strong bonus when the normalized station name contains the same words in the same order. This helps short exact station names outrank longer related variants.

## RM-004-A contract planning gate

RM-004-A is complete and approved. The repository
now contains a proposed shared-catalog contract, Draft 2020-12 JSON Schema, and matching example.
The design was based on a read-only inventory of RockServer, RockCast, and RockMobile and records the
41-row RockCast/RockMobile baseline, current URL-derived mobile IDs, provider ownership, multiple-
stream database behavior, public DTO compatibility, migration, release, rollback, and tombstone
rules. No runtime contract, code, database, client, build, CI, catalog data, or external repository
was changed. RM-004-B and later stages have not started.

Planning verification on 2026-08-21 parsed both JSON files successfully, confirmed that the schema's
embedded example matches the standalone example, checked proposal/schema terminology and required
fields, ran `git diff --check`, and confirmed the change scope is `docs/**`. Cargo and Gradle checks
were intentionally not run because this task changes documentation only.

## Next step

The next RM-004 work, if separately approved, is client integration (RM-004-E/F) and later release
automation. RockMobile can consume the verified `2026.08.2-mobile.1` package according to the
documented checksum procedure; future catalog versions must satisfy the same fixed 16,000-station
release gate. The previously recorded service follow-up remains to add deterministic RockCast-to-RockServer end-to-end coverage
for search and voice, then improve voice cancellation and state reporting across capture, upload,
recognition, search, and playback, followed by retention-safe logging, provider resilience, and a
second recognizer behind the existing trait.

## Shared internet-beta roadmap

The retired planning-only product roadmap placed RM-007, RM-011,
and RM-012 ahead of unrelated roadmap work to prepare a hosted RockServer beta with users,
registration, device linking, synchronization, and remote control across RockMobile and RockCast.
The ESP32 client is intentionally deferred until hardware is available. The documented proposed
first deployment is a single VPS with Docker Compose, Caddy, private PostgreSQL, immutable images,
manual release approval, backup, health checks, and rollback. No runtime, API, database, client,
deployment, credential, or infrastructure change was made by this documentation task.

## Executable internet-beta plan

The retired shared roadmap was decomposed into concrete, sequential Codex tasks:
GATE-001; RM-007-A through D; OPS-001-A through D;
RM-011-A through F; RM-012-A through F; then ESP-001 and ESP-002 when hardware exists. Every
task has repository boundaries, dependency, recommended model/reasoning effort, scope and
acceptance gate. The plan itself is not an approval to implement any task.

## RM-007-D cross-client review (historical evidence, 2026-08-25)

At the time of this evidence-based review, RM-007-D was **not passed**. The review in
`docs/archive/2026-08/rm-007-d-cross-client-review.md` confirmed the shared baseline release and the clients'
offline local-catalog paths, but found four unresolved High issues: RockMobile's persisted JSON is
not the portable RM-007-A profile shape; its read/init path can replace corrupt or unsupported data
with an empty profile and has no migration backup/rollback; its lifecycle resolver is not wired to
production catalog state; and RockCast records URL-derived `rockserver-*` IDs for remote/voice
plays instead of canonical server station IDs. Both clients also have incomplete favourite merge
metadata semantics.

Offline release verification passed for RockServer, RockCast and RockMobile (including the Mobile
extended manifest); catalog tooling passed 12/12 tests; RockServer targeted catalog tests passed
14/14; RockCast targeted personal-data tests passed 5/5 using a separate writable target directory
after its ordinary target lock was inaccessible. RockServer fmt, strict Clippy and full tests, а
также RockCast fmt, strict Clippy и full tests прошли; RockCast full result — 55 library и 2 relay
integration tests, 8 live-network tests ignored. RockMobile targeted unit and lint commands both
stopped before compilation on the inaccessible Gradle wrapper `.zip.lck`, despite the required
process-local `-Duser.home=C:\Users\alex`; lint is not claimed as passed and its three previously
known errors remain unresolved/unverified in this run. No network, production service, secret,
live database, device test, product code or catalog data was changed.

OPS-001-A remained independently available as design-only work. The owner later manually
confirmed RM-007-D for the limited purpose of proceeding with RM-011-A proposal work on
2026-08-26; this historical review remains evidence and must not be reinterpreted as a passed
client verification or a compatible-sync guarantee.

## OPS-001-A production deployment design

`deploy/README.md` now records the proposed single-VPS boundary: Caddy alone exposes 80/443;
RockServer (3000) and PostgreSQL (5432) remain Compose-internal, with named data/certificate
volumes, off-VPS encrypted logical backups, readiness-based rollout, and a compatibility-aware
application/database rollback procedure. It defines the safe exact placeholder
`api.rockserver.example.invalid`, an environment-variable contract with no values, owners and the
required restore rehearsal. OPS-001-B is the implementation that closes the former production gaps:
the service now requires both its Bearer credential and `DATABASE_URL`. No VPS, DNS, production
credential, registry image, firewall or deployment was created.

OPS-001-A design review was explicitly confirmed by the user on 2026-08-25. The approved design
uses the documented non-routable placeholder until a real owned domain/DNS and deployment owners
are supplied as a separate manual launch step; it does not authorize publication, credentials or
VPS changes. Secret injection, backup encryption/retention/key custody/RPO/RTO, SSH allowlist,
Caddy ACME policy and restore authority remain pre-launch operational inputs, not repository
secrets.
`git diff --check`, `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
passed locally (81 regular Rust unit tests plus HTTP/contract suites; two disposable-PostgreSQL,
four billable Yandex LLM, and one credential/audio SpeechKit integration tests remained explicitly
ignored). No Docker or external-infrastructure check applied to that design-only task.

## OPS-001-B reproducible container and Compose stack

OPS-001-B adds the root `Dockerfile`, `.dockerignore`, `deploy/compose.yaml`, local and production
overrides, Caddy templates, a safe `.env.example`, and `deploy/verify-compose.ps1`. The base topology
contains `caddy`, `rockserver`, and `postgres`; only Caddy receives host ports in either local or
production launch, while PostgreSQL stays on the internal `database` network. The application and
PostgreSQL have healthchecks, Caddy waits for application readiness, and the local check reaches
`/health/ready` through loopback Caddy.

Docker image build passed with the pinned Rust 1.95/bookworm builder and non-root runtime. Local
Compose configuration and production rendering passed without printing environment values. The
disposable local stack reached healthy PostgreSQL, RockServer and Caddy states, and the loopback
Caddy request to `GET /health/ready` returned HTTP 200; the script removed its containers, network
and volumes afterward. Production launch remains a manual step requiring a real owned domain,
secret injection, immutable image, VPS, DNS, firewall and backup owners. No production external
action or secret was performed.

OPS-001-B verification: `cargo fmt --check` and strict all-target/all-feature Clippy passed;
`cargo test` passed with 82 regular tests plus HTTP/OpenAPI/WebSocket suites using the serial
`target/ops001b-test-serial` target to fit this host's pagefile. Two disposable-PostgreSQL, four
billable Yandex LLM, one credential/audio SpeechKit and one ONNX asset test remained explicitly
ignored. `graphify update .` refreshed the local graph to 1,673 nodes, 3,192 edges and 98
communities. OPS-001-B is **passed locally**; public deployment remains outside this task.

### RM-007-D remediation

The four original High findings were remediated in client source: RockMobile now uses the portable
v1 profile/timestamp shape with checked persistence, fail-closed reads, legacy backup/journal,
explicit rollback and wired baseline lifecycle resolution; RockCast now preserves canonical server
IDs for search/voice personal records and completes dedup/journal/rollback behavior. RockCast fmt,
strict Clippy and full tests pass after remediation. Android targeted unit and lint commands
remained blocked before compilation by the inaccessible Gradle wrapper lock. The owner later
manually confirmed RM-007-D for RM-011-A proposal work; that decision does not claim the blocked
Android verification as passed.

## OPS-001-C CI image, release, backup and rollback runbook

OPS-001-C is implemented locally. `.github/workflows/ci-release.yml` runs the required Rust
format/Clippy/test checks, builds a container labelled with the source commit SHA, and runs a
loopback Compose readiness smoke using credentials generated only inside the CI job. Publishing to
GHCR is behind `workflow_dispatch`, `publish_image=true`, and the repository `release-gate`
environment approval. No registry credential or application secret is stored in the workflow.

`deploy/release.ps1` is the documented release entry point. It accepts only an image reference with
an SHA-256 digest, renders production Compose without printing its environment, rejects host ports
on PostgreSQL/RockServer, and requires an environment file outside the repository for deploy or
rollback. Deploy backs up PostgreSQL in custom `pg_dump` format before starting the immutable image,
records the backup SHA-256 beside a release record, waits for Docker health and then requires an
approved readiness URL to return HTTP 200. Rollback reuses the same path with a previous verified
image digest; incompatible migrations still require the documented backup restore procedure.

`deploy/restore-rehearsal.ps1` performs a non-production `pg_restore` into a disposable PostgreSQL
container/network, validates restored RockServer tables, runs the local RockServer image against
the restored database and checks readiness inside that network. Temporary credentials are generated
at runtime and are not printed. Disposable cleanup completed successfully in the verified rehearsal.

OPS-001-C local verification on 2026-08-25: PowerShell syntax checks passed for all deployment
scripts; production Compose rendering and port isolation passed; the pinned Docker image built;
the local Compose stack reached healthy PostgreSQL/RockServer/Caddy and loopback readiness HTTP 200;
release preflight dry-runs passed for both `preflight` and `rollback`; and the full disposable
`pg_dump`/`pg_restore` rehearsal passed with restored application readiness. The first rehearsal
attempt exposed and fixed a binary stdout redirection issue and a missing explicit restore username;
the corrected run passed. Final local checks also passed: `git diff --check`, `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` (82 passed; 2 PostgreSQL,
4 Yandex LLM, 1 SpeechKit and 1 ONNX integration cases ignored by their explicit gates), and
`graphify update .` (1,683 nodes, 3,204 edges, 103 communities; the existing zero-node JSON
warnings remain non-fatal).

The actual GHCR publication, production backup/deploy, public HTTPS readiness, and live rollback
remain manual actions requiring an approved registry/release environment, owned domain/VPS,
external backup target and injected secrets. They were not performed or claimed here.

## MVP-001-B — anonymous `/v1` public policy (local implementation)

`/api/v1/catalog/stations`, `/api/v1/catalog/stations/{station_id}`, `/api/v1/search`,
`/api/v1/voice/command`, and `/api/v1/voice/stream` now use the approved anonymous route boundary;
the existing `/api/v1` aliases retain Bearer authentication. Anonymous JSON is capped at 16 KiB;
search/transcript length is capped at 500 characters and results at 20/10. Until trusted proxy
CIDRs are configured, forwarded headers are ignored and a conservative direct-peer fallback
bucket is used. Voice streaming has global admission, 16 kHz PCM16 mono, 32 KiB frames,
2 MiB/60-second audio, and bounded idle/wall/STT/search time.

The changed public handlers log only aggregate-safe fields; request content, transcripts, URLs,
credentials, provider payloads, IPs and forwarded headers are not emitted. `api/openapi.yaml`
remains unchanged until it is reconciled line-for-line with the runtime contract.

## MVP-001-C follow-up — local launcher credential fallback

`run-rockserver-local.ps1` no longer rejects a local `.env` solely because it
omits `ROCKSERVER_API_BEARER_TOKEN`. It generates a random, non-production,
process-local development credential only for the legacy protected `/api/v1`
and local admin access gate. The anonymous `/v1` allowlist remains
unauthenticated; production startup policy is unchanged.

## RM-011-C — unified web shell and auth runtime (current snapshot)

Итоговый отчёт текущего прогона: [`docs/archive/2026-08/rm-011-c-report.md`](archive/2026-08/rm-011-c-report.md). RM-011-C
имеет статус **RM-011-C server/browser implementation verified**. Full real-client staging E2E
явно перенесён в RM-011-D/E и не является prerequisite этой серверной задачи.

The chosen single first-party frontend/admin stack is **TypeScript + Vite + Preact** in `web/`.
It has one shared same-origin API client and types, passkey registration/authentication, pairing
lookup/approval, and local QR presentation; it never stores a bearer, refresh, browser-session or
pairing secret in localStorage. Caddy serves the static bundle and proxies only `/v1`, `/api/v1`,
and health routes to RockServer, injecting a deployment-only proxy proof header so direct browser
requests fail closed. The Rust routes include registration and authentication options/verify with
cryptographic WebAuthn checks, Secure/HttpOnly/SameSite cookies, CSRF/origin checks, PostgreSQL
rate limits, and desktop pairing completion with one-time native access/refresh issuance.
The browser never receives native tokens; the desktop completion client belongs to RM-011-E.

The runtime now also creates a ten-minute desktop pairing request at `POST /api/v1/pairing-requests`
and resolves pending short codes at `GET /api/v1/pairing-requests/lookup?code=...`. PostgreSQL supplies
the expiry clock and persists only SHA-256 proof hashes; lookup exposes only device metadata and the
verification phrase. Approval, completion and request throttling are implemented; the browser UI
executes approval, while the native desktop client remains responsible for calling completion.
Completion accepts only the desktop proof; PostgreSQL derives the owner from the approved request
inside the same transaction, so native clients never need or submit an account UUID.

The approve endpoint is now present at `POST /api/v1/pairing-requests/{request_id}/approve`; it
requires an `HttpOnly` browser cookie proof, the `X-CSRF-Token` double-submit header, exact
first-party HTTPS origin, and the configured Caddy proxy proof. A new nullable cookie-hash column
keeps old B2 rows readable while all newly created browser sessions bind an opaque cookie hash.

Native session/account management is exposed through `POST /api/v1/auth/device-session`,
`GET /api/v1/account/profile`, `GET /api/v1/devices`, and `DELETE /api/v1/devices/{device_id}`.
Access-token lookup and device revocation are owner-scoped in PostgreSQL; the durable device secret
can create a replacement short-lived access session without rotating or storing a refresh token.

Verification on this snapshot: `cargo fmt --check`, `cargo clippy --all-targets --all-features --jobs 1
-- -D warnings`, `cargo test --jobs 1` (93 unit/library tests plus regular integration suites), web
`tsc --noEmit`, and a direct Vite production build with the bundled Node runtime all pass.
All four opt-in PostgreSQL integration tests passed sequentially against a disposable local
`pgvector/pgvector:pg17` container. Both Caddyfiles validated in Caddy 2.10 containers with test
environment values. Web dependencies were installed with the bundled pnpm runtime; `pnpm typecheck`,
`pnpm lint`, and `pnpm build` passed with the bundled Node path (the unconfigured host PATH still
has no `node` command).

RM-011-C is complete at its server/browser boundary. Real-client staging pairing and mobile/native
session UX are explicitly deferred to RM-011-D/E.

## RM-011 wave 1 — A1/A2 browser pairing state

The Preact pairing page now keeps editable registration input separate from the verified browser
account name. Approval requires a verified account, CSRF token, pending preview, and the exclusive
`authenticated` pairing state. A successful approval enters a dedicated success screen; pending
details and registration/passkey/approval controls are removed, and Mobile/RockCast-specific return
guidance plus an account-centre link are shown. Reloading an approved or consumed request resolves
to a terminal screen because lookup no longer returns a pending preview.

Verification: all eight web UX regressions, TypeScript typecheck/lint, Vite production build,
`cargo fmt --check`, strict all-target/all-feature Clippy, and all available Rust tests pass. Six
disposable PostgreSQL tests and six credential/asset-dependent live tests remain intentionally
ignored. OpenAPI and server behavior were not changed; staging/manual browser smoke was not run.

## HTTP transport refactor

The HTTP module is now a thin public facade. Route handlers and transport logic live in
`src/http/endpoints.rs`, while shared application state and anonymous admission controls live in
`src/http/state.rs`. Runtime behavior and the public API are unchanged.

Verification after the refactor: `cargo fmt --check`, `cargo clippy --all-targets --all-features
-- -D warnings`, and `cargo test` passed (92 unit tests plus regular integration suites; external
PostgreSQL, LLM, SpeechKit, and ONNX cases remain ignored behind their explicit gates).

Local Cargo concurrency is capped at two jobs via `.cargo/config.toml` to reduce CPU and memory
pressure during builds; acceptance checks in this run used explicit `--jobs 1` sequentially.

## Future device-control roadmap (planned, 2026-09-02)

## DC-007 — device-control connection registry and presence (complete, 2026-09-02)

`GET /api/v1/devices/connect` now authenticates only the existing native Bearer session before
WebSocket upgrade. It runs the bounded v1 hello/welcome/register handshake and derives all account
and device identity from `DeviceControlPrincipal`. A process-local, per-user registry publishes
bounded owner-scoped presence history and snapshots. Registration makes a device online; server
heartbeats update server-observed activity and a 60-second TTL produces one offline transition.
Each reconnect has a fresh connection ID. Registering a newer connection atomically replaces the
old generation; stale cleanup is guarded by connection ID and cannot remove the new connection.
The transport uses typed envelope and payload DTOs for hello, register, heartbeat and every server
reply; the register manifest stays only a bounded opaque object until DC-008 defines its semantics.

The router's graceful-shutdown path broadcasts a controlled close to live control sockets; their
registry cleanup remains generation-guarded. Existing owner-authorized device revoke paths also
remove the active connection with reason `revoked` after durable revocation succeeds.

This is deliberately not DC-008 or DC-010: manifests/state/telemetry/commands are neither stored
nor forwarded, and no public controller directory or subscription endpoint exists. Local WebSocket
transport tests cover native 401/503 before upgrade, handshake, identity injection, reconnect,
heartbeat/TTL, malformed/binary/timeout handling, transport/graceful cleanup and shutdown.

## DC-006 — control-plane native authentication foundation (complete, 2026-09-02)

`device_control_auth` reuses the existing native-session lookup rather than creating a control
credential, device table, pairing flow, or renewal path. A bounded `Bearer` access token resolves
only through active native sessions into a server-derived `(user_id, device_id)` principal. The
PostgreSQL lookup now additionally requires that the resolved device belongs to the session owner;
expired, unknown, revoked, malformed, legacy, administrator, and browser credentials do not
produce a principal. Lookup failure is distinct and retryable (`Unavailable`), without revoking a
device or altering its durable secret/binding.

`http::authenticate_control_ingress` is the reusable header extractor used by DC-007. It maps
invalid credentials to a generic 401 and storage failures to a retryable 503 before upgrade.
`POST /api/v1/auth/device-session` remains the sole access-token renewal flow, and the future Home
Assistant integration remains a separate credential type.

Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
`cargo test` passed (120 library tests, all regular suites). The four OpenAPI contract tests passed
within the full suite; PostgreSQL, live provider, and ONNX opt-in tests remained ignored because no
disposable `TEST_DATABASE_URL` was configured.

## DC-005 — device-control persistence foundation (complete, 2026-09-02)

Migration `0021_add_device_control_persistence.sql` adds owner-linked manifest, current
capability/entity/surface, latest device/entity state, and bounded command-idempotency tables.
All rows refer to the existing `devices.id`; no HTTP/auth/runtime wiring, provider identity,
telemetry history, pairing, or device-secret model was added. Manifest replacement tombstones
omitted projections and command pruning retains terminal records for at least 24 hours.

The `DeviceControlStore` and `PostgresDeviceControlStore` preserve current-only state (no sensor
history), revision outcomes, active-owner/revocation filtering, safe tombstone revival, and the
24-hour command idempotency/retention boundary. The contract remains transport-only: no route,
authentication wiring, presence registry, provider adapter or command execution was introduced.

Verification: a separately named local Compose project supplied a loopback-only disposable
`TEST_DATABASE_URL`; the focused ignored PostgreSQL case covered migration, populated account/device
ownership, manifest replacement/revival, latest snapshots, command duplicate/result/pruning,
constraints and revocation. The container, network and named volume were removed afterwards.

The planned architecture and ordered backlog are stored in
`docs/roadmap/device-control-rockcast-esp32.md` and
`docs/roadmap/device-control-tasks.md`. They now cover synchronous and asynchronous
speech output, durable server-side operations, timers, a read-only weather provider,
TTS/audio delivery, and allowlisted device actions in addition to RockCast, ESP32
sensors/displays/voice, and Home Assistant. This is planning documentation only: no
device-control endpoint, scheduler, weather provider, TTS path, or asynchronous delivery
is implemented by this change.

The roadmap reuses the implemented account-owned `devices`, pairing requests, durable
device secret, native access-session renewal, `GET /api/v1/devices`, and device revocation.
It does not introduce a second device table, pairing flow, or RockCast machine credential;
new control-plane persistence is linked to the existing `devices.id`.

Verification for this documentation update: embedded roadmap JSON examples and the
continuous DC-000 through DC-038 task sequence were validated; `cargo fmt --check`,
strict all-target/all-feature Clippy, and `cargo test` passed. The regular suite
included 113 library tests and the listed binary/integration suites; external
PostgreSQL, Yandex, SpeechKit, and asset-dependent tests remained ignored.

## DC-008 — manifest and latest-state ingress (complete, 2026-09-03)

The native control socket now accepts typed, validated `DeviceManifest`, complete device-state,
device-state delta, and entity-state payloads after DC-007 registration. Production routers share
the existing PostgreSQL pool with `PostgresDeviceControlStore`, preserving owner/active/revoked
checks and DC-005 manifest tombstones; offline routers retain no durable state. Accepted latest
snapshots and entity values enter an account-scoped bounded internal broadcast hub. Slow receivers
are lossy (`broadcast::Lagged`) and cannot block a provider connection. Entity freshness is derived
at server time from received/stale timestamps and unknown or removed entities are rejected rather
than fabricated as zero values.

Full snapshots are required before heartbeat; replay is idempotent, and gaps/conflicts request a
full resync. This intentionally excludes the DC-009 command router and DC-010 public
directory/subscription API. Verification: `cargo fmt --check`, strict Clippy, `cargo test` (130
library tests and regular suites), OpenAPI fixture tests, and `git diff --check` passed.
PostgreSQL integration tests were not run because no disposable `TEST_DATABASE_URL` was supplied.
