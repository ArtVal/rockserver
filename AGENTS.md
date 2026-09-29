# RockServer contributor instructions

## Purpose

RockServer is a Rust service that will turn natural-language radio requests into a ranked list of internet radio streams for the Windows RockCast client.

## Repository boundaries

- Keep this repository focused on the server API, search domain, persistence, provider abstractions, and operational concerns.
- Keep RockCast UI and playback changes in the RockCast repository.
- Do not combine server and client work in one change.
- Preserve RockCast's local station catalog as an offline fallback.

## Architecture rules

- `api/openapi.yaml` is the source of truth for the HTTP contract once it is introduced.
- Version public endpoints under `/v1`.
- Return errors with `code`, `message`, `request_id`, and `details`.
- Separate HTTP DTOs, domain models, persistence, and external providers.
- Use traits around query parsing and embeddings; tests must use deterministic fakes.
- An LLM may translate a request into structured filters, but must not scan the full catalog.
- Never commit secrets or local environment files.
- Unit tests must not call real LLMs or the external network.
- Keep changes small and aligned with one roadmap stage.
- Keep the crate on Rust edition 2024.

## Modularity and code organization

- Avoid monolithic files: keep source files focused and bounded, aiming for under 400 lines (hard ceiling ~500 lines).
- When introducing or refactoring functionality, decompose into distinct architectural layers immediately rather than accumulating everything in one file:
  - Pure domain models, validation, and domain error types (`domain.rs`).
  - Abstract storage and provider traits (`traits.rs` or in `domain.rs`).
  - Stateful routers, services, and business logic orchestration (`service.rs` / `router.rs`).
  - Wire transport protocols, WebSocket framing, routing, and HTTP DTOs (`protocol.rs` / `routes.rs` / `builder.rs`).
  - Storage implementations, SQL row mappers, and deterministic test fakes (`in_memory.rs` / `fake.rs` / `persistence/`).
  - Thin facades: module entry points (`mod.rs` or `<name>.rs`) must be clean facades re-exporting public types without holding substantial logic.
- Unit tests: isolate unit test suites into dedicated `tests.rs` submodules alongside the module under test instead of appending hundreds of lines of `#[cfg(test)]` to implementation files.
- Integration tests: do not accumulate multi-thousand-line monolithic integration test files.
  - Decompose integration test suites thematically into submodule files under `tests/<suite_name>/` (e.g. `tests/<suite_name>/<topic>.rs`).
  - Declare them in the root test file `tests/<suite_name>.rs` using `#[path = "<suite_name>/<topic>.rs"] mod <topic>;`.
  - Isolate shared test fixtures, assertion helpers, and mock providers in a `common.rs` submodule.
  - When tests require isolated or serialized execution (such as database truncation in `tests/postgres_integration.rs`), preserve a single test target with submodules rather than creating separate concurrently executing binaries or collapsing into a monolith.

## Code comments

- All new or modified public functions, methods, types, and modules must have meaningful Rustdoc comments (`///` or `//!`).
- Complex private functions and methods must have a short comment explaining their purpose, invariants, non-obvious design choices, errors, or side effects.
- Do not add comments that merely restate an obvious line of code.
- When changing an existing method's behavior, verify that its comments remain accurate.
- Apply these rules to code created or substantially changed by the task; do not perform unrelated mass comment rewrites.

## Required checks

Run all of the following before handing off a change:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Update the OpenAPI contract and relevant tests whenever a public HTTP behavior changes.

## Project documentation

- Every task that changes the project must update `docs/status.md` and `docs/tasks.md` before completion.
- `docs/status.md` records the current working state, current stage, known limitations, latest verification results, and next step.
- `docs/tasks.md` is a chronological task log recording each task's goal, scope, result, checks, and status.
- Documentation must describe verified, actual behavior rather than planned or assumed behavior.
- Do not create a separate document for every small task unless the task genuinely needs one.
- When architecture or the public API changes, check `docs/service-diagrams.html` for accuracy. Update it only to reflect verified current behavior or explicitly labeled planned state.
