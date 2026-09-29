# Source map

Use this file to choose a starting module before reading implementation details. It is a navigation
aid, not a second API contract: `AGENTS.md`, `api/openapi.yaml`, migrations, and the code remain
authoritative.

## Dependency direction

`main.rs` composes configured implementations. `http` maps transport to domain contracts. Domain
modules define validated models, decisions, and provider/storage traits. `persistence` and
`providers` implement those contracts and contain SQL or external-service mechanics. Transport,
SQL, credentials, and provider DTOs must not leak into domain models. Public HTTP behavior belongs
in `api/openapi.yaml`; request handling must not crawl catalog providers or perform background
import work.

## Root modules

| Module | Responsibility | Read first when changing… |
| --- | --- | --- |
| `account_cleanup` | Preview-first, exact-target staging cleanup boundary. | Operator cleanup behavior. |
| `admin`, `admin_bootstrap` | Administrator credentials, sessions, and one-shot bootstrap. | Admin identity or access. |
| `auth` | Passkey account, browser session, device pairing, and native-session contracts. | User/device authentication. |
| `catalog` | Catalog records, shared release validation, and controlled imports. | Station catalog ownership or imports. |
| `config`, `telemetry` | Process configuration validation and safe structured logs. | Startup configuration or operational logging. |
| `device_control` | Transport-free protocol-v1 models, validation, lifecycle, and store contract. | Any control model or persisted control record. |
| `device_control_auth` | Server-derived native principal for control ingress. | Control authentication only. |
| `device_control_command` | Owner-scoped command admission, idempotency, delivery, and terminal results. | Executing a typed control command. |
| `device_control_intent` | Deterministic, non-executing intent-to-plan resolution. | Text/voice intent planning; never dispatch here. |
| `device_control_presence`, `device_control_state` | Bounded live connection registry, presence, state cache, and internal fan-out. | Connection lifetime or observed device state. |
| `http` | Axum DTOs, authentication at transport boundaries, routing, errors, and WebSocket lifecycle. | HTTP/WebSocket behavior or OpenAPI changes. |
| `mobile_export` | Deterministic SQLite export for RockMobile offline catalog use. | Mobile catalog export. |
| `personal_data` | Account-owned favourites and playback-history sync domain: validated records, last-writer-wins merge, bounds, retention, and the store contract. | Personal-data sync records, limits, or persistence. |
| `persistence` | PostgreSQL implementations, migrations, and startup backend selection. | SQL, durable state, or repository wiring. |
| `providers` | Bounded adapters for Radio Browser, Yandex, and embedding providers. | External-service request/response mechanics. |
| `search` | Normalized query, parsing/embedding traits, filters, ranking, and station repository contract. | Search meaning, ranking, or repository-neutral behavior. |
| `station_icons` | Persistent storage, normalization, and coordinator for station-icon artifacts. | Station icons, storage, or background icon import. |
| `voice` | Provider-neutral speech and voice-command models. | Speech recognition or recognized-command flow. |

The `speech` and `voice_command` modules in `lib.rs` are compatibility re-exports; new code uses
`voice` directly.

## Focused submodule maps

- `station_icons/` separates domain models/normalization, filesystem atomic storage, external HTTP/HTML favicon fetching, and PostgreSQL import job coordination. Its root is a stable facade.
- `device_control/` separates foundation IDs/time, validation, manifest metadata, runtime state,
  command payloads, command lifecycle, revision ordering, and the store contract. Its root is a
  stable facade; add a new concept to its owning file rather than to the facade.
- `persistence/account_postgres/` groups account, browser, passkey, pairing, rate-limit, cleanup,
  and native-session flows. `rows.rs` owns private SQL row conversions; it is not a domain-model
  home.
- `http/control/` keeps protocol framing/serialization (`protocol.rs`), revision admission and validation (`state.rs`), and transport/unit tests (`tests.rs`) apart from the WebSocket ingress lifecycle (`control.rs`).
- `http/endpoints/` separates Axum route definitions (`routes.rs`) and state assembly via
  `RouterBuilder` (`builder.rs`) from endpoint convenience facades and unit tests (`tests.rs`).
- `personal_data/` separates domain sync records, limits, and the `PersonalDataStore` trait (`domain.rs`) from the in-memory merge/retention implementation (`in_memory.rs`) and unit test coverage (`tests.rs`). Its root is a stable facade.
- `admin/` separates administrator identity models, DTOs, and the `AdminStore` trait (`domain.rs`) from deterministic in-memory session and credential tracking (`fake.rs`) and unit tests (`tests.rs`). Its root is a stable facade.
- `device_control_command/` separates station catalog resolution (`catalog.rs`), target capability and role validation (`target_validation.rs`), and the stateful in-flight command router lifecycle (`router.rs`) with dedicated unit test coverage (`tests.rs`). Its root is a stable facade.
- `http/voice/` separates wire protocol framing, JSON events, error codes, and device voice intent
  planning/execution from the streaming WebSocket and REST command lifecycle.
- `search/` separates domain query and station models and the `StationRepository` trait (`domain.rs`), in-memory and unavailable pinned repository implementations (`in_memory.rs`), and the multi-stage search service and progressive genre fallback orchestration (`service.rs`). Its root is a stable facade.
- `tests/openapi_contract/` separates OpenAPI contract verification by domain area (`surface.rs`, `device_control.rs`, `device_catalog.rs`, `voice_stream.rs`, `personal_sync.rs`) with shared schema validators and fixture specifications in `common.rs`.
- `tests/postgres_integration/` separates database integration coverage into focused thematic submodules (`admin.rs`, `account.rs`, `account_cleanup.rs`, `yandex_home.rs`, `device_control.rs`, `migrations.rs`, `shared_catalog.rs`, `personal_data.rs`) behind a single test binary target with shared inspection helpers in `common.rs`.

## Tests and safe reading order

Unit tests sit in the owning module or its private `tests.rs` sibling and use deterministic fakes.
Cross-boundary API and PostgreSQL coverage is organized into thematic submodules under `tests/`
(e.g. `tests/openapi_contract/`, `tests/postgres_integration/`) behind minimal facades; integration
tests must not be merged into multi-thousand-line single files. Live provider tests are explicitly
ignored and must be run only with owner authorization. For a change, read the root module, its focused
submodule map, the nearest unit test, and then the external contract or integration test. Do not load
unrelated provider, SQL, or fixture code first.
