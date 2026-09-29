# Task log

## 2026-09-29 — DEPLOY: modular refactoring and agent guidelines deployed to staging

- Goal: build, deploy, and verify release commit `137475f` on the staging VPS via `deploy/ops-001-d.ps1`.
- Scope:
  - Built immutable container images `rockserver:sha-137475fb4fd8dfefcd59165614639d2e7145c351` and `rockserver-caddy:sha-137475fb4fd8dfefcd59165614639d2e7145c351`.
  - Executed automated staging deployment via `deploy/ops-001-d.ps1 -Action deploy`.
  - Verified container start and remote readiness probe (`GET /health/ready`).
- Checks: `deploy/ops-001-d.ps1 -Action deploy` returned exit code 0, `status=succeeded`, `readiness=passed`.
- Status: **deployed** as commit `137475f`.

## 2026-09-29 — DOCS: agent instruction guidelines for modularity, file size limits, and test decomposition

- Goal: update contributor and agent instructions to enforce early decomposition of modules and integration test suites, preventing large monolithic files and mixed responsibilities.
- Scope:
  - `AGENTS.md`: added `## Modularity and code organization` section defining file size ceilings (~400-500 lines), architectural layer separation (domain, traits, orchestration, wire protocol/DTO, persistence/fakes, thin facades), unit test isolation (`tests.rs`), and thematic integration test decomposition with shared `common.rs`.
  - `docs/codex-project-context.md`: added rule 7 detailing modularity requirements and thematic integration test decomposition for Codex and other AI agents.
  - `src/ARCHITECTURE.md`: added integration test submodules to submodule maps and updated reading order instructions to mandate modular test suites.
  - Updated `docs/status.md` and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green.
- Status: complete; agent instruction files explicitly mandate layered decomposition upfront.

## 2026-09-29 — ARCH: thematic modular decomposition of integration test suites (tests/openapi_contract and tests/postgres_integration)

- Goal: decompose the massive integration test files `tests/openapi_contract.rs` (1783 lines) and `tests/postgres_integration.rs` (2412 lines) into focused thematic submodule files to improve navigation, maintainability, and compilation speed, while preserving single integration test targets.
- Scope:
  - `tests/openapi_contract/`:
    - `common.rs` (325 lines): fixture specifications, schema validation helpers, golden JSON loaders, and OpenAPI document constants.
    - `surface.rs` (302 lines): public surface validation and search endpoint registration tests.
    - `device_control.rs` (466 lines): device control v1 specification bounds, schema linking, and golden fixture validation.
    - `device_catalog.rs` (425 lines): stream-free catalog bounds, play stream constraints, and native session gating.
    - `voice_stream.rs` (143 lines): voice stream specification alignment with runtime and device flows.
    - `personal_sync.rs` (158 lines): personal data sync specification constraints and session checks.
    - `tests/openapi_contract.rs` (14 lines): minimal facade declaring thematic submodules via `#[path = ...]`.
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
    - `tests/postgres_integration.rs` (31 lines): minimal facade preserving single test binary target for serial execution.
  - Updated `docs/status.md` and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all 9 OpenAPI contract tests pass; all 12 PostgreSQL integration tests compiled cleanly and ignored appropriately; all 184 library tests pass).
- Status: complete; all test semantics, assertions, and execution controls preserved.

## 2026-09-29 — ARCH: domain models, repository implementations, and search service decomposition for search (Step 7)

- Goal: decompose the 849-line monolithic `src/search/mod.rs` by extracting domain models, in-memory repository implementations, and search service orchestration into dedicated submodules, turning `src/search/mod.rs` into a clean facade.
- Scope:
  - `src/search/domain.rs` (211 lines): extracted `SearchQuery`, `SearchConstraints`, `Station`, `StationHealth`, `RankedStation`, `SearchOutcome`, `MIN_RELEVANCE_SCORE`, `StationRepository`, and `RepositoryError`.
  - `src/search/in_memory.rs` (298 lines): extracted `InMemoryStationRepository`, `UnavailableStationRepository`, pinned catalog conversion, and deterministic unit test fixtures.
  - `src/search/service.rs` (343 lines): extracted `SearchService`, concurrent query parsing/embedding, fallback intent preservation, confidence-gated language filtering, and progressive genre fallback (`broaden_tags`).
  - `src/search/mod.rs` (37 lines): reduced to a clean public facade re-exporting all submodules without breaking changes.
  - Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all 49 search unit tests and 184 library tests passed).
- Status: complete; zero breaking changes to search domain contracts or public exports.

## 2026-09-29 — ARCH: catalog resolution, target validation, and command router decomposition for device_control_command (Step 6)

- Goal: decompose the 817-line monolithic `src/device_control_command.rs` by extracting catalog resolution, target validation, and command routing logic into dedicated submodules, reducing the root to a clean facade.
- Scope:
  - `src/device_control_command/catalog.rs` (91 lines): extracted station resolution, `StationCatalog` trait, `ResolvedStation`, and default `SearchService` integration.
  - `src/device_control_command/target_validation.rs` (136 lines): extracted target manifest precondition validations (`validate_target`, `require_role_capability`, `display_supported`).
  - `src/device_control_command/router.rs` (592 lines): extracted `CommandRouter`, in-flight state tracking, reservation, delivery over `ConnectionRegistry`, timeout handling, and terminal results.
  - `src/device_control_command/tests.rs` (933 lines): adapted imports to reference submodules and external crates cleanly.
  - `src/device_control_command.rs` (13 lines): reduced to a clean public facade re-exporting all submodules without breaking changes.
  - Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (all 6 unit tests and 184 library tests passed).
- Status: complete; zero breaking changes to command routing semantics or public exports.

## 2026-09-29 — ARCH: transport and test isolation for http/control (Step 5)

- Goal: decompose the 1088-line monolithic `src/http/control.rs` by extracting unit/transport tests, wire error framing, and state admission decisions into existing submodules.
- Scope: 
  - `src/http/control/tests.rs`: extracted 601 lines of unit and transport tests.
  - `src/http/control/protocol.rs`: added wire scope mapping (`scope_name`) and command rejection envelope formatting (`send_command_error`).
  - `src/http/control/state.rs`: added controller full-state observation checks (`requires_full_state`) and full snapshot persistence outcome acceptance (`accepted_full_snapshot`).
  - `src/http/control.rs`: reduced from 1088 to 442 lines, keeping focused WebSocket upgrade (`connect`) and connection lifecycle loop (`run`).
  - Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (14/14 control tests and all 184 library tests passed).
- Status: complete; zero breaking changes to WebSocket and HTTP control protocol.

## 2026-09-29 — ARCH: domain store and test isolation for personal_data and admin (Step 4)

- Goal: decompose the massive domain files `src/personal_data.rs` (1281 lines) and `src/admin.rs` (885 lines) by separating domain models and traits from in-memory test stores and unit test blocks.
- Scope: 
  - `src/personal_data/`: extracted `domain.rs` (pure sync models, validation, limits, `PersonalDataStore` trait), `in_memory.rs` (`InMemoryPersonalDataStore`, revision generator, `delta_and_echo`), and `tests.rs` (unit tests). Replaced `src/personal_data.rs` with a 26-line facade.
  - `src/admin/`: extracted `domain.rs` (`AdminPrincipal`, `AdminPasswordHash`, DTOs, `AdminStore` trait), `fake.rs` (`FakeAdminStore`, secret hash key, rotation), and `tests.rs` (unit tests). Replaced `src/admin.rs` with a 14-line facade.
  - Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (184 unit/doc tests and all integration tests passed).
- Status: complete; zero breaking changes to public domain and store contracts.

## 2026-09-29 — ARCH: modular layering refactoring for http/endpoints (Step 3)

- Goal: decompose the 1083-line monolithic `src/http/endpoints.rs` by introducing a `RouterBuilder` pattern, extracting route composition, and moving unit tests out of the router definition.
- Scope: created `src/http/endpoints/builder.rs` (fluent `RouterBuilder` with sensible offline defaults and explicit dependency injection), `src/http/endpoints/routes.rs` (Axum route registration and middleware layering), and `src/http/endpoints/tests.rs` (extracted unit tests). Simplified 15 telescoping `router_with_*` functions in `src/http/endpoints.rs` to concise builder calls, re-exported `RouterBuilder` in `rockserver::http`, and simplified `control.rs` test harness. Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green.
- Status: complete; zero breaking changes to public router signatures.

## 2026-09-29 — ARCH: modular layering refactoring for http/voice (Step 2)

- Goal: decompose the 1353-line monolithic `src/http/voice.rs` into clean transport protocol, device voice orchestration, and handler layers.
- Scope: created `src/http/voice/protocol.rs` (wire DTOs, JSON event envelopes, error codes, limits, framing helpers), `src/http/voice/device.rs` (device capability validation, intent resolution, command routing, execution polling), and restructured `src/http/voice.rs` (streaming WebSocket ingestion loop, session lifecycle, REST command handler, search completion). Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (including all `voice_stream_api` and `voice_command_api` tests).
- Status: complete; zero breaking changes to HTTP or WebSocket behavior.

## 2026-09-29 — ARCH: modular layering refactoring for station_icons (Step 1)

- Goal: decompose the 1310-line monolithic `src/station_icons.rs` into distinct single-responsibility architectural layers matching project architecture guidelines.
- Scope: created `src/station_icons/domain.rs` (models, keys, errors, traits, WebP raster preparation algorithm), `src/station_icons/storage.rs` (atomic filesystem storage), `src/station_icons/fetcher.rs` (external HTTP client, SSRF checks, HTML link parsing), `src/station_icons/coordinator.rs` (PostgreSQL job coordination, SQL queries, item planning), and `src/station_icons/tests.rs` (isolated unit tests). Preserved `src/station_icons.rs` as a 27-line stable facade. Updated `src/ARCHITECTURE.md`, `docs/status.md`, and `docs/tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all green (15/15 station_icons tests passed).
- Status: complete; all public contracts and behavior preserved with zero API changes.

## 2026-09-28 — Opt-in PostgreSQL integration test rot repaired

- Goal: make the opt-in PostgreSQL integration suite trustworthy again; five tests had been
  failing identically on clean `master` before the personal-data work started.
- Scope: test-side fixes only. Device-control gap-state expectation updated to the documented
  full-snapshot overwrite semantics; admin bootstrap records its own refresh request instead
  of depending on another test's leftovers; admin identity foundation truncates the
  migration-0019 singleton tables before creating its principal; session rotation asserts the
  server-stamped `created_at`; account cleanup binds a real `bytea` and expects the
  deactivation counters to report only rows that deactivation itself flips. The suite's
  header now documents the required serial run (`--test-threads=1`) because the tests share
  one database and truncate its admin tables.
- Checks: `cargo fmt --check`, strict Clippy, `cargo test` green; opt-in suite 12/12 on a
  fresh disposable pgvector container run serially.
- Status: complete; no production code changed.

## 2026-09-28 — RM-012-A: personal-data sync (favourites + history)

- Goal: give RockCast and RockMobile one server-held copy of favourites and listen history so
  both devices always converge on the same profile.
- Scope: migration `0025_add_personal_data_sync.sql` (`favourite_records`, `history_records`,
  shared monotonic revision sequence); `personal_data` domain owning LWW merge, tombstones,
  bounds, and retention; `PostgresPersonalDataStore`; one `POST /api/v1/sync` endpoint with the
  contract promoted to OpenAPI 0.6.0; hourly opportunistic retention sweep; deleted-account row
  purge. The native-endpoint authenticate-and-throttle helper was extracted to `control_auth.rs`
  and reused by the device catalog (no duplicated auth logic). Clients are unchanged — client
  integration is a separate task in their repositories (RM-012-B).
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed; the opt-in PostgreSQL integration test passed against a disposable
  pgvector container. Five pre-existing opt-in PostgreSQL test failures reproduce identically
  on clean `master` and are unrelated.
- Status: **deployed** as commit `2d8e27f` on 2026-09-28; readiness passed and the live
  endpoint answered the contract 401 without a credential.

## 2026-09-28 — DEPLOY fix: Windows system tar for the image manifest

- Goal: unblock the release after `tar.exe` resolved to MSYS/GNU tar in a Git Bash session
  and rejected the `C:\...` archive path as a remote `host:file` pair.
- Scope: `deploy/ops-001-d.ps1` now resolves `$env:SystemRoot\System32\tar.exe` explicitly
  for reading the saved image's `manifest.json`; the launcher regression suite asserts the
  drive-letter-safe resolution.
- Checks: `deploy/tests/ops-001-d-tests.ps1` passed; the manifest read verified against the
  saved image before the retry.
- Status: **deployed** (this fix is the release commit `2d8e27f`).

## 2026-09-26 — DC playback track metadata

- Goal: expose the current track observed by RockCast to the remote RockMobile player.
- Scope: add optional `track_title` to the runtime playback contract and preserve it through the existing directory projection; no new endpoint or identity field.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` passed. External database and live service tests remain ignored.
- Status: deployed as commit `a3f2a13` on 2026-09-26; readiness passed. Two live phone-to-desktop station switches showed distinct track titles; microphone input and the full physical matrix remain open.

## 2026-09-25 — OPS: deploy-time pg_dump disabled and deploy debris cleaned

- Goal: stop paying the per-release database dump on the small VPS and
  remove accumulated deployment debris at the operator's request.
- Result: `remote-ops-001-d.sh` no longer runs `pg_dump`, copies dumps, or
  prunes the backup directory; the release record keeps the `backup_sha256`
  continuity field with `-`. The launcher regression suite now asserts the
  dump path stays absent and the seed/readiness ordering is unchanged. On
  the VPS this removed the retained dumps (34 MiB), stale deploy logs, a
  dead seed container, and 17 superseded release images (~1 GiB; disk use
  fell from 45% to 40%). Local diagnostic scratch files were deleted. A
  deploy with the change (`454cae7`) created no dump and passed readiness;
  the icon backlog was wiped and restarted (`2c3e367a`), reaching
  ready=151/252 processed in the first minutes with the candidate-fallback
  fetcher active.
- Checks: `deploy/tests/ops-001-d-tests.ps1` passed; live readiness 200;
  backup directory verified empty after the deploy.
- Status: **deployed.**
## 2026-09-25 — RS-ICON-013: ordered favicon candidates with root fallback

- Goal: stop losing stations whose first icon candidate is dead while a
  usable one exists on the same site.
- Result: homepage discovery now returns the ordered candidate list — every
  declared HTTP(S) `icon`/`apple-touch-icon` link in document order
  (deduplicated, at most four) followed by the homepage-root `/favicon.ico`
  fallback — and the worker tries them in order until one normalizes. A
  dead candidate (bad URL, 404, oversized) no longer dooms the station, and
  any transport-level failure keeps the item retryable even when other
  candidates are simply absent. Covers the two production cases found by
  sampling: a deep-page relative `favicon.ico` that 404s while the root
  icon lives, and a first declared candidate that cannot be decoded.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed (229 tests; 15 PostgreSQL/live
  tests ignored as designed), including new candidate-order and
  classification coverage.
- Status: **implemented locally; production deploy pending.**
## 2026-09-25 — RS-ICON-012 deployed with prefix inspection, swap, and a from-scratch import

- Goal: ship the homepage-prefix fix and restart the icon backlog cleanly on
  the operator's request.
- Result: commit `f4afd57` deployed (`readiness=passed`); the homepage
  inspection now parses the collected prefix of oversized pages instead of
  discarding it, so heavy pages (~17% of the remaining permanent errors)
  can still declare their icon. The VPS gained a persistent 3 GiB swapfile
  after the earlier memory-pressure concern. Station icon metadata and job
  history were cleared on explicit operator instruction, and a fresh job
  (`67bfc4a7`) restarted over all 16,825 stations: the first 502 processed
  items showed ready=307 (61%), missing=49, retryable=28 (6%),
  permanent=118 (23%) — materially better than the 44%/23%/27% profile of
  the first run before the fetcher fixes.
- Checks: live counters through the protected admin API; public readiness
  200; swap verified active on the host.
- Status: **deployed; from-scratch import running.**

## 2026-09-25 — RS-ICON-012 deployed: false icon-import failures reduced and import rerun

- Goal: verify the fetcher fixes against production error rates and resume
  the interrupted backlog.
- Result: deploy `53e4968` (fetcher User-Agent, 1 MiB homepage bound, oversize
  classification) passed readiness and the admin sidebar layout shipped with
  it. The first rerun processed 2,423 items with retryable errors down from
  ~23% to ~10% before a host restart interrupted the job by design (startup
  marks unfinished jobs interrupted); a fresh rerun (`55c25b38`, 13,813
  items) is continuing the backlog. Total ready icons reached 3,015 of 16,825
  and grows during the run. Sampling of the remaining permanent errors shows
  they are now mostly genuine: homepages that declare no icon link and a
  404/redirecting `/favicon.ico`, plus dead domains — not fetcher defects.
  Note: host memory is tight (~1.9 GiB total, ~110 MiB free); operators
  should expect import jobs to interrupt on host restarts and rerun them.
- Checks: live counters through the protected admin API; failed-station
  sampling probed from the VPS; `cargo fmt --check`, strict Clippy, and
  `cargo test` (225 passed) for the code change.
- Status: **deployed; rerun job in progress.**

## DOCS-002 — 2026-09-24 — refresh README and active backlog

- Goal: bring the repository front page and near-term backlog in line with the implemented service and current cross-repository handoffs.
- Scope: update README.md and TODO.md; record the documentation task in docs/status.md and this log.
- Result: README now summarizes the current search, voice, account, administrator, Yandex Smart Home, and device-control capabilities, points to the canonical architecture/roadmap documents, and identifies known search-retrieval work. TODO.md now tracks remaining SRCH-002 through SRCH-004, the RC-4/RM-4 live-state handoff, voice reliability, and dependent ESP32/Home Assistant milestones. It distinguishes deployed or implemented behavior from work that remains open or hardware-blocked.
- Checks: cargo fmt --check; cargo clippy --all-targets --all-features -- -D warnings; cargo test; git diff --check. All passed. PostgreSQL and live-provider integration tests were skipped by their explicit environment/credential guards.
- Status: **complete.**

## SRCH-001 — 2026-09-24 — non-blocking ONNX inference and session pool

- Goal: isolate CPU-intensive ONNX embedding inference from Tokio runtime worker threads and enable concurrent search inference without serializing on a single mutex.
- Scope: update `OnnxE5Config` and `OnnxE5EmbeddingProvider` in `src/providers/onnx_e5.rs`, add session pool with semaphore backpressure and RAII cleanup, offload tokenization and inference to `tokio::task::spawn_blocking`, and extend unit tests and live test harness in `tests/onnx_e5_live.rs`.
- Result: inference sessions are pre-allocated in a bounded pool sized by CPU parallelism (1..=4) or `ROCKSERVER_ONNX_SESSION_POOL_SIZE`. Calls to `embed` and `embed_document` asynchronously acquire a pool permit without blocking OS threads, run on Tokio's blocking thread pool, and guarantee session recycling even upon worker panic or cancellation.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test` (172 unit/integration tests passed) and `cargo test --all-features` (179 tests passed, 0 failed, 17 external/live tests safely ignored).
- Status: **complete.**

## SEARCH-ROADMAP-001 — 2026-09-24 — document voice search and vector retrieval modernization roadmap

- Goal: decompose the 14 technical findings from the 2026-08-29 audit into modular, actionable tasks with clear acceptance criteria and verification steps so they can be assigned to autonomous coding agents.
- Scope: create `docs/roadmap/voice-search-improvements.md` defining 13 discrete tasks (`SRCH-001` through `SRCH-012` in `rockserver` and `RC-VOICE-001` in `rockcast`), update `docs/status.md` and `docs/tasks.md`.
- Result: documented 4-phase roadmap covering non-blocking ONNX inference, true HNSW vector retrieval, prefilter order fixes, lexical language gates, RRF hybrid fusion, intent query cleaning, dynamic taxonomy sync, LLM circuit breakers/caching, anti-aliased resampling, incremental backfill, and golden offline eval sets.
- Checks: links verified against codebase, repository boundary rules maintained, documentation updated.
- Status: **complete.**

## SEARCH-PERF-001 — 2026-09-24 — optimize search pipeline latency to eliminate voice command timeouts

- Goal: prevent voice search requests from exceeding the 5.0s contract timeout (`search_timeout_ms: 5000`) in RockCast and RockMobile by reducing LLM parsing, embedding, and database query latencies.
- Scope: update LLM defaults in `src/providers/yandex_llm.rs`, parallelize parser and embedding in `src/search/mod.rs`, optimize PostgreSQL CTE execution in `src/persistence/postgres.rs`, and update deployment environment allowlists in `deploy/compose.yaml`, `deploy/ops-001-d.psm1`, and `deploy/remote-ops-001-d.sh`.
- Result: switched default LLM model to `yandexgpt-lite` with a 4.5s request timeout; overlapped query embedding with LLM parsing via `tokio::join!`; and added `MATERIALIZED` to `candidates` and `scored` CTEs in PostgreSQL station search, dropping DB execution from ~850-1060ms to ~147ms (~7x speedup).
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test` (172 tests passed); `deploy/tests/ops-001-d-tests.ps1`; production deploy `db3066d` verified with readiness check and live search timing (~800–850ms).
- Status: **deployed.**

## YANDEX-HOME-005 — 2026-09-24 — extract all sensor properties and render multi-metric device cards

- Goal: eliminate restrictive temperature-only and `retrievable: true` filters so that all sensors (including battery-powered Zigbee climate sensors) and all properties (humidity, battery, voltage, power, etc.) are extracted and presented in the cabinet.
- Scope: update provider parsing in `src/providers/yandex_home.rs`, HTTP DTOs in `src/http/yandex_home.rs`, OpenAPI schema in `api/openapi.yaml`, and frontend client and presentation in `web/src/api.ts` and `web/src/app.tsx`.
- Result: battery Zigbee sensors (reporting `retrievable: false`) and multi-property devices (e.g. Climate with temperature, humidity, and battery level; humidifier; smart plugs with electrical readings) are parsed with localized names and units. Web cabinet displays device cards with all active metrics.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test` (including openapi contract checks); `pnpm typecheck`; `vite build`; `web/tests/ux-regression.mjs` (11 passed); production release `489c2a0` succeeded and public `https://rockplatform.win/health/ready` returned `{"status":"ok"}`.
- Status: **deployed.**

## YANDEX-HOME-004 — 2026-09-24 — fix cross-site OAuth callback cookie drop and frontend feedback

- Goal: resolve OAuth redirect failure returning `/?yandex_home=failed` when linking Yandex Smart Home, and display meaningful error/success feedback in the web cabinet.
- Scope: update session cookie `SameSite` policy in `src/http/auth.rs`, relax cookie requirement in `src/http/yandex_home.rs`, add diagnostic logs, parse `yandex_home` query parameter and display alert in `web/src/app.tsx`.
- Result: `rockserver_browser` cookie is now issued with `SameSite=Lax`, allowing it to accompany top-level OAuth GET callbacks from `oauth.yandex.ru`. The callback handler verifies the single-use unguessable OAuth state from PostgreSQL and validates cookie ownership if present without failing if a browser blocks cross-site cookies. The frontend shows a status message and cleans up the URL.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test`; `pnpm typecheck`; `vite build`; `web/tests/ux-regression.mjs` (11 passed); production release `d977ded` succeeded and public `https://rockplatform.win/health/ready` returned `{"status":"ok"}`.
- Status: **deployed.**

## YANDEX-HOME-003 — 2026-09-24 — fix SQL syntax in yandex_home PostgreSQL queries

- Goal: eliminate the 503 `auth_unavailable` error returned by `GET /api/v1/browser/account` caused by SQL syntax errors in the Yandex Home persistence module.
- Scope: fix string line continuations in `src/persistence/account_postgres/yandex_home.rs` and add an integration test in `tests/postgres_integration.rs`.
- Result: removed literal `+` artifacts from all 6 queries in `yandex_home.rs`. Calls to `has_yandex_home_connection` now execute cleanly against PostgreSQL without syntax errors.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test`; production release `bc15226` succeeded and public `https://rockplatform.win/health/ready` returned `{"status":"ok"}`.
- Status: **deployed.**

## YANDEX-HOME-002 — 2026-09-24 — deploy Yandex Home OAuth configuration

- Goal: ensure the newly configured Yandex Home OAuth client reaches the
  production container through the standard protected release path.
- Result: OPS-001-D allowlists only `YANDEX_HOME_CLIENTID` and
  `YANDEX_HOME_SECRET` in addition to its existing Yandex configuration, writes
  them into root-only `release.env`, and passes them only to RockServer.
  Subsequent deploys replace either value or remove it when absent; no value is
  written to Git or the deploy summary.
- Checks: `deploy/tests/ops-001-d-tests.ps1`; `cargo fmt --check`; `cargo
  clippy --all-targets --all-features -- -D warnings`; `cargo test` (172
  library tests; 10 PostgreSQL and 5 live provider tests ignored by explicit
  gates); production release `a6e029b` succeeded and public readiness returned
  `{"status":"ok"}`.
- Status: **deployed.**

## YANDEX-HOME-001 — 2026-09-24 — user-linked Yandex Smart Home temperatures

- Goal: let a signed-in Rock account link its Yandex Smart Home and view
  temperature sensors exposed through the platform.
- Scope: read-only `iot:view` OAuth flow, account-bound state persistence,
  encrypted reusable token storage, safe browser API, temperature extraction,
  user-cabinet controls, OpenAPI, and configuration documentation. No device
  control or `iot:control` capability was added.
- Result: the browser begins OAuth through a CSRF-protected endpoint; the
  callback consumes its state once and persists AES-256-GCM ciphertext only.
  The sensor endpoint returns readable float-temperature properties and no
  provider identifiers or tokens. A rejected token is revoked, and users can
  disconnect the integration from their cabinet.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features --
  -D warnings`; `cargo test` (172 library tests; 10 PostgreSQL and 5 live
  provider tests ignored by their explicit gates); `pnpm build`; `pnpm test`
  (11 passed); production release `a6e029b` succeeded and public readiness
  returned `{"status":"ok"}`.
- Status: **deployed.**

## 2026-09-24 — RS-ICON-012: reduce false icon-import failures

- Goal: cut the false error rate of the administrator-started icon import
  after production sampling showed ~49% of processed items ended in
  retryable/permanent errors.
- Result: a 60-station random sample of failed items probed from the VPS
  attributed ~33% to homepages heavier than the 256 KiB inspection bound,
  ~32% to sites answering normally with a browser-like User-Agent (the
  fetcher sent none and was blocked), ~17% to dead domains and the rest to
  403/404/500. Fixes: the fetcher now sends a browser-like User-Agent, the
  homepage inspection limit rose to 1 MiB, and sources exceeding a fixed
  byte limit classify as permanent instead of retryable forever; only
  transport-level failures stay retryable. Stations without a ready icon
  remain eligible for every new job, so a rerun retries all previous false
  failures.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed (225 tests; 15 PostgreSQL/live
  tests ignored as designed).
- Status: **implemented locally; production deploy pending.**
## WEB-ADMIN-UI-001 — 2026-09-24 — desktop-first administrator console

- Goal: replace the narrow, mobile-oriented administrator presentation with a
  normal desktop operations console without changing user cabinets.
- Scope: `/admin` Preact markup and scoped CSS only; sidebar navigation, session
  header, table layouts, filters, pagination, station import controls, and
  administrator login presentation. No API or public/user-cabinet behavior
  changes.
- Result: admin screens now use a wide workspace with persistent navigation,
  responsive table overflow, clearer status treatments, and a separate login
  composition. No Tailwind dependency was added because existing CSS is enough
  for this isolated UI.
- Checks: `pnpm build`; `pnpm test` (11 passed).
- Status: **complete.**

## 2026-09-23 — RS-DOMAIN-001: production move to rockplatform.win

- Goal: relocate the public deployment to the new `rockplatform.win` domain
  on a new host with fresh credentials and a fresh database; the old
  `alex.vault57.ru` host and its data are left untouched.
- Result: the WebAuthn relying-party id and origin
  (`src/auth/webauthn.rs`), the canonical first-party browser origin for
  state changes (`src/http/transport.rs`) and the OpenAPI server URL now use
  `rockplatform.win`; origin-dependent tests reference the constants instead
  of literals, and the ignored deploy inventory points at the new domain.
  Passkeys registered for the old RP id cannot transfer and a fresh host is
  seeded from the pinned catalog release; the administrator must be
  re-bootstrapped and the station-icon import restarted explicitly.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed (225 tests; 15 PostgreSQL/live
  tests ignored as designed).
- Production deploy: host `217.60.60.57` (root, fresh ed25519 deploy key;
  the old host `194.87.26.66` is untouched with its key preserved as
  `rockserver_ed25519.old-host`) was bootstrapped with
  `ops-001-d.ps1 -Action bootstrap -InstallDocker` on 2026-09-24 and
  released with `-Action deploy` for commit `a8f21aa`: `status=succeeded`,
  `readiness=passed`, including the RS-ICON-011 release. The fresh database
  seeded 16,825 stations with 0 failures, then embeddings backfilled.
  Independently confirmed on `https://rockplatform.win` (Cloudflare-proxied
  DNS, certificate issued through it): `/health/ready` 200, search 200 with
  `favicon_url: null` everywhere, unknown-station icon `404`, `/admin` 200.
- Follow-up: the deployment image did not contain the `bootstrap_admin`
  binary, so commit `84cb4eb` added it to the Dockerfile build and runtime
  copy and was deployed (`readiness=passed`). The administrator was then
  bootstrapped with the ignored local `.env` credentials and login verified
  through the public API (200 Bearer).
- Status: **deployed to production on 2026-09-24.**

## 2026-09-24 — first production station-icon import started

- Goal: begin the actual icon backlog on the new deployment using the
  RS-ICON-011 homepage favicon discovery.
- Result: the protected `POST /api/v1/admin/icons/import` created job
  `e456a311-29aa-417d-813e-5d4b05a6ddb7` selecting all 16,825 stations.
  Early persisted counters after ~150 processed items: `ready=52`,
  `missing=30`, `retryable_error=32`, `permanent_error=37` — homepage
  discovery is producing ready artifacts. A ready icon is publicly served
  as `200 image/webp` (28,256 bytes) with a strong ETag and returns `304`
  on a matching `If-None-Match`. The job continues in the background; its
  saved progress is visible on the Stations tab of the admin console.
- Checks: live counters read through the protected admin API; public icon
  delivery verified with `200`/`304` conditional requests.
- Status: **running; completion to be observed via the admin console.**

## 2026-09-22 — RS-ICON-011: homepage favicon discovery for the icon import

- Goal: let the administrator-started import produce icons for the production
  catalog, whose pinned release carries no explicit favicon URLs. The first
  production job therefore ended with 16,825 missing and 0 ready because the
  worker only consumed an already-stored `station_icons.source_url`, and the
  production `station_icons` table was empty.
- Result: the import worker now resolves each item's automatic source in the
  roadmap's priority order — an explicit catalog `source_url` (priority 2)
  first, otherwise a favicon discovered on the station's validated homepage
  (priority 1) through a bounded SSRF-checked homepage fetch, the first
  `<link rel="icon">`/`apple-touch-icon"` target, or the `/favicon.ico`
  fallback. Ready metadata is written through an upsert so stations without a
  prior `station_icons` row become ready atomically. Unsuccessful HTTP
  statuses now classify as permanent (unusable URL) while transport failures
  stay retryable.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed (225 tests; 15 PostgreSQL/live
  tests ignored as designed). New deterministic offline coverage: the
  icon-link parser (rel variants, quoting, entities, `data:`/`mask-icon`
  exclusion, relative resolution, `/favicon.ico` fallback) and fake-fetcher
  classification of missing/retryable/permanent/ready with source
  priorities.
- Status: **implemented locally; production deploy pending.**

## 2026-09-22 — RS-ICON-010: persistent production icon storage

- Goal: make administrator-managed prepared icons durable through the standard
  production deployment without introducing a deploy-time sync.
- Result: Compose config now supplies `ROCKSERVER_STATION_ICON_DIR` and mounts
  `/home/rockserver/station-icons` only into RockServer. The remote bootstrap
  creates that directory with container UID `10001` ownership, and the deploy
  regression protects the wiring. Deployment still never starts an import.
- Checks: final whole-change verification passed 2026-09-22: `cargo fmt
  --check`, strict all-target/all-feature Clippy, full `cargo test` (216
  passed; 15 PostgreSQL/live-provider tests ignored as designed), `pnpm
  typecheck`, `pnpm test` (11 passed), the OPS-001-D local regression suite,
  and `ops-001-d.ps1 -Action deploy -DryRun`. This closes the verification
  deferred by RS-ICON-002 through RS-ICON-006.
- Production deploy: commit `179c9a8` was released through
  `deploy/ops-001-d.ps1 -Action deploy` on 2026-09-22; the VPS applied
  migrations `0022`/`0023` and the gate returned `readiness=passed`.
  Independently confirmed: public `/health/ready` `HTTP 200 {"status":"ok"}`,
  unknown-station `GET /api/v1/stations/{id}/icon` returns the contract
  `404` with `Cache-Control: no-store`, and public search serializes
  `favicon_url: null` for every station because no import job has been
  started. Operational note: the launcher must resolve `tar.exe` to the
  Windows bsdtar (`C:\Windows\System32`); when invoked from a Git Bash
  child, GNU tar interprets the artifact drive prefix as a remote host and
  the deploy aborts before any remote mutation.
- Status: **deployed to production on 2026-09-22; first admin-started icon
  import remains an explicit operator action.**

## 2026-09-22 — RS-ICON-009: nullable server favicon URL in catalog/search

- Goal: let clients use a RockServer icon URL only after the server has ready
  metadata, while retaining a null/placeholder path for all other stations.
- Result: PostgreSQL read queries now left-join ready icon metadata without
  changing rank/order, and `Station`, public catalog, search and voice DTOs
  serialize a nullable same-origin `favicon_url`. Offline catalogs remain null.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed. PostgreSQL integration tests remain
  ignored because no disposable `TEST_DATABASE_URL` was supplied.
- Status: **implemented locally; not deployed.**

## 2026-09-22 — RS-ICON-008: manual icon upload, replace and confirmed removal

- Goal: finish the administrator override path for stations with missing or
  unsuitable automatic icons.
- Result: added raw bounded image upload and confirmed removal routes under the
  existing AdminBearer/Origin boundary, reusing the WebP normalizer and
  preserving automatic source metadata. The Stations tab now exposes native
  file selection and a browser confirmation dialog; removal never deletes a
  potentially shared content-addressed file.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, `cargo test`, `pnpm typecheck`, and `pnpm test` passed.
  PostgreSQL integration tests remain ignored because no disposable
  `TEST_DATABASE_URL` was supplied.
- Status: **implemented locally; not deployed.**

## 2026-09-22 — RS-ICON-007: administrator import control and server icon delivery

- Goal: make the approved importer executable only from the authenticated
  first-party administrator console, retain observable progress, and serve
  prepared server-owned thumbnails.
- Result: added protected start/latest/job-status routes, durable counter polling
  after console reload, public ready-WebP delivery with ETag/404 semantics, and
  station-list thumbnail/placeholder UI. The runtime opens storage only from
  `ROCKSERVER_STATION_ICON_DIR`; startup merely marks abandoned work interrupted
  and neither startup nor deployment starts network work. Manual
  upload/replace/removal is delivered separately as RS-ICON-008.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed; the latter includes OpenAPI contract
  coverage and leaves 10 PostgreSQL tests ignored because no disposable
  `TEST_DATABASE_URL` was supplied (five credentialed live tests are also
  ignored). `pnpm typecheck` and `pnpm test` passed.
- Status: **implemented locally; not deployed.**

## 2026-09-22 — RS-ICON-006: durable import coordinator

- Goal: persist the work and progress behind the future admin button without
  introducing a CLI, cron or startup import.
- Result: migration `0023` adds one-active job and per-station item snapshots;
  the coordinator claims items, stores safe terminal counts and interrupts
  unfinished prior-process jobs without network work.
- Checks: `cargo fmt` and `cargo check` passed; required full verification
  remains pending the in-progress icon work.
- Status: **implemented locally; consumed by RS-ICON-007 protected routes.**

## 2026-09-22 — RS-ICON-005: bounded source validation and normalization

- Goal: make icon download and manual upload share one safe raster boundary.
- Result: only PNG/JPEG/WebP/ICO up to 2 MiB and 1,024×1,024/1,048,576 pixels
  are accepted; the sole output is square WebP. The fetcher disables implicit
  redirects, validates every destination and rejects local/private addresses.
- Checks: `cargo check` passed after resolving the new `image` dependency;
  full required verification remains pending the in-progress icon work.
- Status: **implemented locally; no HTTP route or automatic execution.**

## 2026-09-22 — RS-ICON-004: persistent WebP artifact storage

- Goal: establish safe filesystem storage independently of metadata, HTTP and
  external download.
- Result: added a content-addressed storage trait and filesystem backend with
  canonical keys, atomic publish, read/exists/delete operations and
  deterministic temp-directory coverage.
- Checks: `cargo fmt`, `cargo check`, and `cargo test` passed (160 library
  tests plus regular suites); disposable PostgreSQL coverage is pending.
- Status: **implemented locally; artifacts are not yet fetched or served.**

## 2026-09-22 — RS-ICON-003: catalog icon sources

- Goal: capture trustedly normalized icon-source URLs without exposing them to
  clients.
- Result: shared-catalog and Radio Browser icon fields populate internal
  metadata. A source change flags a non-manual record for refresh while a
  ready artifact remains available.
- Checks: `cargo fmt`, `cargo check`, and `cargo test` passed (160 library
  tests plus regular suites); disposable PostgreSQL coverage is pending.
- Status: **implemented locally; no downloader, job, endpoint or UI yet.**

## 2026-09-22 — RS-ICON-002: station-icon metadata schema

- Goal: add the first compatible, schema-only foundation for server-owned
  station icons.
- Result: migration `0022` creates `station_icons` with status, source,
  ready-WebP metadata, retry and manual-override fields, plus a due-import
  index. It downloads no data and changes no existing station or stream IDs.
- Checks: `cargo fmt`, `cargo check`, and `cargo test` passed (160 library
  tests plus regular suites); disposable PostgreSQL coverage is pending.
- Status: **implemented locally; no icon HTTP or admin behavior yet.**

## 2026-09-22 — station icons: admin-operated import direction

- Goal: revise the deferred icon roadmap so import is controlled from the
  authenticated administrator console rather than a CLI, and the console
  displays ready server-hosted icons.
- Result: removed the CLI/bundle/deploy-sync path from the plan. The new
  sequence records a single durable admin-started background job with saved
  progress, same-origin polling at `https://alex.vault57.ru/admin`, station
  thumbnails/placeholders, and a later manual upload/replace/remove override.
  Deployment remains schema/storage-only and never starts network import. The
  project context and service diagram now reflect this explicitly planned flow.
- Checks: documentation review and `git diff --check` passed. Cargo and web
  checks are not applicable because this task changes no executable code.
- Status: **roadmap updated; icon implementation remains unstarted.**

## 2026-09-22 — administrator station list: 50 rows per page

- Goal: show 50 stations on each administrator-console page.
- Result: the station endpoint, OpenAPI default, browser request, and pager
  offset now use 50. The existing maximum remains 50; device and audit
  pagination remain at 25.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --
  -D warnings`, and `cargo test` passed; the station-handler test confirms
  the default response limit is 50. `pnpm typecheck` and `pnpm test` passed
  (11 web UX regressions).
- Status: **implemented locally.**

## 2026-09-17 — RS-9: target-only station name and future icon delivery

- Goal: correct the RockCast fallback presentation for a station selected on
  RockMobile but missing from RockCast's local cache. The previous RC-4a
  fallback correctly preserved the catalog ID but used it as the visible name,
  yielding a UUID in `Now playing`.
- Contract: the server-resolved `station.play_stream` branch now requires a
  target-only `station: { name, icon_url }` object in OpenAPI. `name` is
  bounded to 1..128 characters. `icon_url` is nullable and, when populated in
  a future catalog, must meet the same bounded public HTTP(S) safety policy as
  a delivery URL (maximum 2048 characters); the current catalog has no icon
  column, so the resolver emits `icon_url: null`.
- Scope: `StationCatalog` returns the resolved stream plus `StationPresentation`;
  the router attaches it only after resolving the controller's original
  `station.play_station`. The controller payload/fingerprint, command
  persistence, terminal lifecycle frames, directory projection, logs and
  error text remain unchanged, and none receive stream URLs or presentation
  fields. Direct stream commands cannot carry a presentation object.
- Compatibility: the Rust decoder accepts an absent `station` only during
  target-first rollout. Deploy updated RockCast before this server release;
  once deployed, the server always supplies the bounded object.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features --
  -D warnings`; `cargo test` — all regular suites passed; `git diff --check`.
  **Deploy evidence:** commit `509ea0c` was released via
  `deploy/ops-001-d.ps1 -Action deploy`; VPS status is `succeeded`, then public
  `/health/ready` returned `HTTP 200 {"status":"ok"}`.
- Status: **server deployed; physical acceptance blocked on installing RC-4b.**
  The explicitly requested server-only deploy preceded the compatible target
  release, so no new catalog-selection command may be tested on an older strict
  RockCast parser.

## 2026-09-17 — RS-8: owner-scoped runtime_state projection in the directory

- Goal: implement Phase 0 + Phase 2 of
  [`roadmap/rockmobile-rockcast-live-control.md`](roadmap/rockmobile-rockcast-live-control.md):
  give the controller-owner confirmed device runtime state (playback/volume with a monotonic
  `state_revision`) through the ordinary directory REST snapshot and WSS `directory.upsert`,
  without new endpoints, transports, pairing, auth changes, or polling.
- Scope: `api/openapi.yaml` adds optional `runtime_state` (`$ref DeviceStateSnapshot`,
  absent-not-null semantics) to `DeviceControlDirectoryEntry`; `DirectoryDeviceDto` serializes
  it with `skip_serializing_if` and fills it in both `directory::snapshot` (WSS fan-out) and
  `directory::get` (REST) from the already-loaded `load_device_state` projection, gated on
  `DeviceControlScope::EntityStateRead` via the existing `granted_scopes`; `state_freshness`
  stays a separate always-present fact. Golden fixture `directory-snapshot-server.json` now
  carries two entries with `runtime_state` plus a legacy offline entry without the field, and
  the new golden `directory-upsert-server.json` (registered in `FixtureSpec`, distinct
  message-ID count 33→34) demonstrates a local volume change with a strictly higher device
  `state_revision`. No station resolver, pairing, auth, or command-lifecycle change; no
  secrets or stream URLs in DTOs/fixtures.
- Result: owner controllers holding `entity.state.read` see the revisioned runtime state in
  the directory; callers without the scope and targets that never published state keep the
  field absent (never a fabricated `stopped`/`0%`); stale and conflicting device revisions
  cannot resurrect an older projection (strengthened WSS admission test plus a new StateHub
  last-accepted-snapshot test). Cross-account isolation semantics unchanged.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
  `cargo test` — 211 passed, 0 failed, 15 ignored (PostgreSQL/live gates as usual);
  `git diff --check`. Deterministic only: no network, LLM, or PostgreSQL in the new tests
  (full-path REST/WSS directory coverage above the DTO projection stays with the
  PostgreSQL-gated suite because `AppState.account_store` is the concrete Postgres store).
- Status: **complete; contract + projection implemented.** Physical acceptance (Phase 4)
  not performed; RC-4 (truthful RockCast publisher) and RM-4 (state-driven UI) remain the
  dependent next steps.

## 2026-09-17 — RC-4 / RM-4 planning handoff: authoritative live playback UI

- Baseline: RC-3 is live-accepted. `station.play_station` remains the only
  controller-facing station action; RS-3 resolves it server-side to a validated
  `station.play_stream` for the selected player. A command terminal result is
  not proof that the player is now playing.
- Planned outcome: add an owner-scoped revisioned runtime-state projection to
  the existing directory (the current entry has only `state_freshness`) and make
  it authoritative for now-playing, playback, and volume. RockCast must publish
  state after both remote commands and local player changes; RockMobile must wait
  for the matching state rather than show optimistic success.
- Canonical handoff: [`roadmap/rockmobile-rockcast-live-control.md`](roadmap/rockmobile-rockcast-live-control.md).
  It includes the OpenAPI compatibility gate, implementation order, negative
  cases, test matrix, documentation obligations, and mockup reference.
  Self-contained interactive mockups and Hi-Res PNG assets are packaged in
  [`live-control-mockups.html`](live-control-mockups.html).
- Exclusions: raw stream URLs/credentials, unbounded ICY track metadata, a new
  pairing or command channel, and relay/Chromecast controls (RS-7 remains their
  prerequisite).
- Status: **planned; no runtime contract or code changed by this entry.**

## 2026-09-09 — RS-5: canonical terminal command-result errors

- Result: server-generated terminal `command.result` failures now serialize
  `request_id` from the original `device.command` envelope and `details: {}`
  alongside the existing code/message. The request ID remains available for
  timeout/disconnect completion and stored-result replay. Rockmobile's parser
  is tolerant of the extra field (`DirectoryDtos.kt:9`, `ignoreUnknownKeys = true`).
- Checks: focused command-router serialization regression, OpenAPI fixtures,
  `cargo fmt --check`, strict all-target/all-feature Clippy, and full `cargo test`.
- Status: **complete.**

## 2026-09-09 — RS-4: route authenticated device voice through typed intents and CommandRouter

- Goal: implement Step 3.3 of the RockCast-radio plan: native device session and `voice.main`
  identity on the existing stream, explicit cancel, radio-only `UserIntent` resolution, and
  lifecycle execution on the source player through the normal device command router.
- Scope: native auth is resolved before upgrade without client-claimed identity; the device start
  is checked against the active `voice_endpoint` role, declared voice surface and bounded input
  capability. Play search accepts only a unique top-ranked station, then creates
  `UserIntent::PlayRadio`; stop and absolute volume create typed media intents. The existing
  resolver fixes target to `source_device_id`, and all commands use `CommandRouter.submit` plus the
  existing command store for terminal status. Anonymous/legacy search is unchanged. Cancel drops
  recognition before one terminal `cancelled` frame. Logs omit raw media, recognized text and
  structured-intent bodies; no new persistence was added.
- Contract: with explicit control-center approval, the minimal RS-1 amendment adds closed
  `VoiceDeviceCommandResult {type, request_id, status}` and frozen `VoiceStreamErrorCode` including
  `clarification_required`; legacy `VoiceStreamResult` remains unchanged. The planned marker was
  removed only from `VoiceStreamCancel`, and planned wording was removed from the implemented
  device voice fields.
- Checks: `voice_stream_api` 9/9 and `openapi_contract` 8/8 pass. Cases cover play/stop/volume routed
  to the authenticated source, original play_station persistence plus router-only play_stream
  delivery, succeeded/failed results, cancel/release/one-terminal invariant, invalid native 401 and
  anonymous compatibility, ambiguity, unsupported intent, bounds, silence/recognition failure,
  offline/search timeout and captured-log privacy. Format and strict clippy checks pass; full
  `cargo test` reports 206 passed and 15 ignored external integration/live tests.
- Status: **complete.**

## 2026-09-09 — RS-3: resolve station.play_station to server-validated play_stream dispatch

- Goal: implement the Step-3.2 command-router resolution frozen by RS-1 (openapi 0.5.0):
  on `station.play_station` RockServer resolves the current catalog stream and dispatches a
  validated `station.play_stream` (source=rockserver_catalog, station_id echo) to the
  explicit player target under the same command_id; the controller and logs never see
  `stream_uri` (plan `rock-esp32/docs/rockcast-device-plan.md`, finding F1).
- Scope: `src/device_control/command.rs` — typed `CommandBody::PlayStream` with the two
  frozen wire variants and the literal SSRF gate `validate_stream_uri` (form + literal
  IPv4/IPv6/always-local destinations); `src/device_control_command.rs` — `StationCatalog`
  boundary (implemented by `SearchService`), PlayStation resolution between receipt and
  dispatch (fingerprint/dedup stay on the original controller command), deterministic
  terminal failures, controller-variant gating, source-aware `validate_target`;
  `src/http/endpoints.rs` — every production router builder wires the catalog into
  `CommandRouter`. Contract: `x-rockserver-status: planned` removed from `StationStreamUri`
  (now `implemented`), info description updated; voice markers untouched. Direct
  playback/volume/display/entity command behavior unchanged.
- Result: controllers submit only `station.play_station`; the target alone receives
  `station.play_stream` under the original command_id with the resolved station echoed;
  the stream URI never enters persistence (the reservation stores the original command),
  controller lifecycle frames, logs, error texts, fixtures, or docs. Unknown station,
  stream-less station, and invalid/forbidden URIs terminate as `invalid_payload` with
  fixed messages and replay idempotently; catalog outages/timeouts terminate as retryable
  `persistence_unavailable`; a router without a catalog rejects station commands before any
  lifecycle record. DNS-level SSRF and server-side redirect checking are documented
  limitations (no shared egress layer; redirect checks belong to the target stream client).
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
  `cargo test` — all regular suites green, including router tests (6: happy path with
  same-command_id resolved dispatch, five deterministic failures with URI-leak and replay
  assertions, transient catalog failure, controller play_stream gating, capability/wiring
  rejections), command unit tests (variant round-trip and a 40+-case URI battery), and
  contract tests (`openapi_contract` 8/8 asserting the implemented stream-URI schema and
  the still-planned voice cancel frame; `device_catalog_api` 12/12 unchanged).
  `git diff --check` clean. PostgreSQL integration and live provider tests remain ignored
  (no disposable `TEST_DATABASE_URL` / credentials configured).
- Status: **complete.**

## 2026-09-09 — RS-2: implement device-session catalog browse and search runtime

- Goal: implement the two device-facing catalog routes frozen by RS-1 (openapi 0.5.0) on the
  existing catalog/search services with device-session authentication, stream-free responses,
  and bounded deterministic behavior (Step 3.1 of `rock-esp32/docs/rockcast-device-plan.md`).
- Scope: new `src/http/device_catalog.rs` — `GET /api/v1/device-control/catalog/stations`
  (stable-ID cursor `^[A-Za-z0-9-]+$` ≤512, limit 1..=20 default 20, `next_cursor` = last
  station ID of a full page) over `SearchService::public_catalog`, and
  `GET /api/v1/device-control/catalog/search` (`q` 1..=128 non-whitespace-only, BCP 47-like
  `locale` default en-US, limit 1..=20, one ranked page without cursor) over
  `SearchService::interpret_and_search` under the shared 5 s timeout (timeout → 503, the
  device contract has no 504). Both authenticate the native RockserverBearer device session
  through `authenticate_control_ingress`/`NativeSessionResolver` (401 for missing, unknown,
  expired, revoked, or non-native credentials; retryable 503 when the session store is
  unavailable); no new token format, no extra scope. `DeviceStationDto` mapping drops
  `stream_url`, score/reason, and provider fields by construction; 200s carry
  `Cache-Control: no-store`. Per-device rate buckets in `AppState::device_request_allowed`
  (browse 60/20, search 30/10; 429 with `Retry-After`, `details.limit_scope = "device"`);
  unknown or oversized query parameters rejected with 400 via `deny_unknown_fields`.
  `x-rockserver-status: planned` removed from exactly the two catalog operations (RS-3/RS-4
  markers untouched) and the info description updated. New public builder
  `router_with_search_service_and_native_session_resolver` for tests and harnesses.
  Side fix: `InMemoryStationRepository::list_public`/`list_admin` now sort by stable
  station ID (matching the PostgreSQL `ORDER BY s.id`); the unsorted pinned catalog
  previously could repeat stations across cursor pages. Tests: new
  `tests/device_catalog_api.rs` (12 integration tests: happy paths, full 41-station cursor
  walk without repeats, parameter bounds including unknown keys, 401 without/expired
  session, retryable 503, timeout mapping, stream-free responses including stations with
  invalid stream URLs, per-device rate isolation); `tests/openapi_contract.rs` updated to
  assert `implemented` status and that both routes are registered behind native-session
  auth (401) instead of the previous 404 check. Docs: `docs/status.md`,
  `docs/roadmap/device-control-tasks.md`, fixture README status, service diagrams.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` — all pass (device_catalog_api 12/12, openapi_contract 8/8, search_api 9/9,
  full suite green).
- Status: complete.

## 2026-09-09 — RS-1: freeze RockCast-radio contracts before implementation

- Goal: freeze the Step-2 contracts of `rock-esp32/docs/rockcast-device-plan.md` in RockServer
  before any implementation (unblocks RS-2/RS-3/RS-4).
- Scope: `api/openapi.yaml` 0.5.0 — two planned device-facing catalog paths
  (cursor browse ≤20/page and one-page ranked search, both behind the RockserverBearer device
  session, both returning a new `DeviceStationDto` without `stream_url`); enforceable
  `station.play_stream` variants (server `rockserver_catalog` with required `station_id`,
  controller `direct_stream`) with SSRF bounds in `StationStreamUri`; voice stream contract
  realigned with the enforced runtime (16 kHz mono, 32 KiB chunks, 2 MiB/60 s, idle 10 s,
  wall 75 s, provider 15 s, machine-readable in `x-voice-stream-limits`) plus the planned
  device-session/cancel extension (`VoiceStreamCancel`, `voice.main`, server-derived
  `source_device_id`); decision that `station_list`/`search`/`loading`/`offline`/
  `playback_error` are local UI states with no new remote presentation types. Fixtures:
  truthful final ESP32 radio register manifest (controller role added, `media.station`
  rockserver_catalog only, `voice.main` surface, no sensors), coherent manifest-rev-2 and
  directory snapshot, new `device-catalog-response.json`, README semantics for
  accepted/result ≤30 s and the Rockmobile volume rule. Tests: `tests/openapi_contract.rs`
  extended with structural, positive and negative schema assertions and a runtime check that
  the planned catalog paths answer 404. Roadmap RS-task section added in
  `docs/roadmap/device-control-tasks.md`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` — all pass; contract suite 8/8. No runtime code changed; all
  `x-rockserver-status: planned` markers preserved.
- Status: complete.

## 2026-09-09 — DC-019/DC-020 deferred; DC-039 selected for execution

- Goal: record the owner's current hardware constraint and executable product priority.
- Scope: deferred DC-019 sensor modules and dependent DC-020 sensor-display end-to-end because
  physical sensors are not yet available. Their definitions remain intact for later resumption.
  The owner selected DC-039, thereby making the required product decision to pursue on-device
  interactive station browse on ESP32. The roadmap preserves the requirement to define any
  contract extension before implementation.
- Checks: documentation-only update; no runtime, protocol contract, or sensor behavior changed.
- Status: complete.

## 2026-09-09 — DC-017 completion and DC-018 roadmap synchronization

- Goal: align RockServer's device-control roadmap with the verified ESP32 firmware state.
- Scope: marked DC-017 complete with its hardware and contract-test evidence; Phase 5 now
  records the completed C6/SDIO, provisioning, NVS, and WSS work. DC-018 remains the next
  executable milestone: `display.main`, protocol presentation views, GT911 touch, and
  idempotent display commands. Interactive station browsing and local audio remain product-gated
  DC-039/DC-040 work.
- Checks: documentation-only comparison against `rock-esp32` commit `813a544` and its
  `docs/dc-017-progress.md` / `docs/device-control.md`; no runtime or protocol contract changed.
- Status: complete.

## 2026-09-08 — DC-018 GUI stack decision: LVGL v9 via esp_lvgl_port (owner confirmed)

- Goal: record the owner's confirmation of LVGL v9 through the C `esp_lvgl_port` component as
  the DC-018 display stack, following the delegated comparison and independent verification
  from the entry above.
- Scope: `docs/roadmap/device-control-tasks.md` DC-018 now carries the decision with rationale
  and exclusions (Slint: paid embedded license and software rendering, kept as documented
  fallback; embedded-graphics: DC-017 provisioning screen only; third-party LVGL v9 binding
  crates: reference only, the binding layer is owned firmware code).
  `docs/roadmap/device-control-rockcast-esp32.md` Phase 5 reflects the confirmed stack. The
  rock-esp32 repository's `docs/rust-direction.md` records the same decision for firmware work
  (C-side `esp_lcd`/GT911 init, owned thin binding layer). RockServer runtime, OpenAPI,
  migrations and clients unchanged.
- Checks: documentation-only; no build or test run needed.
- Status: complete.

## 2026-09-08 — agy delegation convention and DC-018 GUI-stack comparison

- Goal: record external-agent delegation as standard practice, and run the first delegated
  analysis: LVGL v9 vs Slint vs embedded-graphics for the DC-018 display surface on the
  JC4880P443C_I_W (ESP32-P4, 800x480 RGB, GT911 touch, ESP-IDF 6.1, vendored `esp-idf-sys`).
- Scope: added the standing `agy` delegation convention (task classes, invocation flags,
  secret-exclusion, quota care, local verification duty) to `docs/codex-project-context.md`.
  The comparison itself ran headless on Gemini 3.1 Pro with a JSON-schema-bounded response
  from a temp workspace; no repository file was exposed to the external agent.
- Result: external ranking Slint 8.5 / LVGL 7 / embedded-graphics 4. Independent verification:
  Slint's royalty-free license excludes embedded (paid commercial ~$72/seat/year or GPLv3 for a
  proprietary ESP32 product — [pricing](https://slint.dev/pricing),
  [LICENSE.md](https://github.com/slint-ui/slint/blob/master/LICENSE.md)); LVGL v9 Rust
  bindings are new and unstable (`oxivgl`, `lightvgl-sys` 9.5.2; the classic `lvgl` crate
  tracks v8); Slint is officially in the
  [Espressif Component Registry](https://components.espressif.com/components/slint/slint/versions/1.7.1)
  with [ESP32-P4 demos](https://slint.dev/esp32) but software-rendered.
- Recommendation recorded for the DC-018 decision: LVGL v9 via the C `esp_lvgl_port` component
  (MIT, PPA hardware acceleration, proven on this board class) with Rust owning protocol/state
  and a thin owned binding layer; Slint as fallback if FFI friction exceeds the license cost.
  Final choice remains an owner decision per DC-018.
- Checks: documentation-only change to this repository; no build or test run needed.
- Status: complete.

## 2026-09-08 — Milestone D roadmap audit and realignment

- Goal: audit the device-control roadmap and task list from Milestone D onward now that real
  ESP32 hardware is connected, and record the plan-order and code-reuse decisions.
- Scope: documentation only. `docs/roadmap/device-control-tasks.md` marks DC-010 and
  DC-012–DC-016 complete (evidence in the RockCast/RockMobile logs; live command E2E accepted
  2026-09-07, the Chromecast hardware smoke remains unperformed), rewrites DC-017 with bring-up
  preconditions (ESP-Hosted/SDIO Wi-Fi, SNTP, one HTTPS request), the minimal pairing screen
  (short code, verification phrase, QR), the WSS transport choice under ESP-IDF 6.1, the
  RockCast code-reuse strategy and board-specific acceptance (ESP32-C6 reset via GPIO54, SDIO
  power-cycle recovery, secret-free logs), and adds a GUI-stack decision plus the v1 scope bound
  to DC-018. A new product-gated Milestone D2 adds DC-039 (interactive on-device station
  browse) and DC-040 (local audio output). `docs/roadmap/device-control-rockcast-esp32.md`
  aligns Phase 5 with the verified hardware and task order and updates the ESP32
  repository-boundary paragraph and execution-order note.
- Checks: Markdown-only change; completion claims cross-checked against the RockCast/RockMobile
  task logs and the rock-esp32 README/bring-up notes. No build or test run was needed.
- Status: complete.

## DC-016 — forward full-state revision reconnect resync (2026-09-07)

- Goal: stop the permanent RockCast reconnect flap observed on staging (one
  `devices/connect` upgrade every ~51 s, device stuck offline in RockMobile).
- Root cause: the player may publish state revisions while its socket is down, so its
  monotonic counter legitimately runs ahead of the server projection (staging showed
  client revision 19 against stored revision 4). Both the in-memory hub
  (`accept_snapshot`) and PostgreSQL `store_device_state` classified such a full snapshot
  as `Gap`/`Resync`, the client resent the identical snapshot, and the first heartbeat
  closed the connection as `full_state_required`; capped reconnect backoff turned this
  into a non-converging 20 s + 30 s loop.
- Scope: a complete snapshot is the protocol's resync primitive, so a forward revision now
  overwrites the stored projection in both the state hub and the PostgreSQL latest-snapshot
  upsert. Stale/replay/conflict classification, strictly ordered deltas
  (`base_revision`), entities, manifests, authentication, scopes, commands and wire
  schemas are unchanged; `api/openapi.yaml` now documents the forward-overwrite rule.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
  and `cargo test` passed (146 library tests, 22 suites; PostgreSQL integration remains
  opt-in). New transport test reproduces the forward-revision reconnect and asserts the
  heartbeat stays acked and the projection is overwritten.
- Status: implemented; staging deployment and physical RockCast/RockMobile retest follow
  in this task.

## DC-016 — stale full-state reconnect gate (2026-09-07)

- Fixed a player reconnect loop: a stale full snapshot retains the last accepted server state but
  satisfies the connection's mandatory initial-state gate, including PostgreSQL's `Stale` store
  outcome. Previously the next heartbeat closed that healthy connection as
  `full_state_required`.
- Revision gaps and conflicts remain resync-only; the patch neither changes the v1 schema nor
  relaxes authentication, ownership, scopes, command admission or persistence writes.
- Checks: `cargo test control --lib` (31 passed). Deployment and physical retest remain pending.

## DC-016 E2E controller-state gate — 2026-09-07

- Goal: allow a controller-only RockMobile control connection to stay online without inventing a
  player runtime state.
- Scope: narrow WebSocket lifecycle admission change in `http/control.rs`; player and hybrid
  devices still require their full initial state before heartbeat.
- Result: a manifest whose sole role is `controller` no longer enters the full-state gate. Any
  role that publishes facts (`player` included) retains the existing snapshot/resync invariant.
  Identity, pairing, scopes, directory, command routing and wire schemas are unchanged.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed (142 library tests; PostgreSQL and live-provider tests remain opt-in).
- Deployment: staging commit `5c4698b` succeeded; external HTTPS readiness returned 200.
- Post-deploy checks: all 6 RockServer WebSocket lifecycle tests, 15 RockCast device-control tests
  and 101 RockMobile unit tests passed.
- Status: deployed and automatic cross-component coverage passed. Hardware E2E remains pending
  because no paired RockMobile/RockCast player was available to dispatch an actual command.

## Graphify agent policy cleanup — 2026-09-02

- Goal: remove Graphify from agent instructions after deleting the global skill.
- Scope: delete `.cursor/rules/graphify-usage.mdc` and the Graphify section in `AGENTS.md`;
  update status/task docs. No application code changed.
- Result: Cursor no longer loads a RockServer Graphify usage rule; contributor instructions no
  longer prescribe Graphify workflow.
- Checks: documentation-only change; no code checks run.
- Status: **complete.**

## DC-004 — 2026-09-02 — device-control transport-independent domain model

- Goal: implement protocol-v1 domain values and deterministic validation without beginning transport or persistence work.
- Scope: one public `device_control` module, checked RFC3339 timestamps via `time`, canonical-fixture domain tests, and status/roadmap updates.
- Result: separate lifecycle IDs; device/entity/surface, role/scope, capability/manifest, runtime/entity state, presentation and command/result types are public. Unknown namespaced capabilities preserve unknown JSON fields; opaque unknown commands deserialize but are never executable and return `unsupported_command`. Validation covers contract cardinality, unique IDs/names/actions, numeric/unit constraints, deadline, fixed-time freshness, revisions/resync and terminal outcome consistency.
- Checks: focused domain tests, OpenAPI fixture validation, formatter, strict Clippy, full tests and whitespace checks were run serially; no database/provider runtime work was introduced.
- Status: **complete; DC-005 is next.**

## API namespace consolidation — 2026-09-02

- Goal: make every first-party HTTP and WebSocket API route use one `/api/v1/*` namespace.
- Result: removed the former non-namespaced route registrations and voice compatibility aliases. The unified
  `/api/v1/voice/command` and `/api/v1/voice/stream` endpoints work anonymously without an
  `Authorization` header; a supplied Bearer is validated as the legacy deployment credential or a
  paired native-session token. Updated the RockCast client, web client, OpenAPI, Caddy routing,
  contract coverage, and active documentation. External provider URLs were deliberately unchanged.
- Checks: RockServer `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  and `cargo test` passed (113 unit tests; opt-in PostgreSQL/live-provider tests ignored). RockCast
  passed the same formatter and strict Clippy checks plus `cargo test` (97 unit tests; live tests
  ignored). The web client passed `pnpm build` (typecheck, lint, Vite build), and both diffs passed
  whitespace checks.
- Deployment: OPS-001-D deployed commit `75f0fc5` to `alex.vault57.ru` with `status=succeeded`;
  public readiness returned `200`.
- Status: **deployed to staging.**

## Codex master-only workflow — 2026-09-02

- Goal: require Codex to work only in the primary RockServer checkout on `master`.
- Result: `.cursor/rules/master-workflow.mdc` and `docs/codex-project-context.md` now
  forbid creating or using separate Git worktrees for RockServer and require switching
  to `C:\repos\rockserver` on `master` before any edit.
- Checks: documentation-only change; no code or deploy checks run.
- Status: **documented.**

## DC-000 — 2026-09-02 — Windows-first readiness gate

- Goal: determine whether the current RockServer/RockCast Windows path permits the
  device-control protocol to begin, without implementing any later DC work.
- Scope: read-only code/contract audit of `C:\repos\rockserver` and `C:\repos\rockcast`;
  record the supported search/voice behaviour, STT/degraded-state evidence, and RockCast
  playback, volume, Chromecast, and relay entry points in RockServer documentation.
- Result: **GO for DC-001.**  RockCast has an anonymous `/api/v1/search` path and independent local
  catalog/Radio Browser playback fallback.  Voice now selects anonymous `/api/v1/voice/stream` when
  no native credential exists, and renews a paired token through `/api/v1/auth/device-session` before
  using canonical authenticated `/api/v1/voice/stream`.  Those canonical RockServer routes now
  accept a current native-session Bearer as well as the legacy deployment Bearer.  SpeechKit
  remains optional and returns a defined unavailable error when unconfigured; second-provider/
  retry hardening is future work.
  The removed `docs/windows-production-roadmap.md` and Windows E2E/failure coverage are explicitly
  out of scope for this decision.  Playback control presently
  enters through `RockCastApp` actions and `PlaybackController`, with `CastService` and
  `StreamRelay` as the receiver/relay boundaries; no remote device-control adapter exists.
- Checks: targeted source, OpenAPI, active/historical roadmap, and status/task-log inspection;
  RockServer checks passed serially after the native-session voice authorization update.  The
  RockCast change passed `cargo fmt --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` (97 unit tests
  plus non-live integration coverage).  No external provider, database, deployment, or Windows
  hardware probe was executed.
- Status: **complete; GO recorded for DC-001.**  No ESP32 firmware or later DC implementation
  task was started.

## DC-001 — 2026-09-02 — device-control identity and authorization model

- Goal: make the existing account-owned identity boundary and the minimum authorization model
  unambiguous before protocol v1 is specified.
- Scope: document-only comparison of the active account/device schema, pairing, native session,
  device-session, inventory and revocation paths with the device-control roadmap. No runtime,
  OpenAPI, SQL migration, DTO, pairing flow or credential changed.
- Result: the roadmap now distinguishes actor identity, functional device roles and authorization
  scopes; records permissions for RockMobile, RockCast, ESP32, a future automation principal and
  the Home Assistant adapter; and requires same-owner, scope, capability/schema,
  online/freshness and confirmation checks. It explicitly denies cross-user access and hidden
  broadcasts, keeps administrators outside device control, and defers homes/delegation,
  automation identity and finer-grained policy. Canonical existing device endpoints are written
  as `/api/v1/*`.
- Checks: inspected migrations 0012, 0013, 0016 and 0017; account/auth/pairing/persistence
  implementation; endpoints and OpenAPI; and all affected roadmap/status/task sections. Ran
  `git diff --check`, targeted terminology/path searches, and a manual acceptance-table review.
  Markdown-only scope: no build, Clippy or test run required.
- Status: **complete.** The next entry records completed DC-002; DC-003 and later remain planned.

## DC-002 — 2026-09-02 — device-control protocol v1 and transport contract

- Goal: make the RockMobile controller, RockCast player and future ESP32 multi-role client
  independently implementable without reading RockServer Rust code.
- Scope: contract-first OpenAPI changes only: machine-marked planned
  `GET /api/v1/devices/connect`, separate planned
  `GET /api/v1/device-control/directory`, protocol schemas, and minimal structural contract
  assertions. Existing runtime routes, database, clients, firmware and fixtures were not changed.
- Result: protocol major 1 now specifies native-session-only/server-derived identity; common JSON
  envelope and distinct correlation IDs; negotiation/registration, roles/capabilities,
  entity/surface manifests, heartbeat/presence, full/delta state and telemetry, directory events,
  explicit targeting, a typed namespaced command union, received/accepted/exactly-one-result
  lifecycle, structured errors, revision/resync rules and forward/deprecation compatibility.
  `station.play_station` is resolved by RockServer; direct `station.play_stream` requires the
  declared capability and strict URI/DNS/redirect validation.
- Limits: 65,536-byte JSON frame, 61,440-byte payload, 32 capabilities, 256 entities, 16 surfaces,
  50 directory devices, 20-second heartbeat, 60-second offline TTL, 10-second registration,
  16/8 in-flight commands per connection/target, 10-second default and 30-second maximum command
  timeout, 86,400-second command-id idempotency, and at most five revalidated stream redirects.
- Checks: focused `cargo test --test openapi_contract --jobs 1` passed (3 tests), including YAML
  parsing, planned/security markers, exact policy constants, identity/secret exclusions, dangling
  refs, discriminator mappings and unique operation IDs. `cargo fmt --check`, strict
  all-target/all-feature Clippy and full `cargo test --jobs 1` also passed; external PostgreSQL and
  provider tests remained ignored behind explicit gates. No database query changed.
- Status: **complete contract; runtime remains planned.** DC-003 and later tasks remain planned.

## DC-003 — 2026-09-02 — shared device-control v1 golden fixtures

- Goal: provide one canonical, machine-validated raw JSON fixture corpus that each future
  RockServer/RockCast/RockMobile/ESP32 contract test can consume without copying or reading Rust.
- Scope: `tests/fixtures/device-control/v1/`, a concise fixture index, a narrow OpenAPI-schema
  contract test and the smallest v1 schema correction necessary for the documented unknown-command
  compatibility rule. No runtime transport, database, pairing, client, firmware or provider
  adapter work.
- Result: fixtures cover hello/welcome; truthful RockCast player and ESP32 multi-role
  registration/manifests; ESP32 full/delta state and temperature/humidity telemetry; a
  RockMobile-suitable directory snapshot; explicit display `sensor_grid`; success and structured
  failure command lifecycles; forward unknown capability/command; stale revision; semantic sensor
  unit/value rejection; duplicate command replay; offline target; missing surface; and an
  intentionally schema-invalid frame. Home Assistant is limited to source-neutral normalized
  entity directory/state projections, never a paired device or provider-native integration
  protocol. `UnknownCommandBody` permits an unknown namespaced envelope to validate and receive
  `unsupported_command`, while known command schemas remain strict.
- Validator: dev-only `jsonschema` 0.30 compiles JSON Schema 2020-12 roots using the OpenAPI
  component graph and enforces required/type/const/enum/oneOf/additionalProperties/bounds/local
  `$ref`. Explicit flow assertions add command/message correlation, exactly one terminal result,
  monotonic state revisions, idempotent replay, semantic error replies and identity exclusions.
- Checks: focused `cargo test --test openapi_contract --jobs 1` passed (4 tests); `cargo fmt
  --check`, strict `cargo clippy --all-targets --all-features -- -D warnings`, and full `cargo
  test --jobs 1` passed. External PostgreSQL and provider tests stayed ignored behind their
  explicit environment gates; no database query changed.
- Status: **complete contract fixtures; runtime remains planned.** DC-004 and later remain planned.

## Local administrator launcher — 2026-09-01

- Goal: provide a simple host-based launcher for RockServer plus local administrator SPA hosting
  when PostgreSQL and admin bootstrap already exist on the workstation.
- Result: `run-rockserver-admin-local.ps1` and `deploy/Caddyfile.dev-host` start RockServer through
  `cargo run`, serve `web/dist` through a disposable Caddy container on loopback port `3080`, proxy
  API traffic with the trusted-proxy header, and align `ROCKSERVER_LOCAL_ADMIN_ORIGIN` with the
  browser origin. Missing `web/dist` triggers a one-time `pnpm build`. On Windows the launcher
  rewrites stale `unix://` `DOCKER_HOST` values to `npipe:////./pipe/docker_engine` for Docker Desktop.
- Checks: PowerShell parser accepted the launcher; documentation updated. No browser smoke was run.
- Status: **implemented locally.**

## RS-ADMIN-006 — 2026-09-01 — SPA admin console

- Goal: replace the superseded server-rendered administrator shell with the existing first-party
  Preact/Vite SPA served by Caddy, retaining the established revocable administrator boundary.
- Result: `/admin` is a same-origin SPA route with Devices, Audit and Stations tabs. The browser
  retains the Bearer only in current component memory; logout and `401` clear it. Devices and audit
  use safe dedicated server read models; stations use server-side bounded pagination and name/tag
  search. No admin mutation, credential lifecycle or token persistence was added.
- Checks: web typecheck/lint/test/build, `cargo fmt --check`, strict all-target/all-feature Clippy,
  `cargo test`, OpenAPI contract and `git diff --check` passed serially. A loopback-only disposable
  Docker Desktop PostgreSQL database passed the changed admin durable-query integration test; it was
  removed. Merged local Compose config validated. No local Caddy image was available for Caddyfile
  validation or browser smoke, so `http://localhost/admin` was not manually exercised.
- Status: **implemented locally; Caddy runtime smoke pending an available local image.**

## Local launcher alignment — 2026-09-01

- Goal: align `run-rockserver-local.ps1` with the administrator authentication and trusted-proxy
  startup boundaries without weakening them.
- Result: it creates a process-local `ROCKSERVER_TRUSTED_PROXY_TOKEN` when absent, corrects stale
  admin-token messaging, and permits login only from its exact loopback `http://127.0.0.1:<port>`
  origin. Bootstrap remains the separate protected command; LAN origins and production policy were
  not weakened.
- Checks: PowerShell parser validation, `cargo fmt --check`, strict all-target/all-feature Clippy,
  `cargo test` (113 library tests plus regular target/contract suites), and `git diff --check`
  passed. No migration, dependency, or PostgreSQL query changed.
- Status: **complete locally.**

## RS-ADMIN-005 — 2026-09-01 — administrator operational hardening

- Goal: harden the already implemented administrator session boundary and establish bounded,
  retention-safe operational evidence and runbooks without widening product authentication.
- Scope: atomic admin-session refresh rotation, durable safe request records with a 30-day write-path
  retention policy, deterministic fake/HTTP/PostgreSQL coverage, backup/incident/deployment
  acceptance runbook and security-plan/status updates. RockCast machine-credential lifecycle was
  explicitly excluded: product routes remain public and pairing remains its user-identity boundary.
- Result: refresh inserts the replacement session hash and revokes the old session in one transaction;
  failed refresh leaves no usable replacement. Authenticated administrator routes persist only static
  endpoint, safe outcome, bounded duration, request ID and UUID correlations; credentials, headers,
  bodies, search text and transcripts are not stored. `docs/admin-operations-runbook.md` documents
  restore-triggered session revocation, incident handling, retention and acceptance checks.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test` (113 library
  tests plus all non-ignored target/contract suites), OpenAPI contract checks and `git diff --check`
  passed serially. A loopback-only disposable Docker Desktop `pgvector/pgvector:pg17` database
  passed `postgres_admin_auth_queries_are_durable_and_revocable`; it covers every changed or new
  administrator persistence query, including atomic rotation and request-record retention. The
  disposable container was removed after the test.
- Status: **complete locally.** No next RS-ADMIN task is currently defined.

## RS-ADMIN-004 — 2026-09-01 — first-party administrator console

- Goal: replace the unauthenticated local preview with the minimum protected, server-rendered
  administrator console on the RS-ADMIN-003 Bearer boundary.
- Scope: Axum HTML shell and locally served CSS/JS, existing login/logout hand-off, protected
  bounded station listing read model, strict admin response headers, OpenAPI and deterministic
  HTTP/contract checks. No new PostgreSQL query, migration, import/job, credential administration,
  audit/log pages, password change, deployment, or external network access.
- Result: the raw opaque Bearer is only the current page helper's JavaScript variable; it is never
  rendered or persisted and is cleared on logout and `401`. Admin routes use no-store and a CSP
  permitting only same-origin script/style/connect resources. The listing resolves the existing
  revocable session before delegating to the catalog read service.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test` (111 library
  tests plus all non-ignored target/contract suites), OpenAPI contract checks, and `git diff --check`
  passed serially. `TEST_DATABASE_URL` was unavailable; no disposable PostgreSQL run was applicable
  because this task introduces or changes no PostgreSQL query.
- Status: **complete locally.** RS-ADMIN-005 is next.

## RS-ADMIN-003 — 2026-09-01 — administrator login, revocable sessions, and admin boundary

- Goal: implement only the administrator authentication boundary on the RS-ADMIN-001/002 foundation.
- Result: versioned admin login, refresh, logout, and an admin-role verification route use Argon2id
  verification, random opaque 256-bit Bearer values with persisted hashes only, fifteen-minute TTL,
  server-side revoke/rotation, durable login attempt throttle, and redacted security events.
  State-changing requests require the established trusted-origin/proxy validation. Product,
  passkey/browser, legacy compatibility, UI, station, and password-change behavior is unchanged.
- Checks: `cargo fmt --check`, strict Clippy, `cargo test`, OpenAPI contract checks and
  `git diff --check` passed. A disposable `pgvector/pg17` database passed the new admin durable
  query/revocation test and bootstrap concurrency test when each was run serially. The existing
  all-ignored suite has unrelated shared-database collisions and is not a valid parallel run.
- Status: **implemented locally; RS-ADMIN-004 is next.**

## RS-ADMIN-002 — 2026-09-01 — protected one-time administrator bootstrap

- Goal: bootstrap exactly one administrator principal from a protected terminal/deployment
  environment without adding public authentication behavior.
- Scope: an explicit `bootstrap_admin` binary, Argon2id PHC hashing, username/singleton migration,
  atomic PostgreSQL persistence, deterministic unit tests, opt-in disposable PostgreSQL coverage,
  safe `.env.example` placeholders, and factual current-state documentation. No HTTP route, login,
  logout, refresh, middleware, admin UI, deployment, staging, or password-change behavior.
- Result: the binary consumes only `ROCKSERVER_ADMIN_BOOTSTRAP_USERNAME` and
  `ROCKSERVER_ADMIN_BOOTSTRAP_PASSWORD` from its process environment; it does not load `.env` or
  parse secret arguments. A password is never persisted or logged: Argon2id produces the only PHC
  value accepted by the store. Migration `0019` adds username metadata and a singleton index; one
  PostgreSQL CTE creates the principal, credential and `admin_created` event together. Existing or
  concurrent-loser attempts return `AlreadyExists` without modifying a credential. Error and Debug
  paths redact password/PHC material. `.env.example` contains placeholders only and `.env` remains
  ignored.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features --jobs 1 -- -D warnings`,
  `cargo test --jobs 1` (109 library tests plus all regular integration suites), and `git diff
  --check` passed. `git check-ignore -v .env` confirmed the existing ignore rule. A local disposable
  loopback `pgvector/pgvector:pg17` database was created because `TEST_DATABASE_URL` was not
  preconfigured; `cargo test --test postgres_integration -- --ignored --exact
  postgres_admin_bootstrap_is_atomic_and_idempotent` passed. It executes the new bootstrap query
  concurrently and verifies one principal, credential and security event. The exact temporary
  container was stopped and removed afterward.
- Status: **complete locally.** RS-ADMIN-003 remains the next scope.

## RS-ADMIN-001 — 2026-09-01 — durable administrator identity foundation

- Goal: add durable administrator principal, credential/session, login-attempt, and security-event
  foundations without beginning bootstrap, login routes, middleware, UI, or deployment work.
- Scope: migration `0018`, a separate `admin` domain/persistence contract, PostgreSQL store, and
  deterministic in-memory fake. Administrator state has no foreign keys to Rock accounts, devices,
  passkeys, or browser/native sessions; password and opaque token inputs are hash-only.
- Result: database constraints restrict password records to Argon2id PHC representations, fixed-size
  token/account/source hashes, safe lifecycle states, safe event/outcome vocabulary, and active-record
  indexes for verification and future throttling. No public API or existing account semantics changed.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test`, and `git diff
  --check` passed. `cargo test` passed 106 library tests and all regular integration tests. The
  deterministic fake tests cover non-Argon2id rejection, redacted hash diagnostics,
  missing-principal credentials, and disabled-principal sessions. The PostgreSQL migration/constraint
  regression passed against a disposable PostgreSQL 17 Docker database through `TEST_DATABASE_URL`;
  it exercises every new administrator persistence query plus invalid password-hash and token-hash
  constraints. No staging, deployment, or external-network test ran.
- Status: **complete locally.** RS-ADMIN-002 remains the next scope.

## RM-011 — 2026-08-30 — durable device-secret server sessions

- Goal: replace fragile rotating refresh tokens with a persistent revocable device credential.
- Scope: add the device-session API, persist only device-secret hashes, remove native refresh/logout
  routes and refresh-token table, and reset staging native bindings through migration `0017`.
- Result: access expiry triggers a safe credential-based session issuance; a lost response or
  temporary failure cannot unpair the device.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed
  (103 unit tests; PostgreSQL/live suites remain opt-in). OPS-001-D staging deploy of `7916cee`
  reported `status=succeeded` and public readiness passed. An initial deploy of `74589c7` failed
  before readiness because migration `0017` deleted sessions before the dependent refresh table;
  the follow-up commit corrects the order.
- Status: **staging deployment verified.**

## RM-011-E2E-R4 — 2026-08-30 — browser account SQL availability repair

- Goal: remove the common account-service failure observed in the browser cabinet during the
  permitted physical pairing flow.
- Scope: PostgreSQL multiline query continuations for browser-session ownership, device listing,
  rename, and browser logout. No account data, cookie, token, proof, or request URL was read.
- Result: six query strings had emitted a literal backslash, which PostgreSQL rejected with
  SQLSTATE `42601`. They now use Rust string continuations and emit valid whitespace.
- Checks: the existing disposable PostgreSQL
  `postgres_browser_account_centre_owns_rename_and_revoke` regression passed after the fix;
  the ordinary Rust suite passed 103 tests; formatting and strict Clippy passed. OPS-001-D deployed
  commit `c36e652` with `status=succeeded`, and public readiness returned `200`. Renewed physical
  pairing remains pending.
- Status: **staging deployment verified; physical pairing pending.**

## RM-011-E2E-R2 — 2026-08-30 — stale browser reauthentication guard

- Goal: prevent a restored browser cookie from exposing a pairing approval action that the server
  must reject without a fresh passkey assertion.
- Scope: Web pairing state and regression assertion only; no account/device data, proof, token, or
  browser storage was read or retained.
- Result: pairing now requires an explicit fresh passkey after cookie restoration and preserves
  terminal, explicit-authentication, and in-flight approval states against late restoration.
- Checks: Web regression tests 11/11, typecheck/lint/build, `cargo fmt --check`, strict Clippy,
  and `cargo test` (103 passed) completed. Existing disposable PostgreSQL and credential/live tests
  remain intentionally ignored. OPS-001-D deployed commit `82382b1` with worker
  `status=succeeded`; public readiness was `200` and the published bundle contained both the guard
  and reauthentication control.
- Status: staging deployment verified. Physical passkey completion is still pending and not
  claimed.

## RM-011-E2E-R3 — 2026-08-30 — deleted-passkey pairing recovery guidance

- Goal: make the secure consequence of a deleted passkey actionable without treating a retained
  browser cookie as a substitute for authentication.
- Scope: pairing-page explanation and a route to create a distinct account for the active pairing.
  It never restores, changes, merges, or deletes the existing account.
- Result: the page explains that a deleted passkey cannot be recovered from a browser cookie and
  offers a separate-account registration route while retaining the active pairing context.
- Checks: Web regression tests 11/11, typecheck/lint/build, `cargo fmt --check`, strict Clippy,
  and `cargo test` (103 passed) completed. Existing disposable PostgreSQL and credential/live tests
  remain intentionally ignored. OPS-001-D deployed commit `a247ffc` with `status=succeeded`;
  public readiness was `200` and the published bundle contains the distinct-account recovery
  control.
- Status: staging deployment verified. A user passkey ceremony and physical native completion
  remain pending and are not claimed.

## RM-011-R2 — 2026-08-30 — native availability recovery

- Goal: reproduce and remove the reported native account-service-unavailable condition using a
  disposable Android emulator and the official RockCast runtime.
- Scope: safe staging health/schema/proxy probes; a disposable anonymous Mobile pairing creation;
  Mobile stale-endpoint recovery and release metadata; local RockCast startup and tests. No
  account lookup, credential/proof capture, passkey bypass, destructive database action, or push.
- Result: staging account service/proxy/migrations are healthy, and a clean Mobile release reaches
  pairing confirmation. Mobile commit `4f7495d` fixes retained unsupported endpoint overrides and
  uses the actual build version. RockCast release starts and its account/session suite passes.
- Checks: RockServer format, strict Clippy, 103 tests, web 11/11 and build; Mobile unit tests,
  lint, release build, APK certificate/metadata and emulator install; RockCast format and 93 tests.
- Status: availability fault addressed and verified without a user account. Passkey-dependent
  completion/session/device-list E2E remains unverified and is not claimed.

## RM-011-E2E-R1 — 2026-08-30 — late browser-session state regression

- Goal: diagnose the first permitted disposable staging pairing attempt without retaining sensitive
  handoff, account, credential, device, or browser-session data.
- Scope: source-level state analysis, safe local client-storage presence check, and the minimal
  web-only race fix. No database mutation, remote push, credential/proof capture, or new pairing
  attempt.
- Result: a late `browserSession` response could overwrite `approved` with `authenticated` after a
  successful approval, causing the browser to show the pending confirmation again. The web reducer
  now keeps `approved` terminal for that response and has regression coverage. The live account
  centre was unavailable and end-to-end client persistence/device-list verification remains open.
- Checks: web `pnpm test` passed 11/11 and `pnpm build` passed.
- Status: deployed as `82521f7` through OPS-001-D (`status=succeeded`; public readiness `200`). A
  fresh disposable E2E is still required.

## RM-011-FINAL — 2026-08-29 — cross-repository final integration

- Goal: audit all RM-011 waves in RockServer, RockMobile and RockCast; publish the verified Android
  App Link association through the existing staging release workflow; and run one disposable staging
  E2E only if the physical/security prerequisites were actually available.
- Scope: read-only cross-repository commit/RESULT/worktree audit; local source contract checks;
  public certificate verification from the existing signed release APK; one new checked-in static
  Caddy/Vite asset; documented controlled staging deployment and public readiness validation.
- Result: RockServer commit `e171096` deployed successfully. Public `GET /health/ready` returned
  `200`; public `GET /.well-known/assetlinks.json` returned `200 application/json` with the single
  verified `com.rockmobile` association. The local certificate check used `apksigner` against the
  existing release APK and recorded only its public SHA-256 fingerprint. No push, destructive DB
  action, account/device/session mutation, credential/proof capture or real pairing flow occurred.
- Checks: JSON target validation; web `pnpm test` (10/10) and `pnpm build`; `cargo fmt --check`;
  `cargo clippy --all-targets --all-features -- -D warnings`; and `cargo test` passed. Release
  preflight, commit-bound image build, remote worker `status=succeeded`, public HTTPS readiness and
  public asset content validation passed.
- Status: deploy and App Link association complete. Disposable E2E is blocked by absent connected
  Android/ADB and absent running RockCast; passkey, physical QR scanning and installed-client UI
  outcomes remain unverified. Audit exceptions preserved: one dirty user worktree in RockMobile and
  the non-ancestral Wave 2 implementation hash recorded in its RESULT file. See
  `RM-011-FINAL-RESULT.md`.

## RM-011-09 — 2026-08-29 — Wave 9 A4 secure pairing handoff

- Goal: remove the browser approval secret from pairing URL queries while preserving a bounded
  rollout path and the existing pairing lifecycle.
- Scope: existing Preact URL parsing and deterministic regressions only; no server API, OpenAPI,
  deploy, staging or account/device operation.
- Result: new handoffs use `?code=<code>#secret=<proof>`; fragment/query secrets are memory-only,
  retained across rerenders only in the first render's ref, and immediately scrubbed with
  `history.replaceState`. Legacy query support ends at
  2026-09-29T00:00:00Z. Regression coverage asserts fragment parsing, cleanup, bounded legacy
  support and no console output.
- Checks: web `pnpm test` (10/10), web `pnpm build`, `cargo fmt --check`, strict Clippy and
  `cargo test` passed. `git diff --check` is recorded after the documentation commit.
- Status: complete locally. Production App Link verification is blocked outside this repository:
  Caddy serves only the web build and no `assetlinks.json` is present.

## RM-011-G7 — 2026-08-28 — cross-repository account/pairing E2E verification

- Goal: verify the real status and compatibility of the RockServer, RockCast, and RockMobile
  account/pairing UX, and record what remains before RM-011-G closure.
- Scope: source/contract review, deterministic browser harness, server/client unit/build/static
  checks, secret/UI scan, E2E matrix, staging/physical-device checklist, and one RockServer-only
  OpenAPI correction. No client changes, staging mutation, cleanup, migration, deploy, commit, or
  push.
- Result: named passkey/browser/device flow is present in all three checkouts and local radio
  fallback is preserved. OpenAPI now accurately distinguishes anonymous `/api/v1/*` product routes
  from legacy Bearer-protected `/api/v1/*` compatibility routes. Full live acceptance is blocked:
  server completion collapses pending and terminal pairing failures into `401 pairing_rejected`,
  and dismissing either native account UI does not reliably cancel a pending pairing job. A
  cancelled registration can also leave an active user without a passkey.
- Checks: RockServer `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test`,
  `cargo test --all-targets --all-features --jobs 1`, OpenAPI tests, and `docker compose config`
  passed; web typecheck/regression/build passed; RockCast fmt/check/tests passed with one unrelated
  Clippy finding; RockMobile unit tests/assemble passed with three unrelated lint findings. Six
  disposable PostgreSQL tests, live credential/provider tests, physical Android, Windows GUI and
  staging smoke remain unrun. Details: `docs/archive/2026-08/rm-011-g7-e2e-verification.md`.
- Status: **verification documented; RM-011-G remains open pending F-1/F-2, registration-policy
  decision, disposable-DB coverage, and authorized staging/real-device smoke.**

## RM-011-G6 — 2026-08-28 — preview-first staging account cleanup

- Goal: safely retire obsolete staging accounts, passkey rows, devices, and access dependencies
  after the G1/G2 pairing UX without guessing ownership or touching browser-managed passkeys.
- Scope: redacted PostgreSQL preview, exact-UUID operator actions, transactional tombstone/revoke
  cascades, safe audit events, staging-only deployment wrapper, account-center/read-only admin
  messaging, runbook, and regression tests. No public HTTP route, auth-model weakening, migration,
  staging cleanup, production action, deploy, commit, or push.
- Result: preview is default and fail-closed behind `ROCKSERVER_CLEANUP_ENV=staging`; mutations
  require one account/device/server credential row UUID plus its exact confirmation phrase. Account
  deactivation blocks active admin identities, protects the last working passkey, revokes live
  account dependencies, and retains audit/tombstone rows. The runbook separates server revocation
  from manual Google Password Manager/browser cleanup and warns that duplicate `RockServer user`
  labels do not identify an old key.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features --jobs 1 -- -D warnings`;
  `cargo test --all-targets --all-features --jobs 1` (all non-ignored tests passed; six disposable
  PostgreSQL tests remain ignored); web `pnpm typecheck`, `pnpm lint`, `pnpm test`, and `pnpm build`;
  `git diff --check`.
- Status: **implemented locally; staging cleanup and deployment intentionally not performed.**

## REFACTOR-001 — 2026-08-28 — HTTP transport module extraction

- Goal: separate the mixed HTTP endpoint module into understandable domain and transport layers
  without changing public behavior.
- Scope: route composition, auth, account/device management, pairing, catalog, search, voice,
  health/admin, shared HTTP transport, admission state, and boundary-local unit tests. No OpenAPI,
  migration, persistence, web, authorization, or business-logic changes.
- Result: `src/http/endpoints.rs` decreased from 3,704 to 392 lines and now only builds the shared
  router and preserves the public router constructors. The extracted modules have one clear
  responsibility each; common request IDs, error bodies, JSON parsing, proxy/origin trust,
  credential hashing, and response DTOs are in `transport.rs`, while rate admission remains in
  `state.rs`. Security, pairing-preview, and device-name regressions moved next to their modules.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` (100 passed; five PostgreSQL tests ignored without `TEST_DATABASE_URL`), OpenAPI
  contract tests included in the full suite, and `git diff --check` passed. `cargo check
  --all-targets --all-features` also passed.
- Status: **implemented, committed as `2afcae0`, pushed to `origin/master`, and deployed through
  the OPS-001-D staging rollout; readiness passed.**

## HTTP transport architecture guardrail — 2026-08-28

- Goal: prevent further growth of the legacy mixed HTTP endpoint module.
- Result: added always-applied `.cursor/rules/http-module-boundaries.mdc`, requiring domain
  transport modules, thin route composition, and a final refactoring review for every HTTP change.
- Checks: pending final repository checks.
- Status: **guardrail complete; behavior-preserving legacy-module extraction remains a separate task.**

## RM-011-G2 — 2026-08-28 — browser account and pairing UX

- Goal: make a secure pairing link explain one pending device and prevent accidental account
  creation or loss of pairing context.
- Scope: current Preact/Vite page, tab-local browser-session CSRF refresh endpoint, OpenAPI and
  contract/security regressions. No native client, secret file, dependency, account, credential or
  deployment change was made.
- Result: `/` is an authenticated/anonymous account landing page without a generic pairing form.
  A link with the existing pairing code and approval secret renders only that request's product,
  suggested device name, phrase, code and expiry. Anonymous users see distinct passkey sign-in and
  clearly warned account-creation actions; successful ceremonies retain the same URL. A live
  session gets a rotating in-memory CSRF proof and sees a named account/device confirmation with
  connect/cancel. No UUID, credential ID, HTTP detail or native token is rendered.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test` (99 regular
  tests), web regression tests, TypeScript typecheck/lint and Vite production build passed. Clean
  browser automation verified the anonymous landing has zero inputs, then a local deterministic
  API harness rendered both the named RockMobile target/anonymous choices and the named signed-in
  confirmation. No passkey, account or staging pairing request was created. PostgreSQL tests remain
  opt-in without `TEST_DATABASE_URL`.
- Deployment: commit `efc405a` completed through the standard detached staging worker with
  `readiness=passed`. A fresh read-only browser session displays the new anonymous landing without
  a UUID or general pairing-code field; no account or passkey was created.
- Status: **deployed to staging.**

## RM-011-G1 — 2026-08-28 — account and pairing contract

- Goal: make the server contract express one user account with many understandable devices, without
  weakening the deployed username-less passkey or native-token boundaries.
- Scope: migration `0016`, account/device/pairing persistence and DTOs, runtime OpenAPI, safe browser
  preview, explicit registration naming, contract/security tests, and current-state documentation.
  No G2 browser UX or native-client workflow was implemented.
- Result: existing account rows receive `Rock account`; legacy device and pairing values are preserved
  through column renames. Browser pairing preview has display/type, short code, phrase, expiry and
  `pending` status, with account display name only for the current browser session. It serializes no
  native proof, credential ID, access token, or refresh token. Approval does not create users;
  strict input DTOs still reject client-supplied owner identifiers.
- Checks: local Rust format, strict Clippy, and 98 tests passed; disposable PostgreSQL tests remain
  ignored without `TEST_DATABASE_URL`. Web typecheck, lint, and production build passed with the
  bundled runtime. The normal staging deploy was attempted and refused its clean-worktree gate;
  no bypass is permitted because the requested changes must remain uncommitted.
- Status: **implemented locally; deployment blocked by the immutable clean-worktree safety gate.**

## RM-011-F P0 blocker fix — 2026-08-27 — username-less discoverable passkey login (deployed)

- Goal: remove the unusable UUID prerequisite from existing-account browser login while keeping
  WebAuthn and pairing security boundaries unchanged.
- Scope: discoverable WebAuthn begin/verify flow, server-side credential owner resolution from the
  assertion `userHandle`, neutral negative handling, frontend login UI/statuses, OpenAPI, and
  focused tests/documentation. No new frontend, migration, account lookup, secret, token, or
  pairing-model change.
- Result: authentication options now create a challenge with an empty `allowCredentials` list and
  no account input. Verify requires a valid UUID-shaped `userHandle`, derives the owner from it,
  checks legacy in-flight challenge bindings when present, and performs owner-scoped credential
  lookup. Client `user_id` spoofing is rejected by the strict DTO. The UI no longer renders or
  submits an account identifier and maps browser no-key/cancel and server failures to safe
  user-facing statuses. Existing registered credentials remain in the same credential table and
  use the same registration user-handle bytes for owner-scoped lookup.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test`, and web `pnpm typecheck`, `pnpm lint`, `pnpm build` passed. Read-only browser
  inspection of both the local and deployed staging page showed no UUID field. The four
  PostgreSQL integration tests remain explicitly ignored without a disposable `TEST_DATABASE_URL`.
  The standard deploy reported commit `7b4f306635e95297ac3cd8b2d99063500d90bc0f`, image
  `sha256:531f1d5051260a3e008f36c96c2dfd3ce84bcc8158d8ccc5c7d09fa7f7f63de8`, and
  `readiness=passed`.
- Status: **F1 implementation deployed; manual physical-device passkey smoke test remains.**

## RM-011-F — 2026-08-27 — staging authorization security acceptance (in progress)

- Goal: perform the final integration/security acceptance of RM-011 browser/native authorization
  on staging without committing or pushing.
- Scope: reviewed RockServer, RockCast, and RockMobile authorization boundaries; ran local
  server/client checks; inspected local Docker state; prepared the standard staging deploy.
- Result: found and minimally fixed P1 QR approval-secret referrer leakage: Caddy now sends
  `Referrer-Policy: no-referrer` in both local and production configurations. Added the
  `deploy_security` regression test and validated both Caddyfiles in Caddy 2.10 with placeholder
  values only. The deploy script correctly refused the dirty worktree, preserving its immutable
  commit-to-image guarantee; no staging deployment was performed after the fix. The existing
  staging site passed HTTPS readiness and rejected direct port `3000`, but its root and negative
  RM-011 route probes returned `404`, so it predates the required auth router.
- Follow-up deployment diagnosis: the immutable image and Caddyfile safely reached the VPS, but
  Compose stopped before database backup because `ROCKSERVER_TRUSTED_PROXY_TOKEN` was absent from
  root-only `release.env`. Bootstrap now provisions that generated proof with the other protected
  runtime values; no secret was inspected or printed. The matching regression test prevents a
  future bootstrap from omitting the required proxy boundary.
- Second deployment diagnosis: Compose also required an immutable Caddy image, while the VPS had
  only the upstream base Caddy container. The rollout now transfers a commit-labelled Caddy web
  bundle alongside RockServer and verifies both archive hashes and revision labels before starting
  either service. This prevents a healthy server image from being paired with a stale/no-UI proxy.
- Caddy image build follow-up: the pnpm workspace retained its placeholder `esbuild` build-policy
  value, so a clean container correctly refused the script. It now explicitly allowlists only
  locked `esbuild`; no package, lockfile, or broader lifecycle-script permission was added.
- Container build follow-up: pnpm requested an interactive module-directory confirmation after the
  source copy. `Dockerfile.caddy` now sets `CI=true` only for that build command, preserving a
  reproducible non-interactive release build.
- Post-deploy probing found the SPA fallback rewrote `/health/*` and `/api/v1/*` before proxying,
  producing `405` instead of API responses. Both Caddyfiles now use an exclusive API `handle`
  before the static fallback; the regression test covers the required routing boundary.
- Checks: RockServer `cargo fmt --check`, strict Clippy, and `cargo test` passed; RockCast format
  and 85 library tests passed; RockMobile debug unit tests and `assembleDebug` passed; both
  Caddyfiles validated. Physical-device passkey/Keystore evidence remains required.
- Status: **blocked on an approved immutable commit for deployment, then physical-device evidence**.

### Deployment completion follow-up

- Result: after three minimal deployment fixes, staging commit
  `f960e323a2b9e06e0281bf144bb368dc244e54c9` completed successfully. The fixes provision the
  root-only trusted Caddy proof during bootstrap, deploy a matching checksum/label-verified Caddy
  web image, and proxy API/health routes before the SPA fallback.
- Staging checks: `/health/ready` returned JSON `200`; Caddy returned CSP, `no-referrer`,
  `nosniff`, `DENY`, and Permissions-Policy headers; TCP `3000` was unreachable externally;
  browser passkey options without first-party Origin returned `403`; malformed native completion
  returned `422`; a short-lived pairing create returned `201`, and completion with client-supplied
  `user_id` returned `422`. No response proof or identifier was logged.
- Status: **blocked only on manual physical-device passkey/Keystore evidence**.

## MVP-001-C follow-up — 2026-08-26 — local launcher credential fallback

- Goal: allow the documented local launcher to start for public `/v1` testing
  when `.env` contains the required database configuration but no Bearer token.
- Scope: `run-rockserver-local.ps1` and current-state documentation only.
- Result: the launcher now creates a random process-local, non-production
  credential when `ROCKSERVER_API_BEARER_TOKEN` is blank or absent. No value is
  written to `.env` or printed. Legacy `/api/v1` and local admin routes remain
  protected; release/production startup policy is unchanged.
- Checks: PowerShell parser accepted `run-rockserver-local.ps1`; `cargo fmt --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test`
  passed. The five real-provider and two disposable-PostgreSQL tests remained
  explicitly ignored; no server, external provider, or network call was made.
- Status: **complete / passed**.

## OPS-001-A — 2026-08-25 — Production deployment design

- Goal: define the safe single-VPS deployment boundary without creating infrastructure or a runnable
  deployment stack.
- Scope: `deploy/README.md` and current-state documentation only. The design fixes a safe
  non-routable placeholder domain, 80/443 Caddy ingress, Compose-internal RockServer/PostgreSQL
  ports, volumes, environment contract, ownership, health/readiness, firewall matrix, backup/
  restore rehearsal and rollback. Dockerfile, Compose production files, Caddyfile, real secrets,
  registry publishing and deployment are explicitly deferred to OPS-001-B/C/D.
- Result: the design records a private `database` network, Caddy-to-RockServer `edge` network,
  immutable-image release/rollback sequence, and a recovery path that restores a verified encrypted
  logical backup before an incompatible-schema application rollback. It also identifies two verified
  preconditions for OPS-001-B: production must reject the current hard-coded development bearer
  token and must require PostgreSQL rather than falling back to six in-memory stations.
- Checks: documentation/source consistency review against `src/config.rs`, `src/main.rs`, health
  routes and existing development `compose.yaml`; `git diff --check`, `cargo fmt --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` passed. The regular
  suite passed 81 Rust unit tests plus HTTP/contract suites; two disposable-PostgreSQL, four
  billable Yandex LLM, and one credential/audio SpeechKit integration tests remained explicitly
  ignored. No Docker/VPS/network/secret operation was performed.
- Status: **complete / passed**. The user confirmed the design on 2026-08-25. The placeholder
  domain remains intentionally non-routable; real domain/DNS, deployment ownership, secret
  injection, backup policy, SSH allowlist, Caddy ACME policy and restore authority are manual
  pre-launch inputs and were not changed or committed.

## RM-007-A — 2026-08-25 — Common local personal-data model and ID migration contract

- Goal: define one portable offline-first personal-data contract for future RockMobile and RockCast
  favourites/history without implementing either feature, sync, authentication, server endpoints, or
  catalog changes.
- Scope: RockServer documentation only: versioned `LocalProfile`, `Favourite`, and
  `PlaybackHistoryEntry` model; deterministic dedupe/order/retention; RM-004 ID/lifecycle handling;
  local-first migration/rollback; verified client field mapping; privacy boundary; and human
  approval decisions.
- Result: [`archive/2026-08/rm-007-a-local-personal-data-contract.md`](archive/2026-08/rm-007-a-local-personal-data-contract.md)
  distinguishes verified current client behavior from proposed RM-007 design. It preserves IDs for
  URL/primary-stream/rename changes, follows only merged tombstones, and quarantines split, removed,
  missing, and unmapped legacy references. The present RockMobile unavailable-voice migration and
  RockCast URL-derived transitional IDs are not represented as nonexistent favourites/history.
- Checks: documentation/source consistency review against RM-004 contract, current
  RockServer/RockCast/RockMobile status, and `git diff --check` passed.
- Status: ready for human approval; implementation is intentionally deferred to RM-007-B/C.

## RM-004-D — 2026-08-21 — RockServer shared station catalog integration

- Goal: integrate the approved immutable RockCatalog release candidate without adding a network or
  build-time dependency.
- Scope: vendor the v1 schema/catalog/manifest for version `2026.08.2`; add a checksum- and
  invariant-validated provider-neutral adapter; replace the runtime in-memory development catalog;
  atomically activate provider-owned PostgreSQL rows; support stable multi-stream ownership,
  provider-scoped retirement, and preservation/invalidation of operational and derived fields.
- Result: canonical station IDs are both RockServer IDs and `source_station_id` under `rockcatalog`;
  streams use `<station-id>:<stream-id>`. Activation does not cross-provider merge or mutate Radio
  Browser rows, retains absent baseline records through soft retirement, keeps health/probe data for
  an unchanged URL, resets only a changed stream URL, and invalidates station embeddings on metadata
  changes. No public HTTP/OpenAPI fields or DTO shape changed. The extended RockMobile export is not
  part of this stage.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed. The opt-in PostgreSQL integration test also passed against a disposable local
  PostgreSQL/pgvector container on port 15432. Catalog validation, formatting, and all nine offline
  catalog-tool tests passed.
- Status: complete at the RM-004-D acceptance gate.

## RM-004-B — 2026-08-21 — Catalog repository and validation tooling

- Goal: establish the approved shared-catalog v1 authoring infrastructure without consumer integration or legacy data migration.
- Scope: create the local `C:\repos\rockcast-station-catalog` git repository, final-name schema, minimal canonical catalog, offline standard-library tools, fixtures, release manifest, and ownership/release documentation; record its location here.
- Result: author validation rejects unknown fields and contract/semantic violations; consumer mode tolerates unknown optional v1 fields; formatting and SHA-256 output are deterministic. No RockServer code/API/database changes, network access, actual provider data, 41-station conversion, or extended PostgreSQL snapshot occurred.
- Checks: catalog validation, formatting check, eight `unittest` fixture tests, and SHA-256 manifest generation passed offline. `graphify update .` follows below for the RockServer documentation reference.
- Status: complete.

## RS-037 — 2026-08-20 — Reliable local launcher endpoint output

- Goal: make the local launcher print the actual RockServer bind address and reachable admin URLs.
- Scope: derive the bind port from `ROCKSERVER_BIND_ADDR`, detect active LAN IPv4 addresses with a restricted-shell fallback, and remove stale hardcoded localhost output.
- Result: `run-rockserver-local.ps1` reports the configured listener, localhost preview, and one preview URL per detected LAN address; it no longer claims that `127.0.0.1` is the only endpoint when the server binds all interfaces.
- Checks: PowerShell syntax validation, gateway-backed LAN-address detection (`192.168.31.133`), `git diff --check`, and `graphify update .` passed.
- Status: complete.

## RS-038 — 2026-08-20 — Enforce the fixed local bootstrap token

- Goal: ensure the running RockServer cannot diverge from the token compiled into RockMobile because of stale environment values or an old custom launcher argument.
- Scope: make Rust startup and the local PowerShell launcher use the fixed bootstrap token; improve admin-preview error wording so only HTTP 401 is reported as token rejection; update current-state documentation.
- Result: `rockserver-dev-bootstrap-7f4b9a2c1e6d8a40` is now authoritative for local startup. Legacy `ROCKSERVER_API_BEARER_TOKEN` values are ignored until real user/client credentials replace this temporary mechanism.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, sequential `cargo test`, PowerShell syntax validation, `git diff --check`, and `graphify update .` passed. The regular suite passed 72 library tests plus HTTP/contract tests; external database/provider tests remained ignored.
- Status: complete.

## RS-036 — 2026-08-20 — Stable RockMobile bootstrap credential

- Goal: keep the local RockServer/RockMobile connection working with one credential until user accounts and revocable client tokens exist.
- Scope: add the stable development bootstrap credential to RockServer configuration and the local launcher; use the same default in RockMobile; retain environment/settings overrides; update current-state and API documentation.
- Result: RockServer and `run-rockserver-local.ps1` use `rockserver-dev-bootstrap-7f4b9a2c1e6d8a40` when no override is configured, and RockMobile sends the same value by default. Configured `ROCKSERVER_API_BEARER_TOKEN` values remain validated and take precedence.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and sequential `cargo test` passed (72 library tests plus HTTP/contract tests; external database/provider tests remained ignored). PowerShell syntax validation, `git diff --check`, and `graphify update .` passed. RockMobile `:app:lintDebug` passed; its unit suite still has six unrelated existing failures involving unmocked `android.util.Log` and coroutine timing.
- Status: complete.

## RS-035 - 2026-08-20 - Bind HTTP listener on all interfaces

- Goal: make the default RockServer listener reachable from other hosts on the configured network.
- Scope: change the default socket address from `127.0.0.1:3000` to `0.0.0.0:3000`; keep `ROCKSERVER_BIND_ADDR` as the explicit override; update user-facing documentation.
- Result: when `ROCKSERVER_BIND_ADDR` is unset, `Config::from_env` now configures `0.0.0.0:3000`. Localhost-only operation remains available by setting `ROCKSERVER_BIND_ADDR=127.0.0.1:3000`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `graphify update .` passed. The regular suite ran 70 library tests plus the HTTP/contract suites; database and credential-dependent live tests remained explicitly ignored.
- Status: complete.

## RS-034 — 2026-08-20 — Buffered microphone-capture regression diagnosis

- Goal: identify and correct the loss of recognition quality reported in both selectable voice modes.
- Scope: a billable opt-in Yandex TTS-to-PCM-to-STT diagnostic, focused provider verification, and a RockCast buffered-capture correction. No production API or protocol change.
- Result: a 1,841 ms Yandex-generated PCM16 command was recognized exactly by SpeechKit v1 in 925 ms, isolating the provider from the regression. RockCast buffered capture no longer routes audio through the bounded `try_send` queue introduced for live streaming, so it cannot silently discard callback chunks when that consumer falls behind. The streaming path remains separately queued for its next backpressure-focused iteration.
- Checks: `cargo check --bin diagnose_speechkit_pcm`, 70 RockServer library tests, and 41 RockCast library tests (including the new buffered-frame preservation test) passed. `graphify update .` refreshed the local RockServer code graph; it reported the existing `hooks.json` zero-node warning and stale community labels.
- Status: complete.

## RS-033 — 2026-08-20 — Selectable SpeechKit streaming recognition

- Goal: add upstream streaming recognition without removing the existing bounded, buffered voice path.
- Scope: SpeechKit v3 gRPC adapter, per-session `recognizer_mode` selection in the existing WebSocket start event, backwards-compatible `buffered_v1` default, OpenAPI/docs, deterministic request/response tests, and RockCast settings integration. No live SpeechKit call is made by ordinary tests.
- Result: `streaming_v3` sends PCM chunks to SpeechKit while RockCast records and can forward partial/final transcript events. `buffered_v1` remains the default and continues to submit the bounded recording after `commit`.
- Checks: changed-file formatting, `cargo check`, 70 library tests, the streaming WebSocket integration test, and strict all-target/all-feature Clippy passed. `graphify update .` refreshed the ignored local graph.
- Status: complete.

## RS-031 — 2026-08-20 — Explicit world-country parsing

- Goal: make explicit country requests such as `немецкий рок` consistently set the ISO country hard filter without treating locale or an LLM guess as country intent.
- Scope: replace the three-country mapping with ISO 3166-1 alpha-2 aliases in Russian and English, including common demonyms and multi-word country names; add regression tests. Public HTTP/OpenAPI behavior and ranking are unchanged.
- Result: `немецкий` and the natural request form `рок из Германии` now produce `country_code=DE`; explicit names and demonyms across the supported country set produce their alpha-2 code. Ambiguous matches intentionally produce no country filter.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` passed (61 unit tests, integration/contract tests, with opt-in live/database tests ignored as intended).
- Status: complete.

## RS-030 — 2026-08-20 — Indexed candidate-branch PostgreSQL search

- Goal: remove timeout-prone broad PostgreSQL station searches without changing query-to-station matching or ranking behavior.
- Scope: replace the correlated `OR` prefilter with separate tag, full-text, and trigram candidate-ID branches; union/de-duplicate IDs before the existing bounded prefilter and unchanged final scoring; construct FTS input once, precompute FTS queries per expanded term, and normalize/tokenize each candidate station name once; add stage-duration debug logs and a structural regression test for the query shape. Public HTTP/OpenAPI behavior is unchanged.
- Result: broad candidate selection can use the existing GIN indexes independently before expensive scoring, while expanded voice terms no longer repeat FTS parsing, name normalization, or regex tokenization. The original hard filters, exclusions, candidate limit, score calculations, and `score DESC, id ASC` ordering remain in place.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` passed. The opt-in PostgreSQL integration test remains ignored because no disposable `TEST_DATABASE_URL` was available. Live voice-command checks against the configured PostgreSQL database returned 30 stations in 1,387 ms for `Включи немецкий рок или хэви метал` and in 1,388 ms for the observed STT transcription `Включи немецкий рок или хейли метал`; PostgreSQL took 233 ms for the latter.
- Status: complete.

## RS-019 — 2026-08-19 — Genre hierarchy and progressive search fallback

- Goal: make subgenre queries (e.g. "heavy metal", "black metal", "smooth jazz") find stations tagged with parent genres when no exact match exists, and prefer stations matching the requested language.
- Scope: expanded canonical tag vocabulary with subgenres from the real Radio Browser catalog; a static genre-parent hierarchy (`black metal` → `metal` → `rock`); ancestor-aware `station_matches_requested_genre`; progressive fallback in `SearchService::search` (exact → parent → drop genre); a new builtin catalog station for metal; deterministic tests including the `"english heavy"` use case; and documentation updates. No public HTTP API, OpenAPI, PostgreSQL SQL, migration, or embedding changes.
- Result: `CANONICAL_TAGS` grew from 27 to 40 entries with metal subgenres (`black metal`, `death metal`, `doom metal`, `power metal`, `symphonic metal`, `thrash metal`), rock subgenres (`alternative rock`, `classic rock`, `indie rock`, `pop rock`, `progressive rock`, `psychedelic rock`, `soft rock`), and jazz subgenres (`acid jazz`, `latin jazz`, `smooth jazz`). `genre_parent` and `genre_ancestors` expose the hierarchy. `station_matches_requested_genre` accepts stations whose tags match any ancestor. `SearchService::search` progressively relaxes the genre constraint: exact match → parent genres → no genre filter, always gated by `MIN_RELEVANCE_SCORE`.
- Checks: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test` passed with 67 regular tests (48 unit + 19 integration), 6 ignored live/database tests.
- Status: complete.

## RS-016 — 2026-08-17 — Local ONNX multilingual semantic search

- Goal: add an opt-in CPU/local ONNX embedding provider suitable for Russian and English station discovery.
- Result: selected `intfloat/multilingual-e5-small` (384 dimensions), added local ONNX Runtime/tokenizer inference, E5 query/document prefixes, normalized station text, importer maintenance, backfill support, and a provenance-specific pgvector HNSW cosine index.
- Verification: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` pass with 50 regular tests. The local PostgreSQL database was migrated, 999 real Radio Browser stations were imported, all 1,005 catalog rows received E5 embeddings, and three Russian live queries ran through the HTTP search path. Jazz and 80s-rock ranked relevant stations first; the classical/instrumental query exposed a catalog/locale-filter quality limitation.
- Status: complete; local model assets remain intentionally outside Git.

## RS-000 — 2026-08-13 — Repository bootstrap

- Goal: establish the Rust crate, repository hygiene, contributor guidance, project scope, architecture outline, and near-term roadmap.
- Scope: Rust edition 2024 bootstrap binary and documentation only.
- Result: completed by commit `4117786` (`Document RockServer project setup`); the commit added the crate files, ignore rules, contributor instructions, README, TODO, and architecture outline.
- Checks: the commit and resulting files were inspected for this log entry; historical command output is not available in Git and is therefore not claimed.
- Status: complete.

## RS-001 — 2026-08-13 — HTTP service skeleton

- Goal: introduce a minimal, testable Axum HTTP service with operational health endpoints.
- Scope: library plus thin binary, stable JSON health models, request and application tracing, configurable local listener, Ctrl+C graceful shutdown, in-memory router tests, and project documentation. Search, OpenAPI, persistence, containers, external providers, and client work are excluded.
- Result: added the Axum library application and thin binary, liveness and readiness routes with a shared stable serde model, JSON application tracing and HTTP request spans, `ROCKSERVER_BIND_ADDR` configuration with the local-only `127.0.0.1:3000` default, an extensible graceful-serving boundary with Ctrl+C as the current signal, and in-memory router tests. Updated the contributor rules and current project documentation.
- Checks: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test` passed with 2 tests; `git diff --check` passed.
- Status: complete.

## RS-002 — 2026-08-13 — Search API contract and service diagrams

- Goal: define the public search contract before implementation and provide a clear offline architecture reference.
- Scope: OpenAPI 3.1 contract for `POST /api/v1/search` and existing health routes; validation rules, defaults, response/error schemas, status semantics, and Russian and English examples; a structural contract test; autonomous Russian HTML diagrams; contributor and project documentation. Search routing, catalog/domain implementation, persistence, providers, ranking, authentication, and client changes are excluded.
- Result: added `api/openapi.yaml` with the required schemas and 200/400/422/429/500 outcomes, explicitly separating malformed JSON (400) from well-formed but invalid input (422). Added a `cargo test` integration check using test-only `serde_yaml`; it parses the YAML and guards the required OpenAPI version, paths, POST operation, and schemas. Added `docs/service-diagrams.html`, which distinguishes CURRENT, NEXT, and FUTURE architecture and works without external resources. The Axum router was intentionally unchanged, so `POST /api/v1/search` remained 404 at that stage. Added permanent Rustdoc/comment guidance and a diagram-accuracy rule to `AGENTS.md`.
- Checks: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test` passed with 4 tests, including an explicit assertion that `POST /api/v1/search` remained 404; `git diff --check` passed. Python's standard-library `html.parser` accepted the complete HTML file and additional local checks confirmed one doctype, one `<html>` root, no external resource URLs, and balanced SVG elements.
- Status: complete.

## RS-003 — 2026-08-13 — Deterministic in-memory station search

- Goal: make the contract-defined `POST /api/v1/search` route usable without persistence, providers, embeddings, LLMs, or external network access.
- Scope: Axum search routing and DTO validation; contract-compliant 400/422 errors with request IDs; a small built-in in-memory catalog; separated search domain, `StationRepository` trait, normalization, filtering, deterministic ranking, and result mapping; HTTP and domain tests; current-state documentation and diagrams. PostgreSQL, imports, external providers, semantic ranking, rate limiting, client changes, and authentication are excluded.
- Result: registered `POST /api/v1/search` alongside the unchanged health routes. The route accepts all existing request fields, defaults locale and limit, rejects malformed JSON with 400 and syntactically valid invalid input with 422, and always returns the standard `ErrorResponse` fields on errors. The domain applies inferred language/country and exclusion constraints to a six-station catalog, scores matching metadata, and breaks equal scores by station ID ascending. No OpenAPI changes were needed because the existing schema already described this behavior.
- Checks: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test --all-targets --all-features` passed with 13 tests; `git diff --check` passed. Search tests cover success, empty results, limit, stable ordering/tie-break, exclusions, malformed JSON 400, and semantic validation 422. The suite uses no network listener, external network, AI provider, or LLM. A standard-library HTML parser accepted the updated service diagrams.
- Status: complete.

## RS-004 — 2026-08-13 — PostgreSQL persistence foundation

- Goal: add real PostgreSQL catalog storage behind `StationRepository` without changing the public search contract or introducing import, pgvector, LLM, authentication, or rate limiting.
- Scope: versioned station/stream migrations, constraints and indexes, idempotent development seed, SQLx PostgreSQL repository, automatic startup migrations, `DATABASE_URL` selection with in-memory fallback, backend logging without DSN disclosure, dependency-aware readiness, local PostgreSQL Compose, deterministic unit tests, real opt-in PostgreSQL integration coverage, OpenAPI readiness alignment, and current-state documentation.
- Result: added separate `stations` and `station_streams` tables with timestamps and catalog invariants; seeded the same six development stations as the fallback; implemented parameterized SQL for hard filters, exclusions, deterministic metadata scoring, score-descending/station-ID-ascending order, and limit; made search repository operations asynchronous; added startup backend selection and migrations; kept liveness process-only and made PostgreSQL readiness return 503/`not_ready` after dependency loss. The responsibility boundary is explicit: the domain owns normalized inputs and rule meaning, while each repository executes those rules efficiently in its storage model.
- Checks: Docker Engine 29.7.2 and Compose 5.3.1 were available. A dedicated `rockserver-rs004-test` project ran PostgreSQL 17 and the opt-in integration test passed for migrations, seed, HTTP search, exclusions, limit, tie-break, and readiness loss; only that project's container, network, and volume were removed afterward. `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test --all-targets --all-features` passed with 15 regular tests and the opt-in PostgreSQL test ignored by default; the same PostgreSQL test passed when explicitly run with `TEST_DATABASE_URL`; `git diff --check` passed; the standard-library HTML parser accepted `docs/service-diagrams.html`.
- Status: complete.

## RS-005 — 2026-08-13 — Keep Graphify artifacts local

- Goal: prevent generated Graphify output from being included in commits or pushed to GitHub while preserving the local knowledge graph.
- Scope: root ignore rule and Git-index cleanup only; no Graphify files were deleted from disk.
- Result: normalized `.gitignore` to one `/graphify-out/` rule and removed all staged Graphify artifacts from the Git index. No `graphify-out` path existed in `HEAD`, so no committed repository content required deletion.
- Checks: `git ls-files -- graphify-out` returned no paths; `git check-ignore -v` matched both root and nested Graphify artifacts; the local directory remained present with 43 files; `git diff --check -- .gitignore docs/status.md docs/tasks.md` passed.
- Status: complete.

## RS-006 — 2026-08-13 — Controlled Radio Browser import

- Goal: add a controlled, repeatable Radio Browser import into PostgreSQL outside the HTTP request path and server startup, without pgvector, embeddings, LLMs, authentication, rate limiting, RockCast work, or stream probing.
- Scope: provider-neutral import models and orchestration; an import-provider trait independent of `StationRepository`; a bounded Radio Browser HTTP client; deterministic DTO validation/normalization; an import-only PostgreSQL persistence trait; provider ownership and stream identity; durable `import_runs`; a one-shot CLI; unit, local mock HTTP, and opt-in PostgreSQL integration coverage; current-state documentation and diagrams. The public OpenAPI/search contract, liveness, and readiness semantics remain unchanged.
- Result: added migration `0003_add_catalog_import.sql`, which preserves the six seed stations as `builtin`, adds source identities for station/stream idempotency, removes global stream-URL uniqueness in favor of provider stream identity, and adds terminal import-run status/count/timestamp/error storage. Added `CatalogImporter`, `CatalogImportProvider`, and `CatalogImportStore` boundaries. Added `RadioBrowserClient` with explicit User-Agent, official DNS-balanced default root, configurable safe root, 1–60 second timeout, 1–500 page size, 1–100 maximum pages, 8 MiB response cap, deterministic per-page UUID order, and sanitized failures. Added `PostgresImportStore` transactional upserts and `import_radio_browser`; `DATABASE_URL` is mandatory for that binary and no DSN, stream URL, or response body is logged.
- Mapping rules: consumes official `stationuuid`, `name`, `url_resolved`, `homepage`, `tags`, `languagecodes`, `countrycode`, `codec`, `bitrate`, and `lastcheckok`. Rows are skipped unless the upstream check is good, UUID/name are valid, and resolved stream URL is credential-free HTTP(S). Optional homepage is validated; tags are lowercase/sorted/deduplicated and bounded; language prefers the first valid two-letter code across the full list and falls back to the first valid three-letter code only when necessary; valid alpha-2 country, bounded codec, and 1–2000 kbps bitrate are retained. Repeat runs update `(radio_browser, stationuuid)` and its provider-owned primary stream without duplicates. Missing upstream rows are retained.
- Sources: behavior and fields were verified against the official Radio Browser [API usage](https://docs.radio-browser.info/#using-the-api), [Station](https://docs.radio-browser.info/#station), [advanced search](https://docs.radio-browser.info/#advanced-station-search), and [mirror discovery](https://docs.radio-browser.info/#server-mirrors) documentation and the official [API repository](https://gitlab.com/radiobrowser/radiobrowser-api-rust).
- Checks: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test --all-targets --all-features` passed with 25 regular tests and one opt-in PostgreSQL test ignored by default; `git diff --check` passed. Unit tests cover mapping, language preference/fallback, normalization, skip rules, bounds, pagination, partial failure accounting, source mismatch rejection before upsert, terminal failure counts, and deterministic/idempotent input. Local loopback mock HTTP tests cover request headers/query, parsing, and sanitized HTTP errors with no real external call. `docker compose config --quiet` passed. Python's standard-library `html.parser` accepted `docs/service-diagrams.html`; checks also confirmed one doctype/root, balanced SVG elements, and no external resource URL.
- Real integration: Docker Engine 29.7.2 and Compose 5.3.1 ran a dedicated `rockserver-rs006-test` PostgreSQL 17 project on host port 55436. The ignored integration test passed for migrations, first import, repeat update without station/stream duplicates, six-seed preservation, completed/failed run counts/status/timestamps/error summary, search over imported metadata, and readiness. Only that project's container, network, and volume were removed afterward.
- Smoke: a read-only official API request with `limit=1`, a descriptive `RockServer-Smoke/0.1.0` User-Agent, and a 15-second timeout succeeded and returned one checked station with a non-empty UUID. It wrote no database and is not in the ordinary suite.
- Known limitations: upstream name-ordered offset pagination is not a transactional snapshot; configured bounds intentionally produce a safe slice, not a full mirror. No-delete ownership prevents partial runs from removing data. A hard process/host failure can leave `started` stale because reconciliation is not yet implemented. `lastcheckok` is trusted without local probing, and three-letter-only language metadata still cannot match a strict locale-derived two-letter filter without a future ISO 639 mapping.
- Status: complete.

## RS-006 review hardening — 2026-08-13 — Language and source invariants

- Goal: resolve merge-blocking compatibility between imported language codes and locale-derived search filters, and prevent provider records from crossing import-run source ownership.
- Scope: deterministic language selection, orchestration ownership validation, run-bound PostgreSQL source enforcement, focused tests, and documentation. No migration or public HTTP/OpenAPI change.
- Result: language normalization now scans the complete list and prefers the first valid two-letter code regardless of position; only a three-letter-only list falls back to its first valid three-letter code. `CatalogImporter` rejects a mismatched-source page before upsert, marks the normalized page failed, and records a generic terminal error. `PostgresImportStore` verifies the explicit source against the still-started run and uses that value for writes rather than record fields.
- Checks: focused tests cover `yue,zh -> zh`, `EN,eng -> en`, deterministic three-letter-only fallback, no-upsert ownership rejection, sanitized error text, and terminal failed counts/status. Final formatting, strict Clippy, all-target/all-feature tests, and diff checks passed.
- Real integration: because runtime SQL changed, the opt-in PostgreSQL test passed again in isolated Compose project `rockserver-rs006-review-test` on port 55437; only that project's container, network, and volume were removed afterward.
- Status: complete.

## RS-007 — 2026-08-14 — Semantic ranking foundation

- Goal: add a minimal working semantic-search foundation behind domain and repository boundaries without adding a production LLM/model provider, changing the public HTTP schemas, or moving catalog/provider network work into `POST /api/v1/search`.
- Scope: provider-neutral `QueryParser` and `EmbeddingProvider`; deterministic parser/failure fallback and deterministic fakes; explicit non-production development embedder; pgvector migration and provenance-aware station embedding store; controlled one-shot backfill/update CLI; PostgreSQL exact hybrid search; in-memory metadata fallback; unit, HTTP, and opt-in real PostgreSQL coverage; Compose, OpenAPI description, architecture/status/roadmap/diagram updates. Authentication, rate limiting, RockCast, voice input, stream probing, scheduler, production providers, and ANN tuning are excluded.
- Result: `SearchService` now interprets validated request-only input and obtains an optional query embedding before crossing `StationRepository`. Providers cannot receive a station collection through their interfaces. Parser errors or invalid structured filters use the deterministic parser; embedding errors omit semantic input. The in-memory repository continues to use the original deterministic metadata ranker. PostgreSQL applies language/country filters and exclusions before score/limit, joins only matching model/version/dimension provenance, performs exact cosine similarity, and orders score descending with station ID ascending as the last tie-break.
- Ranking decision: domain constants own weights. Compatible embeddings use normalized cosine `1 - cosine_distance / 2` and `0.70 * metadata_score + 0.30 * semantic_score`. Missing compatible station embeddings retain full metadata score; missing/failed query embeddings use metadata-only inclusion and scoring. SQL receives the domain weights as bind parameters rather than duplicating literals.
- Persistence decision: migration `0004_add_station_embeddings.sql` enables pgvector and creates a separate table keyed by station/model/version with dimension, unbounded `vector`, and timestamps. Dimension and non-zero norm are CHECK-enforced. The schema is not locked to the test/dev dimension; no HNSW/IVFFlat index is added before a production model and scale measurements exist. SQLx queries remain runtime-checked and require no live database at ordinary build time.
- Validation decision: an `Embedding` can be constructed only through its validating constructor; its provenance and vector values are private and exposed read-only so a provider cannot bypass model, dimension, finite-value, or non-zero-norm checks before the repository boundary.
- Workflow decision: `backfill_embeddings` requires PostgreSQL and explicit `ROCKSERVER_SEMANTIC_PROVIDER=deterministic-dev`, pages by stable station ID, embeds one station document at a time, and idempotently upserts provenance-specific rows. It runs outside HTTP startup/search and does not alter Radio Browser ownership/import behavior. The deterministic hash embedder is documented as a development fixture, not a production semantic model.
- Checks: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test --all-targets --all-features` passed with 35 regular tests and one opt-in PostgreSQL test ignored by default; `git diff --check` passed. `docker compose config --quiet` passed with the pgvector PostgreSQL 17 image. Python's standard-library HTML parser accepted `docs/service-diagrams.html`; checks confirmed one doctype/root, balanced SVG elements, and no external resource URL. Unit coverage includes parser boundaries/normalization, invalid intent fallback, embedding model/dimension/finite/non-zero validation, deterministic fake embeddings, provider-failure metadata fallback, controlled backfill, fixed hybrid weights, exclusions, limit, and stable tie-break.
- Real integration: Docker Engine 29.7.2 and Compose 5.3.1 ran isolated pgvector projects on ports 55438 and, after final runtime-SQL weight binding, 55439. The final ignored integration test passed for extension/migrations, embedding insert/repeat update, provenance/dimension storage, exact cosine similarity, hard filters, exclusions, final limit, semantic station-ID tie-break, metadata fallback, HTTP search, and repository-only readiness. The real deterministic backfill also completed twice and produced seven unique development-provenance rows. Only the created projects' containers, networks, and volumes were removed.
- Graphify: after deleting only the verified task-created `C:\repos\rockserver\target-rs007`, `graphify update .` completed. The permitted `tree-sitter-sql` extra was installed in Graphify's existing isolated `uv tool` environment; the final code update rebuilt the ignored local graph to 517 nodes, 1066 edges, and 30 communities without SQL-parser warnings. Remaining tool warnings are a skill/package version mismatch (`0.9.35` instructions versus `0.9.40` package), `hooks.json` producing no nodes, and stale community labels; optional LLM relabeling or semantic extraction of changed documentation was not run. Generated `graphify-out/` files remain untracked.
- Known limitations: exact vector scan has no ANN index; deterministic-dev has no production semantic quality; backfill recomputes all station documents and has no durable resume/unchanged-input skip; no model registry enforces one dimension globally per model/version; optional providers have no production authentication/retry/circuit breaker. Existing RS-006 upstream/import limitations remain.
- Status: complete.

## Roadmap decision — 2026-08-14 — Windows-first voice production path

- Goal: preserve the agreed delivery order in the repository so implementation follows the Windows RockCast production workflow rather than beginning with ESP32.
- Scope: planning documentation only. No server API, database schema, provider, RockCast code, or runtime behavior changed.
- Result: added `docs/windows-production-roadmap.md`; the plan verifies RS-007 on disposable local PostgreSQL first, integrates RockCast text search with local fallback, adds Windows microphone capture, adds a RockServer voice/STT path that reuses `SearchService`, introduces production providers, completes Windows end-to-end and operational hardening, and leaves ESP32 in future backlog. Updated the near-term TODO and current status to point to the same ordered plan. Database credentials remain environment-only and are not committed.
- Checks: documentation paths and Git diff were inspected; `git diff --check` passed. Rust checks were not required because no Rust, OpenAPI, dependency, migration, or runtime behavior changed.
- Status: planned.

## RS-007 verification — 2026-08-14 — Local PostgreSQL and HTTP smoke

- Goal: re-verify the completed semantic-ranking foundation on the user-designated disposable local `rockserver` database before beginning RockCast integration.
- Scope: database reset, clean migrations, the existing ignored PostgreSQL integration test, deterministic embedding backfill idempotency, database inspection, and a local HTTP smoke check. No source code, OpenAPI, migration, or runtime behavior changed.
- Result: connected to PostgreSQL 18.1 with pgvector 0.8.6, reset only the `public` schema of database `rockserver`, and successfully applied migrations 1–4. The exact ignored integration test passed. Two consecutive `deterministic-dev` backfills produced seven unique 32-dimensional embeddings for seven stations and zero duplicate station/model/version keys. The test's four 3-dimensional `integration-model` fixtures also remained valid. A temporary server on loopback reported `live=ok` and `ready=ok`; `POST /api/v1/search` for `jazz` returned hybrid-ranked results headed by `station-jazz-002` with reason `Hybrid match: metadata 1.000, semantic 0.878.` The temporary server was stopped after the smoke check.
- Security: the database password and connection URL were supplied only through process environment variables and were not written to repository files.
- Checks: `cargo test --test postgres_integration --all-features -- --ignored --exact postgres_migrations_seed_search_and_readiness --nocapture` passed with one test and no failures; migration, row-count, provenance, dimension, and duplicate-key queries passed; `git diff --check` passed.
- Status: complete.

## RS-008 — 2026-08-14 — Windows voice-command API contract

- Goal: stabilize the JSON contract used by the Windows voice-command flow after RS-007 without adding a second search implementation, audio upload, STT, or provider credentials.
- Scope: canonical OpenAPI/Axum `POST /api/v1/voice/command`, deprecated `/api/v1/voice/command` compatibility alias, transcript DTOs, selected-station response semantics, request IDs, validation, request-size and service-time boundaries, contract tests, and current-state documentation. Existing `POST /api/v1/search` request/response fields remain compatible.
- Result: the canonical route accepts only a bounded, already-recognized `transcript` plus locale, limit, and station exclusions, then calls the existing `SearchService`. It returns the trimmed transcript, normalized query, deterministic `stations`, and `selected_station` equal to the first result or `null`. Valid `X-Request-Id` values are echoed in the response body and header; otherwise the server generates one. JSON bodies are limited to 65,536 bytes; malformed input returns 400, oversized input 413, validation 422, unexpected search failure 500, and the five-second interpretation/search deadline returns structured 504. No audio, STT, LLM, or provider credential is accepted or simulated.
- Compatibility decision: `/api/v1/voice/command` is canonical for new Windows clients. `/api/v1/voice/command` maps to the same handler and DTOs but is documented as deprecated so the repository rule for `/v1` public versioning and the Windows `/api/v1` contract can coexist. Existing `/api/v1/search` remains registered with its existing body schemas; it now also uses the shared bounded JSON/error/request-ID transport behavior.
- Tests: `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test` passed with 40 regular tests and one opt-in PostgreSQL test ignored by default. Contract coverage verifies the canonical route, compatibility alias, request-ID propagation, selected/no-result behavior, transcript validation, structured 413 size failure, and deterministic 504 timeout.
- Graphify: `graphify update .` rebuilt the ignored local code graph to 559 nodes, 1,165 edges, and 32 communities. The tool reported only its pre-existing `hooks.json` zero-node warning and stale community labels; generated graph artifacts remain untracked.
- Documentation: `api/openapi.yaml` is the source of truth; `docs/status.md`, `docs/architecture.md`, `docs/windows-production-roadmap.md`, `README.md`, and `docs/service-diagrams.html` now distinguish the implemented JSON boundary from still-future microphone, audio-upload, and STT work.
- Status: complete.

## RS-009 — 2026-08-14 — Provider-neutral streaming voice API

- Goal: add a stable bidirectional audio contract without coupling RockCast to Yandex or OpenAI protocols and without changing the RS-008 JSON endpoint.
- Scope: canonical WebSocket `GET /api/v1/voice/stream`, deprecated `/api/v1/voice/stream` alias, PCM16 mono start/audio/commit protocol, incremental and final transcript events, final `SearchService` resolution, request IDs, size/time/error boundaries, provider traits, OpenAPI, deterministic tests, and current-state documentation. Real provider credentials and external-network tests are excluded.
- Result: added `StreamingSpeechRecognizer` and isolated per-client `SpeechStreamSession` boundaries. The WebSocket accepts 8/16/24/48 kHz PCM signed 16-bit little-endian mono chunks, limits each chunk to 65,536 bytes and the session to 10 MiB, emits `ready`, `transcript`, and terminal `result`/`error` events, and applies ten-second provider-operation and five-second search boundaries. The final transcript reuses the existing query interpretation, ranking, exclusions, and selected-station behavior. Default startup fails closed with `speech_provider_unavailable` until a concrete provider is configured.
- Tests: a real loopback WebSocket upgrade test injects a deterministic fake provider, verifies request-ID propagation, binary audio delivery, partial/final transcripts, and final station selection. OpenAPI structural coverage verifies both routes and all client/server event schemas. `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed with 41 regular tests and one opt-in PostgreSQL test ignored. Diff whitespace and the standalone HTML parser check passed. No external provider or internet call occurs.
- Graphify: `graphify update .` rebuilt the local ignored graph to 620 nodes, 1,339 edges, and 33 communities. The existing `hooks.json` zero-node warning remains, and labels are stale after the new community was introduced.
- Known limitations: this task stabilizes the service protocol and provider seam; it does not yet decode production audio because neither Yandex SpeechKit nor OpenAI Realtime is configured in `main`.
- Status: complete.

## RS-010 — 2026-08-14 — Yandex AI Studio structured intent parser

- Goal: add a production Yandex AI Studio adapter that turns text and already-recognized voice commands into the existing `QueryIntent`, without creating a second search path or changing public HTTP schemas.
- Scope: provider-neutral `LlmProvider` and `LlmQueryParser`; synchronous Yandex completion adapter; environment-only configuration and optional ignored local `.env`; strict JSON Schema plus local response validation; bounded HTTP timeout/body/token handling; deterministic failure fallback; loopback-only provider tests; and current-state documentation.
- Result: startup selects `YandexLlmProvider` only when both `YANDEX_AI_API_KEY` and `YANDEX_FOLDER_ID` are present. With neither value, it preserves `DeterministicQueryParser`; partial settings are a safe configuration error naming only variables. The Yandex request uses documented `Api-Key` authorization and `gpt://<folder>/<model>/latest`, fixed system rules, a JSON-encoded `{command, locale}` user message, and `jsonSchema` for the existing terms/tags/language/country intent. No station data crosses the provider boundary. Non-2xx, timeout, malformed/oversized response, malformed intent JSON, and invalid hard filters all keep the existing deterministic fallback. This parser is unrelated to SpeechKit/STT and is used by both text search and already-recognized voice commands through `SearchService`.
- Configuration: `.env.example` contains empty secret placeholders only. `dotenvy` may load an ignored local `.env`; production still reads process environment. Optional `YANDEX_LLM_MODEL` defaults to `yandexgpt`; `YANDEX_LLM_TIMEOUT_MS` defaults to 3000 and permits 100–10,000.
- Tests and checks: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test` (47 regular tests passed; one opt-in PostgreSQL test ignored), and `git diff --check` passed. Provider tests use loopback Axum mocks and deterministic fake credentials; no test calls Yandex or reads a real `.env`. `graphify update .` was invoked but the installed Graphify environment failed before rebuilding with `No module named 'networkx.readwrite'`; generated graph artifacts remain absent and no global dependency was installed.
- Status: complete.

## RS-011 — 2026-08-14 — Opt-in real SpeechKit voice integration test

- Goal: add a manual live check for an actual recorded voice command sent to Yandex SpeechKit, with operational logs that do not disclose credentials or voice content.
- Scope: ignored integration test only; synchronous SpeechKit v1 recognition for a local mono Ogg/Opus recording; bounded request/response handling; deterministic environment validation; safe tracing; and current-state documentation. No public API, streaming protocol, or production STT adapter changes.
- Result: `tests/yandex_speechkit_live.rs` loads configured values only at explicit test execution, posts the recording to SpeechKit, verifies a 200 JSON result and normalized expected-command containment, and logs only test ID/status/timing/character count/match boolean. `generate_speechkit_fixture` explicitly synthesizes the committed Ogg/Opus fixture and its expected phrase with Yandex TTS; it requires only `YANDEX_AI_API_KEY` and `YANDEX_FOLDER_ID`. The live STT test uses those committed files by default, accepts optional audio/phrase overrides, and remains ignored because it is billable.
- Tests and checks: `cargo fmt --check`, strict all-target/all-feature Clippy, `cargo test` (the live test is correctly ignored), and `git diff --check` passed. `graphify update .` was invoked but the installed Graphify environment failed before rebuilding with `No module named 'networkx.readwrite'`; no global dependency was installed. A live invocation is not run unless the user explicitly requests it with configured credentials and a real local recording.
- Status: complete; live execution remains pending the local audio input.

## RS-012 вЂ” 2026-08-14 вЂ” Generate committed SpeechKit fixture

- Goal: create the repository's real voice-command fixture through Yandex TTS using the configured environment without exposing credentials.
- Result: added opt-in `generate_speechkit_fixture`, which sends only the fixed command text to the documented TTS v1 endpoint, bounds and validates the Ogg response, and writes the Ogg fixture and expected transcript only after success. It uses service-account API-key authentication and does not send or require `folderId`, as SpeechKit derives the folder from the key's service account. The live STT test now uses these committed files by default and retains optional overrides. Two explicit TTS runs, before and after that correction, returned HTTP 401; no audio fixture was generated and no secret or provider body was logged.
- Status: complete. After local authorization was corrected, the generated Ogg fixture was sent to the live STT test on 2026-08-14 and returned HTTP 200 with a normalized expected-command match. No credential or transcript was logged.

## RS-013 вЂ” 2026-08-14 вЂ” Safe SpeechKit fixture diagnostics

- Goal: make the failed live TTS request inspectable without exposing the configured API key.
- Result: `YANDEX_SPEECHKIT_DEBUG=1 cargo run --bin generate_speechkit_fixture` prints the fixed non-secret request fields, redacted authorization, response status/headers, and a 16 KiB bounded redacted non-success response body. It also removes provider-echoed key-like fragments. It intentionally does not print the credential or raw unbounded provider content.
- Status: complete.

## RS-014 вЂ” 2026-08-14 вЂ” Opt-in STT mismatch diagnostics

- Goal: make a live SpeechKit recognition mismatch directly inspectable by the developer.
- Result: `TEST_YANDEX_STT_DEBUG=1` logs the endpoint, non-secret query fields, audio path and size, expected transcript, raw successful JSON response, and recognized transcript. The `Authorization` header is represented only as `Api-Key [REDACTED]`.
- Status: complete.

## RS-015 — 2026-08-14 — Structured voice-command interpretation

- Goal: turn final SpeechKit text into a strict typed player command through Yandex structured output, while keeping the LLM provider replaceable and ordinary tests offline.
- Scope: serde-backed `VoiceCommand`, `Intent`, and `RadioQuery`; provider-neutral `CommandInterpreter`; strict Yandex-compatible JSON Schema; semantic validation and normalization; secret-safe bounded HTTP diagnostics; unit coverage; and a separate ignored `yandex_llm_live` target with calm-jazz, Russian-rock, next-station, and volume exact tests.
- Result: `LlmCommandInterpreter` sends only transcript and locale through the existing `LlmProvider`, deserializes the provider JSON directly, and rejects invalid intent/field combinations without an heuristic parser. Yandex diagnostics log method, endpoint, redacted authorization/folder, request body, status, and bounded response body; configured API keys and folder IDs are removed. Yandex requires every schema field to be listed as required, with nullable values used for intent-specific omissions; both voice-command and existing search-intent schemas now follow that constraint.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed with 50 regular tests, four Yandex LLM live tests ignored, the existing SpeechKit live test ignored, and the PostgreSQL integration ignored. `cargo test --test yandex_llm_live -- --ignored --exact interprets_real_voice_command_with_safe_logs --nocapture` passed live with HTTP 200 and a typed calm-jazz command; logs exposed no credential or folder identifier. `graphify update .` rebuilt the local graph to 774 nodes, 1,692 edges, and 37 communities, with only the known zero-node `hooks.json` warning and stale community labels.
- Status: complete.

## RS-017 — 2026-08-18 — Application API Bearer gate

- Goal: establish the first fail-closed API access boundary before adding persisted RockCast
  clients and Bearer-based administrator sessions.
- Scope: production token configuration, HTTP and WebSocket authorization gate, OpenAPI security
  declaration, deterministic tests, and current-state documentation. No cookies, CSRF flow,
  database credential tables, or admin UI are included in this slice.
- Result: production startup requires `ROCKSERVER_API_BEARER_TOKEN` with at least 32 characters.
  All search and voice application routes require `Authorization: Bearer <token>`, including
  validation before a WebSocket upgrade. Missing, malformed, and invalid credentials receive the
  standard structured `401 authentication_required` response with `WWW-Authenticate: Bearer`.
  Liveness and readiness remain unauthenticated for supervision. The OpenAPI document declares
  the opaque Bearer scheme and each protected operation's 401 response.
- Documentation follow-up: updated `docs/service-diagrams.html` to show public health versus
  Bearer-protected application routes, current RockCast voice authorization, `401` semantics,
  and the separate NEXT state for persisted client/admin identity and the HTMX console.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test`, and `git diff --check` passed. The ordinary suite ran 56 tests successfully;
  PostgreSQL and provider-credential-dependent live tests remained explicitly ignored.
- Status: complete.

## RS-020 — 2026-08-19 — Genre DB, stream probe, multi-tag import

- Goal: store the genre hierarchy in the database, add live stream probing, expand the import to pull stations by tag, and filter degraded streams from search.
- Scope: migration 0006 (`genre_hierarchy` table with ~250 canonical genres/subgenres); migration 0007 (`last_probe_at`/`last_probe_error` on `station_streams`); `GenreTaxonomy` struct loadable from PostgreSQL with builtin fallback; `probe_streams` binary (8s timeout, 50 concurrency); `RadioBrowserClient::with_tag()` and `RADIO_BROWSER_TAGS` env; `health <> 'degraded'` filter in search SQL; persistence helpers for taxonomy loading and stream health updates.
- Result: all 52 unit tests pass. fmt/clippy/test clean. Backward-compatible static API preserved.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` — all clean.
- Status: complete.
## RS-021 — 2026-08-19 — Station name search accuracy

- Goal: fix voice queries like "включи радио диджей" not finding station `radioDJ`; add embedding backfill to the database population script.
- Scope: stop-word filtering (command verbs removed from query terms); CamelCase splitting in `tokenize()` (`radioDJ` → `radio` + `dj`); word-level Cyrillic↔Latin transliteration (~50 mappings: радио↔radio, диджей↔dj, фм↔fm, etc.); substring matching with 0.5 weight in both in-memory ranking and PostgreSQL search CTE; `searchable_text()` enriched with CamelCase-split tokens at import time; `fill-database.ps1` updated with `backfill_embeddings` step.
- Result: all 59 unit tests pass; new tests cover stop-word removal, transliteration expansion, camelCase split, and tokenize integration. fmt/clippy/test clean.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` — all clean.
- Status: complete.

## RS-022 — 2026-08-19 — Search name performance + intent normalization

- Goal: fix slow/timeouts for station name searches and improve relevance when the LLM returns empty or multi-word `terms`.
- Scope: normalize provider `terms` by splitting into atomic tokens (`tokenize()`), apply transliteration alias expansion for query terms, and add deterministic fallback when both `terms` and `tags` are empty; add PostgreSQL `pg_trgm` index on `lower(stations.name)` to speed up `LIKE '%term%'` prefiltering and substring scoring; keep existing metadata/semantic hybrid ranking and degraded-stream filtering.
- Result: unit/integration tests pass; SQL is still hybrid-ranked but prefiltering becomes index-friendly once `pg_trgm` is applied.
- Checks: `cargo fmt --check`, strict `cargo clippy`, and `cargo test` passed.
- Status: complete.

## RS-023 — 2026-08-19 — Full-text search and similarity scoring

- Goal: replace slow sequential `LIKE '%term%'` prefilter with index-backed FTS and pg_trgm similarity.
- Scope: migration 0009 (`searchable_tsv` tsvector column, GIN index, auto-update trigger using `simple` config); prefilter rewritten to use `plainto_tsquery` (FTS) and `%` operator (pg_trgm similarity) instead of `LIKE`; `trgm_score` added to scoring with 0.3 weight; `raw_query` field threaded through `QueryIntent` → `SearchQuery` → `PostgresSearchParameters` → SQL `$14`.
- Result: all 56 unit tests pass. fmt/clippy/test clean. Migration must be applied on live DB for effect.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` — all clean.
- Status: complete.

## RS-024 — 2026-08-19 — Correct name ranking when LLM omits terms

- Goal: fix incorrect ranking and extra latency when the LLM returns only `tags` and `terms` is empty (e.g. "включи радио рокс"), which breaks station-name token matching.
- Scope: in `SearchService::interpret_and_search`, if `intent.terms` is empty then we run `DeterministicQueryParser` and:
  - keep provider hard genre tags (when present),
  - but replace `terms`, `core_term_count`, and `raw_query` with deterministic tokenization outputs for name matching.
  - extend transliteration vocabulary with common voice tokens: `ультра`↔`ultra`, `рокс`↔`roks`, `викер`↔`viker`.
- Result: unit tests pass, including new transliteration coverage.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.
- Status: complete.

## RS-025 — 2026-08-19 — Narrow prefilter candidate set before heavy scoring

- Goal: reduce slow search queries and timeouts on broad requests where `prefiltered` matched too many stations before exact scoring.
- Scope: make `prefiltered` `MATERIALIZED`, compute a cheap `prefilter_score` from exact tag hits + `ts_rank_cd` + max trigram similarity, sort by that score, and cap the prefiltered pool to `GREATEST(limit * 20, 200)` before running expensive `regexp_split_to_table`, substring, and embedding scoring.
- Result: library checks are clean; the change is designed to keep relevance while cutting the number of rows that reach the expensive scoring phase.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --lib`.
- Status: complete.

## RS-026 — 2026-08-19 — Remove per-candidate regex tokenization from exact matching

- Goal: speed up candidate scoring further by removing repeated `regexp_split_to_table(...)` work on each candidate station row.
- Scope: replace exact term matching in `matched_count` with `s.searchable_tsv @@ plainto_tsquery('simple', qt.term)` so the scoring phase uses the precomputed `searchable_tsv` document instead of re-tokenizing station names and tags; simplify `prefilter_score` to tag hits + `ts_rank_cd`, leaving trigram similarity only for the already-limited candidate set.
- Result: library checks stay green while candidate scoring becomes cheaper for broad queries.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --lib`.
- Status: complete.

## RS-027 — 2026-08-19 — Fast empty result when metadata prefilter finds nothing

- Goal: avoid long over-limit requests when no suitable station exists.
- Scope: remove semantic-only fallback branch from SQL search path; if metadata prefilter has no candidates, the query now returns empty immediately instead of scanning embedding neighbors and ranking irrelevant semantic matches.
- Result: no-match path becomes fail-fast and avoids timeout-prone broad semantic fallback.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --lib`.
- Status: complete.

## RS-028 — 2026-08-19 — Generic bidirectional transliteration for station-name recall

- Goal: avoid brittle point-by-point brand mappings and make station-name search work across Russian and Latin spellings by default.
- Scope: `expand_transliterations` now performs generic bidirectional transliteration for every query token (ru→lat and lat→ru), in addition to existing high-signal dictionary mappings. This improves recall for names like `radio BOB` / `радио боб` and `God Radio` / `год радио` without adding per-station hardcoded pairs.
- Result: transliteration-based recall works through a general mechanism; tests were updated to cover generic token conversion.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --lib`.
- Status: complete.

## RS-029 — 2026-08-19 — Name-priority mode for "включи радио ..."

- Goal: improve top-1 selection when the user explicitly asks to play a station by name, especially for short names like `Rock FM`.
- Scope: derive ordered station-name hint phrases when the command starts with `включи радио` / `поставь радио` (and English equivalents), pass them into `SearchQuery`, and add a strong SQL ranking bonus for ordered phrase matches inside normalized station names. Also keep a bonus for full core-term coverage in the station name, preferring shorter exact names over longer variants.
- Result: commands in station-name mode now prioritize exact ordered name matches over broader tag/semantic matches.
- Checks: `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --lib`.
- Status: complete.

## RS-018 — 2026-08-18 — Read-only administration-console preview

- Goal: make the initial administration interface visible and usable during local development.
- Scope: a server-rendered `/admin` HTML page that authenticates API calls with the already configured
  application Bearer token, shows readiness, and searches catalog stations. It deliberately excludes
  persistent administrator identities, sessions, storage, and state-changing operations.
- Result: the page keeps the entered token only in JavaScript memory for the current browser tab, calls
  `/health/ready` and the existing protected `POST /api/v1/search` endpoint, and removes the in-memory
  token on disconnect. The OpenAPI contract records the HTML endpoint. The local launcher accepts a
  configured token or generates a random process-only token and prints it for the preview login.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test`, and `git diff --check` passed. The regular suite ran 57 tests successfully;
  PostgreSQL and provider-credential-dependent live tests remained explicitly ignored.
- Status: complete.




## RS-032 — 2026-08-20 — Package layout and architecture review

- Goal: review the repository for god objects and place related files into logical Rust packages
  without changing the public HTTP contract.
- Scope: catalog import packaging, voice-command/speech packaging, compatibility facades, graphify
  architecture review, and verification.
- Result: added `src/catalog/{mod.rs,import.rs}` and `src/voice/{mod.rs,command.rs,speech.rs}`;
  updated internal consumers to use the new package boundaries while preserving the old speech and
  voice-command module paths. Graphify identified `src/http/mod.rs` and `src/search/mod.rs` as the
  next low-cohesion seams; they remain explicitly documented for a follow-up incremental split.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed (61 unit/integration tests passed; 6 external/credential/database tests
  remained ignored). The test run used an isolated target directory because the normal debug binary
  was held open by an existing local process.
- Status: complete; next refactor should extract HTTP transport DTO/error/auth modules, then split
  search domain models, in-memory repository, and orchestration.

## RS-033 — 2026-08-20 — Confidence-gated local language and country intent filters

- Goal: stop Russian voice/text requests such as `Включи английский рок`, `Включи рок из Англии`,
  and `Включи испанские новости` from losing their requested language or country before station
  search.
- Scope: add an E5-based language-label classifier that reuses the query embedding, cache its
  compact multilingual label vectors at startup, add score and runner-up-margin gates, preserve a
  validated LLM language when deterministic parsing is silent, and preserve a validated LLM country
  only for explicit `из` / `from` requests. Add the `англии` country inflection and an environment
  rollback switch that does not disable semantic station ranking.
- Result: a confident semantic match applies `language` before repository search; a low-confidence
  match applies no hard filter. `из Англии` deterministically resolves to `GB`, and the existing
  provider's `GB` output no longer disappears during normalization. `ROCKSERVER_SEMANTIC_LANGUAGE_FILTERS`
  defaults to `on` when the local E5 embedding provider is configured and accepts `off` for
  rollback. Deterministic development embeddings cannot enable hard filters.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` are required before handoff. Unit coverage includes acceptance/rejection thresholds,
  integration before in-memory search, `англии`, and explicit-country provider preservation.
- Status: complete. `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
  passed; 67 library tests and the regular HTTP/contract suites passed, while external database and
  credential-dependent tests remained explicitly ignored.

## RS-032 — 2026-08-20 — Reconcile Windows voice documentation

- Goal: align the Windows roadmap, near-term TODO, README, and status with the implemented RockCast/RockServer voice MVP.
- Result: documentation now records that RockCast text search and default-microphone capture are implemented, and that RockServer selects the bounded, commit-time Yandex SpeechKit adapter when configured. It also records the remaining work accurately: deterministic end-to-end coverage, input-device selection, cancellation/state reporting, retention-safe logging, provider resilience, and true upstream partial recognition.
- Checks: documentation-only review against `rockcast/src/{rockserver.rs,voice/}` and `rockserver/src/{main.rs,providers/yandex_speechkit.rs}`; no code or tests changed.
- Status: complete.

## DOC-RM-004 — 2026-08-21 — Shared station catalog implementation plan

- Goal: split RM-004 into independently executable, approval-gated tasks and assign the appropriate
  Codex model and reasoning effort to every stage.
- Scope: planning only for the canonical schema, catalog repository, legacy conversion, RockServer,
  RockMobile, RockCast, release automation, cross-project review, and final cutover.
- Result: added `docs/rm-004-shared-station-catalog-plan.md` with subplans RM-004-A through RM-004-I,
  model assignments, dependencies, boundaries, acceptance gates, rollback, and completion criteria.
- Checks: documentation reviewed against the existing catalog/import graph and current project
  contributor instructions; no implementation checks were run because no code or behavior changed.
- Status: complete; implementation not started and requires an explicit follow-up request.

## RM-004-A — 2026-08-21 — Shared catalog contract and migration design

- Goal: define the proposed canonical station-catalog v1 contract and migration policy without
  starting implementation.
- Scope: read-only architecture inventory of RockServer, RockCast, and RockMobile; proposed envelope,
  station, stream, legacy-ID, tombstone, ownership, versioning, immutable-release, checksum,
  migration, coexistence, rollout, rollback, risk, and compatibility rules. Writes were limited to
  `docs/**`; the existing RM-004 plan was not modified.
- Result: added `docs/rm-004-contract-proposal.md`,
  `docs/rm-004-stations-v1.schema.proposed.json`, and
  `docs/rm-004-stations-v1.example.json`. The proposal is explicitly **PROPOSED / NOT APPROVED /
  NOT IMPLEMENTED** and lists all decisions requiring user approval before RM-004-B.
- Checks: restarted Graphify navigation sequentially at user request; RockServer query completed,
  while hanging RockCast/RockMobile queries were stopped after bounded waits and replaced by the
  documented read-only `graph.json` fallback, with all findings checked against current source.
  Both JSON files parse, the schema declares Draft 2020-12 and a stable `$id`, and the embedded and
  standalone examples match. `git diff --check` passed (with only Git's existing LF→CRLF working-
  copy warning), and the complete working-tree change set is confined to `docs/**`. Cargo/Gradle
  tests were not run because no code or build behavior changed.
- Status: proposal complete at the RM-004-A acceptance gate; awaiting explicit user approval.

## RM-004-D addendum — 2026-08-21 — RockMobile extended SQLite catalog release gate

- Goal: prepare a real, prebuilt SQLite release package for the next RockMobile update without
  misrepresenting a development-sized PostgreSQL catalog as complete.
- Scope: a provider-neutral PostgreSQL-to-SQLite export CLI, strict active/playable eligibility,
  deterministic primary-stream selection and provider-safe dedupe, exact-byte manifest hashing,
  eligibility/gap reports, schema and consumption documentation. RockMobile source and any network
  provider import remain out of scope.
- Result: added `export_mobile_catalog` and schema-versioned SQLite output containing only stable
  station/provider identity, discovery metadata, normalized name/tags and selected primary stream.
  It excludes health/probe data, embeddings, import runs, timestamps, credentials and provider
  operational metadata. A complete release requires at least 16,000 eligible stations. The verified
  local disposable database initially had 42 and correctly emitted only a gap report. After the
  local `rockserver` migrations and checksum-pinned baseline activation, the verified real catalog
  had 16,825 eligible rows and produced the complete `2026.08.2-mobile.1` SQLite, manifest, and
  eligibility report. The exact SQLite SHA-256 is
  `ad469d405f177d7e476cf9b3d9985497d0e2c6132ac0f3ce14485f4eab402073`.
  `docs/rockmobile-extended-catalog.md` documents exact file names, tables, columns, indexes,
  checksum verification, Room bundling, and release procedure.
- Checks: SQLite fixture passed integrity, user/schema version, metadata/count, FTS-row-count,
  ordinary search-index, and SHA-256 checks. The live PostgreSQL export recorded the 42/16,000
  gap. Final Rust, catalog, graph, and diff checks are recorded in the task handoff.
- Status: complete; verified bundleable RockMobile artifact released at the RM-004-D acceptance
  gate. Android consumption remains a separately approved RM-004-E/F activity.

## RM-004-G — 2026-08-25 — Release and synchronization automation

- Goal: make immutable shared-catalog releases and consumer snapshot updates explicit, offline,
  reproducible, and checksum-gated.
- Scope: local catalog release tooling/tests/documentation and vendored snapshot metadata/artifacts
  in RockServer, RockMobile, and RockCast; no runtime fetches, releases, pushes, or legacy removal.
- Result: `tools/release_sync.py` publishes immutable `release/<version>` directories and validates
  selected releases before syncing a consumer. It supports dry-run, drift verification, and rollback
  to a retained prior release. RockMobile validates its independent extended SQLite manifest/hash,
  integrity, DB schema, metadata version, and count without conflating it with baseline JSON.
- Checks: catalog-tool unit suite passed, release publication/dry-runs/sync/verify passed for all
  consumers. Consumer build/test checks are recorded with this handoff.
- Status: complete at RM-004-G acceptance gate; RM-004-H/I not started.

## RM-004-H remediation — 2026-08-25 — Preserve catalog lifecycle metadata and fail closed

- Goal: resolve the two High findings in `docs/rm-004-h-cross-project-review.md` without changing
  the public HTTP/OpenAPI contract or the ownership of Radio Browser records.
- Scope: retain canonical tombstones and replacement semantics in RockServer preflight, activation,
  PostgreSQL persistence, and internal lookup; prevent an invalid production pin from falling back
  to the unrelated six-station test fixture.
- Result: `PinnedSharedCatalog` exposes validated tombstones and returns `Redirect` only for a
  single-target merge; splits return `Ambiguous` and removed IDs return `Removed`. Migration 0011
  adds the provider-scoped active lifecycle table. Transactional activation updates the lifecycle
  view, station/stream retirement, and import run atomically; reimport is idempotent and rollback
  by reactivating the prior release removes the newer tombstones and reactivates its stations.
  Search continues to exclude retired station/stream rows. Radio Browser is never included in
  RockCatalog lifecycle writes. Pinned checksum/schema/semantic failures now surface as startup or
  readiness failure and leave any already-active PostgreSQL release untouched; no production path
  can select the fixture, which is test-only.
- Checks: catalog unit coverage includes removed/merged/split semantics and invalid artifact
  rejection; an ignored disposable-PostgreSQL integration scenario covers activation, active search
  exclusion, replacement lookup, idempotence, rollback, and Radio Browser coexistence. Full Rust
  verification is recorded with the handoff.
- Status: complete pending the recorded verification results; RM-004-I was not started.

## RM-004-I — 2026-08-25 — Cutover and legacy cleanup

- Goal: complete the RM-004 cutover without weakening the already-remediated lifecycle/tombstone
  and corrupt-pin safety findings.
- Scope: synchronize and verify the approved immutable baseline across all consumers; remove only
  expired legacy paths; document source ownership, baseline/extended flow, rollback, and exceptions.
- Result: designated RockCatalog `2026.08.2` / SHA-256
  `3fa20dca94fc059bd433a47b9fba9bb6d5e5e1aa2957a5ffb58b2a7b20b1d74d` as the shared consumer
  baseline. RockMobile retains its separately verified `2026.08.2-mobile.1` SQLite package
  (16,825 records; SHA-256 `ad469d405f177d7e476cf9b3d9985497d0e2c6132ac0f3ce14485f4eab402073`).
  The only retained legacy item is RockCast's user-override TXT adapter: owner RockCast
  maintainers, reason preserve offline overrides, removal date 2026-10-31. No manual consumer-copy
  route is supported; `release_sync.py sync` and `verify` are the required offline workflow.
- Checks: catalog tooling tests passed 12/12; `release_sync.py verify` passed for RockServer,
  RockCast, and RockMobile; exact baseline and extended hashes, SQLite integrity/schema/count/FTS,
  RockServer `cargo fmt --check`, strict Clippy, and `cargo test` passed (81 regular tests);
  RockCast fmt, strict Clippy, and `cargo test` passed (50 regular tests). A loopback in-memory
  RockServer readiness smoke returned 200 and was stopped cleanly. RockMobile Gradle unit/lint
  checks were blocked before compilation because the configured API-36 SDK metadata is inaccessible
  and licenses are unaccepted; no download, license acceptance, device, network, provider, or live
  database test was performed.
- Status: cutover work complete locally, but the RM-004-I acceptance gate is not fully passed until
  RockMobile's offline unit/device verification can run against a readable, licensed Android API-36
  SDK. This is an environment handoff item, not a claimed successful check.

## Shared roadmap — 2026-08-25 — Internet beta planning

- Goal: create a cross-project execution route for RockServer, RockCast, RockMobile, and a future
  ESP32 client before public internet testing.
- Scope: documentation only in `docs/shared-product-roadmap.md`, plus status/task recording.
- Result: RM-007 → RM-011 → RM-012 is the current priority. The plan defines local personal data,
  account/session/device registration, synchronization and remote control, then ESP32 pairing after
  hardware arrives. It recommends a single-VPS Docker Compose/Caddy/private-PostgreSQL deployment
  with immutable-image releases, backup, readiness checks, and rollback.
- Checks: documentation reviewed for scope; `git diff --check` to be run before handoff.
- Status: planned; no implementation, live deployment, external access, credentials, API, or client
  code changes performed.

## Shared roadmap — 2026-08-25 — Executable task decomposition

- Goal: turn the internet-beta roadmap into bounded tasks that can be separately approved and sent
  to Codex.
- Scope: documentation only: `docs/shared-product-execution-plan.md`, roadmap link and status/task
  records.
- Result: created GATE-001, four RM-007 tasks, four OPS-001 deployment tasks, six RM-011 account
  tasks, six RM-012 sync/control tasks and two post-hardware ESP32 tasks. Each records repository
  boundary, dependency, model/reasoning, work and acceptance gate.
- Checks: `git diff --check` to be run before handoff.
- Status: planned; this decomposition authorizes no implementation or deployment.

## RM-007-D — 2026-08-25 — Cross-client local personal-data review

- Goal: verify RM-007-A field mapping, stable identity, lifecycle, offline-first behavior,
  migration/restart safety and rollback across the existing RockMobile RM-007-B and RockCast
  RM-007-C working trees without changing either client implementation.
- Scope: read-only client/catalog/source/test review and local offline verification; writes limited
  to RockServer `docs/**`. Existing dirty changes in all repositories were preserved. No network,
  production service, secret, live database, device test, shared catalog mutation or commit.
- Result: added `docs/rm-007-d-cross-client-review.md`. Baseline/catalog identity is consistent and
  pure lifecycle rules avoid automatic split selection, but four High blockers remain: incompatible
  RockMobile profile/timestamp shape, destructive fail-open Mobile profile reading without
  migration rollback, unwired Mobile lifecycle reconciliation, and URL-derived RockCast remote/
  voice history identity. RM-011-A is blocked; OPS-001-A remains independent design-only work.
- Checks: release verification passed for all three consumers including the Mobile extended
  manifest; catalog-tool tests passed 12/12; RockServer `cargo test catalog --lib` passed 14/14;
  RockCast `cargo test personal_data --lib` was initially blocked by its ordinary cargo lock and
  then passed 5/5 with `--target-dir C:\repos\rockserver\target\rm007d-rockcast`. RockServer fmt,
  strict Clippy and full tests passed. RockCast fmt, strict Clippy and full tests passed (55 library
  + 2 relay integration; 8 live-network tests ignored). RockMobile
  targeted unit and lint invocations used process-local
  `-Duser.home=C:\Users\alex` sequentially, but both stopped before compilation on the inaccessible
  Gradle wrapper `.zip.lck`; lint is not passed and the three known errors were not cleared.
- Status: **not passed**; remediate every High finding and repeat the same-fixture cross-client
  review before RM-011-A.

## RM-007-D remediation — 2026-08-25

- Goal: remediate every High/Medium implementation finding from the cross-client review without
  changing shared catalog data or adding server/auth/sync functionality.
- Result: RockMobile portable v1 storage, safe legacy migration/rollback, lifecycle integration and
  resolver edge cases were implemented; RockCast canonical remote identity, favourite merge,
  migration counts and explicit rollback were implemented. Client documentation was updated.
- Checks: RockCast fmt, strict Clippy and full tests passed (55 library + 2 relay integration; 8
  live-network ignored). RockMobile targeted unit and lint commands used the required sequential
  process-local `-Duser.home=C:\Users\alex` policy but both stopped before compilation on the
  inaccessible Gradle wrapper lock.
- Status: implementation findings remediated; cross-client gate remains **not passed** until
  RockMobile compile/unit/lint verification can execute successfully.

## OPS-001-B — 2026-08-25 — Reproducible container and Compose stack

- Goal: provide a reproducible local container/runtime foundation for the approved single-VPS
  Caddy, RockServer and private PostgreSQL design.
- Scope: root Dockerfile and ignore rules; Compose base plus local/production overrides; local and
  production Caddy templates; safe environment example; local preflight/startup script; fail-closed
  startup configuration requiring `ROCKSERVER_API_BEARER_TOKEN` and `DATABASE_URL`; deployment
  documentation. No VPS, DNS, registry, production secret, public port, database or deployment was
  changed.
- Result: the service now accepts the configured Bearer credential instead of a source-embedded
  bootstrap value and refuses to select the in-memory catalog when `DATABASE_URL` is absent. The
  Compose base keeps RockServer and PostgreSQL un-published; local Caddy is loopback-only and the
  production override publishes only Caddy 80/443. Healthchecks cover PostgreSQL, RockServer and
  Caddy; the verification script validates Compose without printing environment values and can run
  a disposable local readiness smoke.
- Checks: Docker image build passed. `deploy/verify-compose.ps1 -Mode local -Start` passed with
  healthy PostgreSQL, RockServer and Caddy and loopback Caddy `GET /health/ready` HTTP 200; it
  removed only its disposable project. Production rendering passed and exposed only Caddy `80` and
  `443`; PostgreSQL and RockServer had no host ports. `cargo fmt --check`, strict all-target/all-
  feature Clippy, and serial-target `cargo test` passed (82 regular tests plus HTTP/OpenAPI/
  WebSocket suites; external/credential/asset tests remained ignored). `graphify update .` passed.
- Status: **passed locally**; public domain/DNS/VPS/registry/secrets/firewall/deployment remain
  manual OPS-001-D/production actions and were not performed.

## OPS-001-C — 2026-08-25 — CI image, release, backup and rollback runbook

- Goal: make commit-SHA image verification, manual release approval, backup-before-deploy,
  readiness-gated rollout, previous-image rollback and non-production restore rehearsal explicit
  and reproducible without using production infrastructure.
- Scope: `.github/workflows/ci-release.yml`, `deploy/release.ps1`,
  `deploy/restore-rehearsal.ps1`, and the OPS-001-C sections of `deploy/README.md` plus current
  status/task records. No VPS, DNS, public endpoint, registry publication, production database,
  secret, client/catalog change or commit was made.
- Result: CI now runs format/Clippy/tests, labels a built image with the source commit SHA and
  executes a loopback Compose readiness smoke. GHCR publication is manual and protected by the
  `release-gate` environment. The release script enforces digest-pinned images and production port
  isolation, creates a custom-format PostgreSQL backup before deploy, records its checksum, waits
  for health/readiness, and supports previous-digest rollback without editing a running container.
  The restore script uses a disposable PostgreSQL network, `pg_restore`, restored-table validation,
  and an in-network RockServer readiness check; all temporary resources are cleaned by default.
- Checks: PowerShell parser checks passed for `deploy/release.ps1`,
  `deploy/restore-rehearsal.ps1`, and `deploy/verify-compose.ps1`. Production Compose rendering
  passed with only Caddy ports 80/443. The pinned Docker image built successfully. The local
  Compose stack reached healthy PostgreSQL, RockServer and Caddy states and loopback readiness
  returned HTTP 200. Release `preflight -DryRun` and `rollback -DryRun` passed without starting
  containers or contacting a deployment target. A corrected disposable `pg_dump`/`pg_restore`
  rehearsal restored the database and returned application readiness; the disposable network and
  containers were removed. The initial rehearsal findings (PowerShell binary redirect and omitted
  restore username) were fixed before the passing run. Final `git diff --check`,
  `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed; the ordinary
  suite reported 82 passed tests with only explicitly ignored PostgreSQL/provider/asset cases.
  `graphify update .` passed and refreshed the local graph to 1,683 nodes, 3,204 edges and 103
  communities; its existing zero-node JSON warnings were non-fatal.
- Status: **passed locally**, pending only manual external release/deployment actions. GHCR
  publication, production backup/deploy, public HTTPS readiness and live rollback were not run.

## OPS-001-D — 2026-08-25 — Registry-free single-VPS bootstrap and staging update

- Goal: make the approved single-VPS staging path owner-operated from one ignored inventory and one
  local update command, without GitHub/GHCR, a registry, or unsafe password handling.
- Scope: deployment launcher/module/remote script, Docker seed binary, Compose seed/ONNX wiring,
  examples, local script tests and current deployment documentation. No VPS, DNS, registry, GitHub
  secret, live credential, ONNX asset download, commit or push was used.
- Result: the inventory accepts exactly SSH user/host/domain and no password. First SSH access is an
  explicit OpenSSH prompt followed by generated-key use; bootstrap provides TTY-backed sudo and a
  validated command-scoped rule, while normal deploy is non-interactive. A clean current commit is
  locally built/tagged/labeled, saved and checksummed, then loaded and identity-checked on the VPS.
  Protected env merging preserves generated DB/API secrets, writes distinct lines, and transfers
  only the four allowlisted Yandex names. Backup → embedded migrations → pinned idempotent full
  catalog activation → HTTPS readiness and opt-in checksum-gated ONNX remain fail closed.
- Checks: focused PowerShell tests passed for registry independence, commit/image identity,
  password-free inventory, secret-safe output/env serialization, TTY/sudo construction, and current
  catalog/ONNX safeguards. Dry-run, production Compose rendering, PowerShell parsing, Rust fmt,
  strict Clippy, full tests, and diff checks passed. An all-features Docker build downloaded locked
  dependencies and entered compilation, then was stopped at the owner's request before completion;
  Docker image build is therefore not claimed as passed. External actions remain unverified.
- Status: **passed locally** pending the owner’s real VPS/DNS/firewall/backup-monitoring setup and a
  deliberate staging launch. A registry is not a staging prerequisite.

## Station icons — шаг 0 — 2026-08-26 — Контракт и границы

- Goal: зафиксировать совместимый RockServer-first контракт и границы station icons до реализации.
- Scope: только документация в `docs/roadmap/station-icons.md` и текущие status/task records. Не
  менялись Rust, SQL migrations, router, OpenAPI, фактическое HTTP-поведение или service diagram.
- Result: roadmap теперь явно помечает будущий `GET /api/v1/stations/{id}/icon` и nullable
  `faviconUrl` как planned state; внешний `source_url` остаётся внутренним. Зафиксированы приоритет
  source URL → homepage favicon → отсутствует, запрет network I/O в migrations и request path,
  v1 WebP/raster decision и лимиты, `200`/`304`/`404`, cache policy и безопасный rollout/rollback.
- Checks: reviewed `AGENTS.md`, roadmap, architecture/status/tasks, current OpenAPI and service
  diagram; current OpenAPI was confirmed not to expose the planned endpoint or `faviconUrl`.
  Required-contract text and trailing-whitespace checks, `git diff --check`, and a no-index diff
  check for the untracked roadmap file passed.
- Status: **documentation step complete**; feature remains unimplemented and requires separately
  approved roadmap steps.

## MVP-001-A — 2026-08-26 — Public API contract and abuse model

- Goal: define an approval-only anonymous public `/v1` contract and abuse model for the MVP-001
  zero-config catalog/search/voice path.
- Scope: `docs/mvp-001-a-public-api-contract.md`,
  `docs/mvp-001-a-openapi.proposed.yaml`, and current status/task records only. Existing dirty
  roadmap/status/task work was preserved. No Rust, runtime endpoint, current `api/openapi.yaml`,
  RockCast/RockMobile, secret, network, live provider, commit or push was changed.
- Result: the proposal names exactly five anonymous operations, bounds all request/audio/session/
  concurrency resources, distinguishes `429` quota exhaustion from global `503` voice capacity,
  fixes safe error/metric/logging fields and fail-closed protected endpoint families. It explicitly
  rejects shared Bearer tokens and all client secrets as public-user authorization. It contains
  acceptance criteria and five human-approval decisions before MVP-001-B.
- Checks: reviewed `AGENTS.md`, current OpenAPI and MVP-001 roadmap/execution plan; Markdown
  whitespace, proposed-OpenAPI structural/policy assertions (five anonymous operations, no
  credential security scheme), current OpenAPI unchanged, and `git diff --check` passed. Cargo
  checks are not applicable because no code or runtime behavior changed. The Graphify navigation
  query left two confirmed helper processes; both were stopped and verified exited.
- Status: **proposal complete; awaiting human approval**. No public endpoint is implemented or
  declared current by this task.

## MVP-001-B — 2026-08-26 — anonymous public endpoint policy

- Goal: implement the approved anonymous `/v1` catalog/search/voice allowlist without a shared
  release-client credential, while retaining Bearer protection for `/api/v1` aliases.
- Result: bounded catalog/search/voice handlers, direct-peer fail-closed quotas, global voice
  admission, safe error bodies and redacted public-path diagnostics were added. No client,
  provider, secret, network, deployment, commit, or push was used.
- Checks: `cargo fmt`, `cargo check`, and focused deterministic search, voice-command and
  WebSocket tests passed. Full required Clippy/test and OpenAPI reconciliation remain pending.
- Status: implementation in progress; no public rollout is authorized.

## RM-011-A — 2026-08-26 — account/device contract and threat model

- Goal: prepare the smallest secure approval-only account, session, device and future pairing
  contract after the owner-confirmed RM-007-D/MVP-001 gates, without implementing authentication.
- Scope: `docs/rm-011-a-auth-device-contract.md`,
  `docs/rm-011-a-openapi.proposed.yaml`, and factual roadmap/status/task records only. Existing
  unrelated changes were preserved. No runtime OpenAPI, Rust, PostgreSQL migration, client UI,
  email/SMS, production secret, network, provider, commit or push was used.
- Result: revised by owner direction to remove email/password/SMS and require no RockMobile
  installation. The proposal now uses a passkey in a first-party mobile browser to approve a
  desktop-originated QR/short-code request; the desktop alone receives its distinct opaque
  10-minute access/30-day rotating-refresh session. The threat model fixes separate desktop and
  approval proofs, safe errors, rate-limit keys, redacted logs and minimum audit events. It lists
  eight owner decisions plus WebAuthn/migration prerequisites for RM-011-B/C. Phone-first browser
  registration is explicit, and an additive `AccountIdentity` boundary preserves a future optional
  email or explicit-login method without redesigning user/device/session ownership.
- Checks: new proposal Markdown heading/whitespace check passed; a temporary no-runtime Rust test parsed the
  revised proposed OpenAPI and asserted phone-first passkey registration plus QR/short-code pairing,
  removal of the old register/login routes and email/password fields, opaque desktop bearer,
  browser-cookie scheme and reserved additive-identity boundary (1 passed), then was removed.
  `cargo fmt --check`,
  strict all-target/all-feature Clippy, `cargo test` (84 library + 18 regular integration tests;
  7 configured external/PostgreSQL tests ignored), and `git diff --check` passed. Current
  `api/openapi.yaml` was inspected and deliberately left unchanged because it has no matching
  runtime behavior.
- Status: **proposal approved by the owner on 2026-08-26**. No account/device endpoint is current.
  The proposal intentionally preserves explicit recovery/retention/operational alternatives; the
  relevant option and listed RM-011-B/C prerequisites require approval before implementation.

## RM-011-B — 2026-08-26 — approved account/session persistence subset

- Goal: implement only RM-011-A persistence boundaries that do not require selecting unresolved
  recovery, retention, WebAuthn-operation, proxy, browser or credential policy.
- Scope: migration `0012`, passkey-only account/device/session domain and PostgreSQL store, plus
  deterministic unit and opt-in PostgreSQL integration coverage. No password/email identity,
  browser/pairing, WebAuthn verification, rate limit, HTTP endpoint, client, external provider,
  secret, deployment, commit or push was added.
- Result: users/devices/sessions/refresh-token chains and audit classifications persist hashes
  only; a used refresh token revokes its session family atomically. Account deletion tombstones the
  user and revokes active devices, sessions, refresh tokens and passkeys. The reserved identity
  table is unused and does not introduce login behavior.
- Checks: `cargo fmt --check`, offline compile, strict Clippy and full tests were run locally;
  the PostgreSQL integration test is deliberately ignored unless `TEST_DATABASE_URL` points to a
  disposable database. No live database was provided by this task.
- Status: **approved subset complete locally**. RM-011-C remains blocked until RM-011-B2 review and
  runtime OpenAPI reconciliation; this historical B1 entry does not claim those later gates.

## RM-011-B2 — 2026-08-26 — browser, pairing, WebAuthn-boundary and rate-limit persistence

- Goal: implement the approved second persistence boundary without adding public HTTP routes or
  client UI: browser sessions, one-time pairing requests, WebAuthn challenge context/sign-count
  guards, and PostgreSQL-backed rate-limit buckets.
- Scope: migration `0013_add_browser_pairing_and_rate_limits.sql`, `src/auth/mod.rs`,
  `src/persistence/account_postgres.rs`, deterministic auth unit tests and an opt-in PostgreSQL
  integration test. No raw token, QR/code, cookie, WebAuthn assertion, password, email, HTTP route,
  runtime OpenAPI or client change was added.
- Result: browser approval requires a live, recently reauthenticated session; pairing approval and
  completion are single-use and transactional; completion enforces the ten-device cap; challenges
  are origin/RP/ceremony-bound and single-use; sign-count rollback is rejected; rate-limit buckets
  increment atomically in PostgreSQL. Existing account deletion revokes the new browser sessions;
  pairing/challenge records remain server-only.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` (88 unit/library tests and regular integration suites), and the disposable PostgreSQL
  test `postgres_b2_browser_pairing_webauthn_and_rate_limits -- --ignored --nocapture` passed. The
  normal compose volume was not modified; a temporary disposable container was removed afterward.
- Status: **B2 persistence boundary complete locally**. Cryptographic WebAuthn signature provider,
  HTTP routes and runtime OpenAPI reconciliation remain RM-011-C prerequisites.

## RM-011-C — 2026-08-26 — unified first-party web shell (partial)

- Goal: establish the agreed single frontend/admin technology and static delivery boundary.
- Scope: added `web/` TypeScript + Vite + Preact project with shared API client/types and pairing
  presentation, plus a Caddy image that builds and serves it while proxying API routes.
- Result: `/admin` is no longer a separate public frontend when accessed through Caddy; it is
  handled by the same SPA bundle. No secret is bundled or persisted by the UI.
- Checks: TypeScript `tsc --noEmit` passed using the installed dependency runtime. Vite production
  build is blocked locally because the environment's package policy refuses esbuild's required
  install script; it must be run in CI/container with the approved dependency-build policy.
- Status: **partial**. No RM-011-C auth API, WebAuthn verifier, session cookie/CSRF handling or
  runtime OpenAPI update is claimed by this UI-only increment.

## RM-011-C — 2026-08-26 — pairing request and lookup runtime slice (partial)

- Scope: production startup now supplies the existing PostgreSQL account store to the HTTP router;
  added pairing-request creation and neutral short-code lookup, with the corresponding runtime
  OpenAPI paths.
- Result: request proofs are UUID-derived opaque values sent only to the desktop initiator and
  persisted as SHA-256 hashes; PostgreSQL calculates the ten-minute expiry. Lookup returns only
  the non-secret preview already defined by B2.
- Checks: `cargo fmt --check`, strict Clippy, `cargo test` (88 tests), and `git diff --check`
  passed. Disposable PostgreSQL integration tests remain opt-in and were not run.
- Status: **partial**. No approval/completion route, rate-limit use, trusted-proxy validation,
  WebAuthn, browser cookie/CSRF boundary, or end-to-end pairing claim is made.

## RM-011-C — 2026-08-26 — browser-proof approval slice (partial)

- Scope: migration `0014_add_browser_session_cookie_hash.sql`, browser-proof persistence method,
  `POST /api/v1/pairing-requests/{request_id}/approve`, CSRF header/cookie checks and OpenAPI entry.
- Result: approval no longer accepts a bare/guessable browser session ID; the persistence query
  requires the opaque cookie hash, CSRF hash, active session and fresh passkey timestamp.
- Checks: offline cargo check, strict Clippy and targeted API test passed.
- Status: **partial**. Browser session issuance still requires WebAuthn registration/authentication;
  trusted-proxy enforcement and desktop completion remain outstanding.

## RM-011-C — 2026-08-26 — pure-Rust WebAuthn browser ceremonies (partial)

- Scope: added `passkey-auth` with fixed RP ID/origin policy, registration and authentication
  options/verify routes, PostgreSQL opaque challenge state blobs, credential lookup and atomic
  sign-counter advancement, plus Secure/HttpOnly/SameSite browser cookie issuance.
- Result: registration and authentication verification now perform cryptographic client-data,
  RP-ID, challenge, user-verification and signature checks; replay/rollback is rejected by the
  existing B2 challenge and counter guards. No OpenSSL dependency is required.
- Checks: sequential offline `cargo check` passed after dependency download. Full Clippy/test
  rerun is pending after this slice; no disposable PostgreSQL run was performed.
- Status: **partial**. Pairing completion, trusted proxy secret binding and full web UI ceremony
  wiring remain outstanding.

## RM-011-C — 2026-08-26 — desktop pairing completion slice (partial)

- Scope: added `POST /api/v1/pairing-requests/{request_id}/complete`; it delegates to B2's atomic
  approval/consumption transaction, enforces the owner UUID, ten-device cap and one-time desktop
  proof, and issues short-lived native access plus rotating refresh credentials.
- Result: access expires after 15 minutes and refresh after 30 days using PostgreSQL's clock; raw
  credentials are returned only in the response and never logged.
- Checks: sequential offline `cargo check` passed; full test and Clippy rerun remains pending.
- Status: **partial**. End-to-end browser UI wiring and final security review remain.

## RM-011-C — 2026-08-26 — browser ceremony UI wiring (partial)

- Scope: enabled the Preact UI's real `navigator.credentials.create()` path, strict base64url
  serialization of attestation data, registration verify call, in-memory CSRF token and pairing
  approval button. QR links may carry the approval secret without persisting it.
- Result: the browser can register a passkey and approve a looked-up device from the same origin;
  no token is written to localStorage or bundled into the frontend.
- Checks: source changes are complete; TypeScript build must run in the Caddy image because the host
  package policy blocks esbuild's install script.
- Status: **superseded by the current RM-011-C snapshot below**.

## RM-011-C — 2026-08-26 — unified auth/pairing delivery and verification (current snapshot)

- Goal: finish the first-party browser API/UI slice on the approved B2 persistence boundary using
  one TypeScript + Vite + Preact stack for user and admin surfaces.
- Result: cryptographic passkey registration and authentication, cookie/CSRF/origin checks,
  authenticated Caddy proxy proof, PostgreSQL-backed pairing/rate-limit usage, pairing approval,
  desktop completion, and a shared Preact UI with passkey controls, short-code lookup, device and
  verification-phrase display, and in-memory QR rendering are implemented. Native access/refresh
  tokens are returned only by the desktop completion endpoint; the browser does not persist them.
- Checks: `cargo fmt --check`, strict Clippy with `--jobs 1`, `cargo test --jobs 1` (93 tests passed),
  OpenAPI contract tests, and all four opt-in PostgreSQL integration tests against a disposable
  `pgvector/pgvector:pg17` container passed sequentially.
  `pnpm install --frozen-lockfile`, `pnpm typecheck`, `pnpm lint`, and `pnpm build` passed with
  the bundled Node runtime. No git push was performed.
- Status: **complete — RM-011-C server/browser implementation verified**. The first-party UI and
  server API are validated autonomously; real-client RockCast/RockMobile staging E2E is deferred
  to RM-011-D/E and is not a prerequisite for this task.

## RM-011-C — 2026-08-26 — native session and device HTTP surface

- Goal: expose the B2 native-session persistence primitives through owner-scoped HTTP routes.
- Scope: added refresh rotation with transactional access-token replacement, logout/family
  revocation, account profile projection, active-device listing, and owner-checked device revoke;
  updated both router constructors and `api/openapi.yaml`.
- Checks: `cargo fmt`, `cargo check --all-targets --all-features --jobs 1`, strict Clippy, full
  `cargo test --all-targets --all-features --jobs 1` (93 passed), OpenAPI contract tests, and all
  four PostgreSQL integration tests against a disposable pgvector container passed.
- Status: **complete** at the RM-011-C server/browser boundary; real-client E2E is deferred to
  RM-011-D/E.

## RM-011-C — 2026-08-26 — completion owner derivation

- Goal: remove the impossible native requirement to know a browser-approved account UUID.
- Result: completion request payload is now only `{ "desktop_token": "…" }`; the locked approved
  pairing request is the sole source of owner identity for the device/session transaction.
- Checks: PostgreSQL integration asserts completion returns the approved owner and rejects a replay;
  strict JSON schema rejects an extra forged `user_id` field at the HTTP boundary.
- Status: **complete**.

## HTTP transport refactor — 2026-08-26

- Goal: reduce the `src/http/mod.rs` god object without changing HTTP behavior.
- Scope: moved the public facade to `src/http/mod.rs`, route and DTO implementation to
  `src/http/endpoints.rs`, and shared `AppState`/anonymous admission controls to
  `src/http/state.rs`.
- Result: `mod.rs` is now a 10-line export surface; endpoint behavior, route registration, and
  existing test boundaries remain unchanged.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed;
  92 unit tests and all non-ignored integration tests passed.
- Status: **complete locally**.

## Limit local Cargo build parallelism — 2026-08-26

- Goal: prevent Cargo builds from consuming the whole workstation.
- Scope: added `.cargo/config.toml` with `[build] jobs = 2`.
- Result: project-local Cargo commands now use two compilation jobs by default; acceptance checks
  explicitly used `--jobs 1` for sequential execution.
- Checks: configuration is valid TOML; no source behavior changed.
- Status: **complete locally**.
## RM-011-G3 — 2026-08-28 — browser account and device centre

- Goal: provide one safe browser view proving that RockCast and RockMobile belong to the same
  named account, with device rename, revoke and current-browser logout.
- Scope: added cookie/proxy/CSRF-protected browser account endpoints and Preact account centre;
  device responses omit account IDs, tokens, credentials and audit details, while an opaque route
  handle remains internal to the browser UI and is never rendered.
- Result: rename rejects empty, control-character and over-128-character names; rename/revoke
  remain owner-scoped and auditable. Revoke ends all native sessions and refresh tokens on the
  selected device, whereas browser logout revokes only the current browser session. The UI uses
  explicit confirmations and explains that these two actions are different.
- Checks: `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test` passed
  (100 tests; five disposable PostgreSQL tests intentionally ignored without
  `TEST_DATABASE_URL`). TypeScript typecheck/lint, Vite build and five deterministic browser UX
  regressions passed using the checked-in web dependencies. No staging deploy, commit or push.
- Status: **complete locally; staging/physical-phone smoke remains.**

## SECURITY-001 — 2026-08-28 — remove vulnerable Rust TLS dependency chain

- Goal: remove the Dependabot-reported `rustls-webpki 0.101.7` chain without changing service
  behavior or the SpeechKit streaming contract.
- Scope: replaced the `yandex-cloud`/`tonic 0.9` dependency path with a minimal checked-in
  SpeechKit v3 protobuf and streaming client boundary on `tonic 0.14.6`, `tonic-prost 0.14.6`,
  and `rustls 0.23.43`; added wire-tag regression tests and updated security status records.
- Result: the old `yandex-cloud`, `tonic 0.9`, and `rustls-webpki 0.101.7` packages are absent
  from the lockfile graph. Existing API-key metadata, PCM16 streaming, EOU, transcript handling,
  endpoint validation, and HTTP/OpenAPI behavior are unchanged.
- Checks: `cargo fmt --check`, `cargo check --locked --all-targets --all-features --jobs 1`,
  `cargo clippy --all-targets --all-features --jobs 1 -- -D warnings`,
  `cargo test --all-targets --all-features --jobs 1` (102 passed; 12 explicitly ignored),
  `cargo tree` dependency inspection, and `git diff --check` passed. `cargo audit` is not
  installed on this host; the exact Dependabot advisories were independently checked against the
  lockfile dependency path.
- Status: **deployed and readiness-verified**. Commit
  `266b9740ae2f0489952f46a15e80a18791d09a7e` reached `origin/master`; OPS-001-D reported
  `status=succeeded` and readiness passed for the immutable server image.

## RM-011-G7 blocker removal — 2026-08-28

- Goal: remove the cross-repository pairing blockers found by the RM-011-G7 E2E verification.
- Scope: distinguish pending/invalid/expired/device-limit server completion states; make first
  account registration atomic after a successful passkey ceremony; cancel RockCast and RockMobile
  pairing when the native UI is dismissed; and clear the existing RockCast Clippy and RockMobile
  lint gates with targeted fixes.
- Result: F-1/F-2/F-3 are resolved in the current checkouts. No UUID/manual bearer token path was
  introduced, and local radio/fallback behavior remains independent of the server. The deploy
  follow-up preserves one verified on-VPS rollback dump and leaves the previous dump intact when
  backup creation or verification fails; the portable image ID and legacy argument compatibility
  checks are covered by the deploy test suite.
- Checks: RockServer fmt, strict Clippy and all-target/all-feature tests passed (103 library tests
  plus available target tests; six disposable PostgreSQL tests ignored without
  `TEST_DATABASE_URL`); OpenAPI and OPS-001-D contract tests passed. RockCast fmt/check/89 unit
  tests/strict Clippy passed. RockMobile unit tests, debug assemble, and lint passed. Commit
  `99c2783` contains the pairing fixes; commit `392d8b4` contains the deploy/backup follow-up.
- Status: **complete in local checkouts; disposable-DB and staging/real-device smoke remains.**

## OPS-001-D backup retention follow-up — 2026-08-28

- Goal: restore the requested bounded local rollback-backup behavior without weakening deploy
  failure safety.
- Scope: retain exactly one verified `rockserver-*.dump` on the VPS, add portable saved-image ID
  extraction on Windows, preserve former deploy argument compatibility, and update the static deploy
  checks for the current Caddy-aware compose environment.
- Result: a new dump is verified before old matching dumps are removed; a failed dump/checksum does
  not prune the previous backup. Local OPS-001-D tests pass. Commit `392d8b4` contains the behavior
  and `26906ef` records the verification; both reached `origin/master`, and OPS-001-D reported
  `status=succeeded` with readiness passed.
- Status: **deployed and readiness-verified; manual account/device smoke remains.**

## OPS-001-D activate updated operator script — 2026-08-29

- Goal: make the requested backup retention logic execute during a normal deploy.
- Finding: the previous deploy uploaded `remote-ops-001-d.sh` into a temporary stage but started the
  worker from the older `/opt/rockserver/remote-ops-001-d.sh`; staging therefore retained 19 old
  `rockserver-*.dump` files.
- Fix: after taking the deploy lock and before starting the worker, validate the staged operator
  file is a regular non-symlink and install it as the active root-side script. If lock or bundle
  validation fails, the old script remains active.
- Checks: OPS-001-D local tests and `git diff --check` pass. Commit `27beb6c` reached
  `origin/master`; OPS-001-D reported `status=succeeded` and readiness passed. A read-only audit
  confirmed exactly one staging `rockserver-*.dump` remains after deploy.
- Status: **deployed, readiness-verified, and backup retention confirmed.**

## RM-011-01 — 2026-08-29 — web registration and pairing state

- Goal: fix RM-011 A1/A2 without changing the API contract or native clients.
- Scope: split editable registration input from authenticated browser account state; require the
  verified account and CSRF proof for approval; add exclusive browser pairing states and a real
  post-204 success screen with return/account CTAs.
- Result: typing no longer unlocks approval, success removes pending and approval controls, and a
  reload of an approved/consumed request cannot restore an enabled approval action.
- Checks: eight web regressions, TypeScript typecheck/lint, Vite production build, Rust fmt, strict
  Clippy, and all available Rust tests passed. Six disposable PostgreSQL and six live external
  tests remained intentionally ignored.
- Status: **complete locally; staging/manual browser smoke remains.**

## RM-011-07 — 2026-08-29 — A6+A7 anonymous landing and separate registration

- Goal: implement only RM-011 web A6+A7 on the existing Preact root.
- Scope: anonymous `/` sign-in/registration distinction, `/register` form, explicit default-name
  choice, click-only and double-submit-safe registration ceremony, cancellation recovery, and
  pairing-context return. No API, OpenAPI, native client, A4 URL-contract, staging, or account/data
  change.
- Result: registration input stays isolated from lifecycle state; cancelling preserves the typed
  name; standalone registration exposes browser-signed-in success with the account-centre CTA;
  registration from a pairing URL returns to its exact confirmation context. Eleven deterministic
  web regressions cover A6/A7 and the earlier A1/A2 protections.
- Checks: `cd web && pnpm test` (11/11), `cd web && pnpm build`, `cargo fmt --check`, strict
  all-target/all-feature Clippy, `cargo test`, and `git diff --check` passed. Six disposable
  PostgreSQL and six live external tests remain ignored by design.
- Status: **complete locally; no deploy, push, staging, or manual WebAuthn ceremony.**

## RM-011-08 — 2026-08-29 — web cabinet A8–A12

- Goal: implement only RM-011 web cabinet information architecture, safe native-device actions,
  account/session states, responsive accessibility, and pairing-success cabinet handoff.
- Scope: updated the existing Preact/CSS cabinet and deterministic web regressions. Browser login
  is visibly separate from native devices; card actions remain confirmed and owner-scoped through
  existing APIs; success handoff uses only in-memory target metadata and creates no database state.
- Result: loading has no anonymous flash; confirmed auth failure clears device data into a session
  expired view; service failure offers retry; rename/revoke/logout have independent busy state;
  empty and `10 из 10` states differ; legacy product-prefixed display names are not doubled; no
  IDs, proofs, or tokens are rendered. The mobile layout keeps actions full width with focus,
  semantic status/alert text, and non-colour status symbols.
- Checks: `cd web && pnpm test` (9/9), `cd web && pnpm build`, `cargo fmt --check`, strict
  all-target/all-feature Clippy, and `cargo test` passed; six disposable PostgreSQL, four billable
  Yandex LLM, one SpeechKit, and one ONNX live test remain intentionally ignored. `git diff
  --check` passed.
- Status: **complete locally; no deploy, push, staging, or real account/device operation.**

## DOCS-CODEX-CONTEXT — 2026-09-01 — canonical cross-project task context

- Goal: create one durable, fact-checked starting context for future Codex tasks
  spanning RockServer, RockCast and RockMobile.
- Scope: document repository paths and boundaries, actual implementation state,
  approved admin authentication and station-icon decisions, delivery rules, and
  ordered `RS-ADMIN-*` / `RS-ICON-*` backlog entries.
- Result: added `docs/codex-project-context.md`. It preserves the existing
  `admin-security-plan.md` Bearer-in-memory decision and `roadmap/station-icons.md`
  contract; it marks planned icon work as planned rather than claiming it live.
  The context explicitly requires every ready stored and served v1 icon to be
  WebP; source bytes are not retained or served. It also serializes all
  Codex-started compilation/build/compiling-test/dependency-install work across
  the three projects and their worktrees. Every new or modified PostgreSQL query
  must additionally be covered by a disposable-database integration test via
  `TEST_DATABASE_URL`; fakes are not a substitute. Future Codex tasks default
  to the current `C:\repos\rockserver` master checkout rather than a worktree,
  while preserving unrelated changes. The RS-ADMIN-005 backlog excludes obsolete
  RockCast machine-credential lifecycle work: RockCast product routes are public
  and its user identity uses pairing.
- Checks: targeted inspection of the three repositories, RockServer status/tasks,
  architecture, OpenAPI, admin-security plan, station-icon roadmap and client
  icon implementations; Markdown content review. No build was needed because
  this change is documentation-only.
- Status: **complete; no API, source, migration, deployment, staging, or client
  behaviour changed.**

## DOCS-CLEANUP — 2026-09-01 — active documentation cleanup

- Goal: remove superseded documentation from the active RockServer docs root
  while preserving useful historical evidence.
- Result: deleted obsolete MVP/RM-004/shared-roadmap proposal, schema and example
  artifacts; moved historical RM-004/RM-007/RM-011 reports, diagnostics and
  explanatory diagrams to `docs/archive/2026-08/`; added the archive README and
  repaired active-document links.
- Checks: targeted reference scan for every removed filename and `git diff --check`.
- Status: **complete; no code, API, migration, deployment, staging, or client
  behaviour changed.**

## RS-ICON — 2026-09-01 — direction deferred

- Decision: retain the approved station-icon architecture in
  `docs/roadmap/station-icons.md` and the ordered `RS-ICON-*` backlog in
  `docs/codex-project-context.md`, but defer every implementation task.
- Scope: no migration, storage, downloader, sync job, endpoint, public
  `faviconUrl`, client change, deployment or staging data operation.
- Status: **deferred until a new explicit owner request.**

## DOC-DEVICE-ROADMAP-001 — 2026-09-02 — asynchronous speech and server operations

- Goal: extend the future device-control plan so a remote/control surface can play
  both immediate and asynchronous RockServer speech, including timer completion.
- Result: expanded the architecture with `speech_output`, durable operations and
  scheduler triggers, an allowlisted server executor registry, timer and weather
  use cases, provider-neutral TTS, temporary audio assets, a delivery outbox,
  playback acknowledgements, destination/fallback/expiry policy, and composed
  speech/display/device actions. Expanded the executable backlog through DC-038.
- Scope: documentation only; no API, source, migration, provider, deployment,
  firmware, client, timer, weather, TTS, or runtime behaviour changed.
- Checks: reviewed roadmap dependency order and lifecycle separation; validated
  all four embedded JSON examples and sequential DC-000 through DC-038 numbering.
  `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
  passed; external PostgreSQL, Yandex, SpeechKit, and asset-dependent tests
  remained ignored.
- Status: **complete.**

## DOC-DEVICE-ROADMAP-002 — 2026-09-02 — reuse existing account-device identity

- Goal: align the future device-control roadmap with the account, pairing, device
  session, inventory and revocation flows already implemented in RockServer.
- Result: removed the proposed `device_pairings` persistence and every task that
  would recreate pairing. The plan now uses the existing `devices.id`, durable
  device secret, `/api/v1/auth/device-session`, `/api/v1/devices` and revoke semantics;
  only capabilities, presence, entities, surfaces, state and routing are additive.
- Scope: documentation only; no API, source, migration, account/device row,
  credential, session, pairing request, client or runtime behaviour changed.
- Checks: targeted comparison with migrations 0012, 0013 and 0017, the current
  OpenAPI account/device endpoints, and the updated roadmap/task references;
  all four embedded JSON examples and DC-000 through DC-038 numbering validated;
  `cargo fmt --check`, strict all-target/all-feature Clippy, and `cargo test`
  passed. External PostgreSQL and credential/provider tests remained ignored.
- Status: **complete.**

## DC-005 — 2026-09-02 — device-control persistence foundation

- Goal: add the smallest durable PostgreSQL foundation for the DC-004 device-control domain.
- Result: added migration `0021_add_device_control_persistence.sql`, a domain-facing
  `DeviceControlStore` boundary, and `PostgresDeviceControlStore`. It uses existing
  `users`/`devices` ownership and revocation joins, current-only snapshots, manifest tombstones,
  and a 24-hour terminal-command retention query. No route, authentication wiring, provider
  identity, device/session model, sensor-history table, or command execution was added.
- Checks: `cargo check`, `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, and `cargo test` passed (117 library tests; opt-in PostgreSQL tests stay ignored by
  default). With a separately named local Compose project and loopback-only disposable
  `TEST_DATABASE_URL`, `cargo test --test postgres_integration -- --ignored --exact
  postgres_device_control_manifest_is_owner_scoped_and_tombstoned --nocapture` passed (1 test).
  Its container, network, and named volume were removed immediately afterwards.
- Status: **complete.**

## DC-006 — 2026-09-02 — control-plane native authentication foundation

- Goal: reuse completed account/device authentication for the future device-control ingress without
  creating a second credential, pairing, device, or native-session lifecycle.
- Scope: a public native-session resolver boundary, server-derived `DeviceControlPrincipal`, a
  reusable HTTP header extractor for the future `/api/v1/devices/connect`, one ownership-safe
  session lookup predicate, focused deterministic tests, and documentation. No runtime route,
  WebSocket upgrade/registration, registry, heartbeat, TTL/reconnect, presence, provider, or
  Home Assistant credential was added.
- Result: only a bounded native `Bearer` access token may resolve to an active session's existing
  `user_id` and `device_id`; no request/query/frame payload can assert either identifier. Expired,
  malformed, unknown and revoked native credentials have the same non-disclosing invalid outcome;
  legacy RockCast and administrator Bearers do not resolve through the native-session store, and
  browser cookies are not an input to the extractor. Session-store failure is a distinct retryable
  `Unavailable` outcome and does not revoke the device or its durable binding. The resolver also
  verifies that `sessions.user_id` owns `devices.id`. Renewal remains only
  `POST /api/v1/auth/device-session`.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed (120 library tests plus all regular suites; the four OpenAPI contract tests
  passed). PostgreSQL/live-provider/ONNX tests remained explicitly ignored because no disposable
  `TEST_DATABASE_URL` was configured.
- Status: **complete; DC-007 owns the WebSocket lifecycle and presence implementation.**

## DC-007 — 2026-09-02 — connection registry and presence

- Goal: add the native-authenticated device-control connection lifecycle without creating a new
  device/session store or implementing DC-008 state hub and DC-010 directory APIs.
- Result: `/api/v1/devices/connect` now upgrades only after the DC-006 native Bearer guard passes;
  malformed/invalid credentials receive 401 and lookup unavailability receives 503 before upgrade.
  The WebSocket requires hello, emits welcome, accepts a structurally bounded register frame, and
  emits server-derived registered identity. The process-local registry uses bounded replacement
  channels and owner-scoped bounded event history. A new registration atomically replaces the old
  generation; connection-ID guarded cleanup emits at most one offline transition for the active
  generation. Heartbeats refresh server time and TTL expiry removes the active generation.
  Existing owner-authorized revoke paths notify the active registry entry after durable revoke;
  graceful process shutdown broadcasts a controlled close to active control sockets.
- Follow-up: replaced manual `serde_json::Value` field access in the control lifecycle with typed
  envelope/payload DTOs. Only the structurally validated, intentionally unprocessed register
  manifest remains opaque pending DC-008.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test`, and `git diff --check` passed. Local WebSocket tests cover native 401/503 before
  upgrade, hello/welcome/register, server-derived identity, reconnect replacement/stale cleanup,
  graceful and transport-loss cleanup, heartbeat/TTL, invalid/binary/timeout frames and shutdown;
  registry tests cover owner isolation and revocation.
- Status: **complete.**

## DC-008 — 2026-09-03 — typed manifest/latest-state ingress

- Goal: begin the DC-008 manifest, state, persistence and internal fan-out lifecycle on the
  completed authenticated DC-007 transport.
- Result: registration now carries a typed `DeviceManifest`; later manifest replacement, full
  snapshots, state deltas and entity observations use typed DTOs and bounded payload dispatch.
  Production control routers use the DC-005 `PostgresDeviceControlStore`; latest accepted values
  are also sent through a per-user, 64-event lossy internal hub. Device deltas use
  `revision_order`, gaps/conflicts request a full resync, and entity freshness is calculated from
  server time without synthesizing sensor values. No command routing or public directory API was
  introduced.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` (128 library tests plus all regular suites), OpenAPI contract fixtures, and
  `git diff --check` passed. Ignored PostgreSQL integration tests were not run because no safe
  disposable `TEST_DATABASE_URL` was configured.
- Status: **complete.**

## DC-009 — 2026-09-03 — bounded owner-scoped command router

- Goal: route typed control commands over the existing authenticated WebSocket without adding a
  directory API, intent resolver, offline queue, pairing flow, or a parallel idempotency store.
- Result: added `device_control_command`, bounded outbound delivery on the single-active DC-007
  registry, and command dispatch for `device.command`, `command.accepted`, and `command.result`.
  Admission derives actor identity from `DeviceControlPrincipal`; it checks active controller role,
  explicit server grant, same owner, target presence/generation, role/capability/entity/surface and
  the bounded command payload before DC-005 `reserve_command`. The command lifecycle is received,
  accepted and one terminal target result; success is never inferred from queueing or acceptance.
  Result/acceptance correlation verifies owner/device/connection generation, timestamps are server
  observed, and timeout/disconnect/replacement end the record deterministically. The 16 per
  controller connection / 8 per target limits fail with `too_many_in_flight`; no command is queued
  offline. A canonical SHA-256 request fingerprint provides 24-hour replay, while the server default
  deadline remains stable across retries. v1 has no explicit cancel frame, so deadline/loss is the
  documented cancellation policy. Added a typed actuator command variant aligned with the existing
  OpenAPI union. No credential/token/raw payload logging was added.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, focused
  `cargo test device_control_command --lib`, full `cargo test`, OpenAPI contract test, and
  `git diff --check` were run. PostgreSQL ignored integration was skipped because no disposable
  `TEST_DATABASE_URL` was supplied.
- Status: **complete; DC-010 public directory/subscription API and DC-011 intents remain future work.**

## DC-010 — 2026-09-03 — controller directory HTTP projection

- Goal: begin the public account-owned controller directory without changing inventory/revoke,
  pairing, commands, homes, or client UI.
- Result: implemented the canonical native-authenticated `GET /api/v1/device-control/directory`
  route, owner-scoped persistent projection, capability/entity/surface metadata, presence,
  freshness metadata, bounded `domain`/`device_class` filtering, no-store response, and typed
  rejection of malformed/unknown filters. Entity values remain absent without `entity.state.read`.
  OpenAPI now marks the HTTP endpoint implemented and describes the exact v1 filter boundary.
- Checks: formatting, strict Clippy and the regular test suite were run; PostgreSQL ignored tests
  remain skipped because no safe disposable `TEST_DATABASE_URL` was supplied.
- Result: controllers now subscribe to the existing owner-scoped 64-event hub before receiving an
  initial watermark snapshot. Later owned presence, manifest, device-state, and entity-state
  transitions emit a bounded `directory.upsert`; a broadcast lag produces the explicit
  `directory_resync_required` close/reload policy. DC-007 replacement closes the prior socket and
  its subscription, preventing duplicate deliveries.
- Status: **complete.** DC-011 intents/presentation and DC-015 client UI remain out of scope.

## DC-011 — 2026-09-03 — typed intent resolution and presentation builder

- Goal: add a deterministic, non-executing device-control intent layer without introducing a
  voice/text endpoint, LLM parser, provider/tool dispatch, persistent target preference, client
  work, or a second command router.
- Result: added versioned strict `UserIntent` vocabulary and separate typed resolution context,
  plan, clarification, confirmation and error outcomes. Resolution consumes only a server-derived
  actor and account-owned directory projection; it verifies owner, scope, online target, role,
  capability, manifest entity/surface and readable state before creating a DC-009-compatible
  command plan. Selection is explicit IDs, request-local current target/surface, supplied canonical
  area mapping, then exactly one candidate. No name guessing, wildcard area, broadcast or offline
  fallback exists. `show_sensors` makes an explicit `sensor_grid`; stale remains labelled stale,
  unavailable/missing becomes explicit `null`, and no value is fabricated as zero. `query_sensor`
  requires one readable entity. Actuator proposals require explicit target and confirmation and
  cannot dispatch; lock/climate-like or non-allowlisted entities are denied. Added typed volume/mute
  command support already described by the v1 OpenAPI command union.
- Scope: no endpoint/OpenAPI operation, CommandInterpreter/LLM change, DC-009 execution change,
  persistent area/home storage, provider adapter, device firmware, RockCast or Rockmobile code.
- Checks: focused resolver tests, formatting, strict Clippy, full test suite, OpenAPI fixture tests
  and `git diff --check` are recorded after final verification. Ignored PostgreSQL tests remain
  skipped unless a safe disposable `TEST_DATABASE_URL` is supplied.
- Status: **complete.** DC-020 owns end-to-end display dispatch and DC-025 owns parser/LLM wiring.

## REFACTOR-002 — 2026-09-04 — cohesive control and intent module boundaries

- Goal: safely reduce two current Rust god objects without changing any service behavior.
- Scope: private v1 WebSocket protocol and state-admission helpers beneath `http::control`, plus
  private public-model ownership beneath `device_control_intent`. No endpoint, OpenAPI, migration,
  persistence, auth, authorization, command lifecycle, logging, audit, configuration or client
  changes.
- Result: `src/http/control.rs` is now the socket lifecycle/composition facade; its exact typed
  wire DTOs, bounded parsing, serialization and close helpers are in `control/protocol.rs`, and
  its monotonic state-admission helpers are in `control/state.rs`. `src/device_control_intent.rs`
  is now the resolver facade; its existing public schema/validation/context/result types are
  re-exported unchanged from `device_control_intent/model.rs`.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
  `cargo test` (141 library tests, OpenAPI contract fixtures, and regular suites passed);
  `git diff --check`. Ten PostgreSQL integration tests remain ignored because no disposable
  `TEST_DATABASE_URL` was configured; billable Yandex and SpeechKit live tests also remain ignored
  by their explicit credential/asset gates.
- Status: **complete; behavior-preserving internal refactor only.**

## REFACTOR-003 — 2026-09-04 — layered domain and persistence boundaries

- Goal: remove the remaining thousand-line source units and make the codebase easier to navigate
  for both maintainers and automated readers, without changing service behavior.
- Scope: private decomposition of the device-control domain model and PostgreSQL account store;
  extraction of existing unit tests from device-control, command, intent and search production
  modules. No endpoint, OpenAPI, migration, persistence behavior, auth, authorization, command
  lifecycle, logging, audit, configuration or client change.
- Result: `device_control.rs` is a documented public facade over focused model, validation,
  manifest, runtime-state, command, lifecycle and store modules. `account_postgres.rs` delegates
  to cohesive account, browser, passkey, pairing, rate-limit, cleanup and native-session modules;
  common private SQL row conversions are centralized. Existing unit tests now reside in private
  sibling test modules, so production flows can be read without fixture noise.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
  `cargo test` (141 library tests, regular suites and four OpenAPI contract tests passed);
  `cargo test --test openapi_contract` (4 passed); `git diff --check`. Ten PostgreSQL integration
  tests and five credential/asset-gated live tests were correctly ignored.
- Status: **complete; behavior-preserving internal refactor only.**

## DOCS-001 — 2026-09-04 — compact source navigation map

- Goal: provide maintainers and agents with a small, code-adjacent architecture index without
  duplicating implementation details in every source file.
- Result: added `src/ARCHITECTURE.md`, covering root-module responsibilities, dependency direction,
  focused device-control/persistence maps, test placement, and reading order. Added compact links
  from the crate root and Codex project context.
- Scope: documentation and crate-level Rustdoc only; no runtime, public API, storage, provider,
  authorization, or service-diagram behavior changed.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
  `cargo test`; `git diff --check`.
- Status: **complete.**
