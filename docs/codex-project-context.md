# Канонический контекст для Codex: RockServer, RockCast и RockMobile

> **Назначение.** Это постоянная точка входа для последующих задач Codex по трём
> проектам. Перед началом любой такой задачи сначала прочитать этот файл, затем
> проверить фактическое состояние затронутого кода и документации. Этот документ
> описывает актуальные решения и намерения, а не заменяет код, миграции, OpenAPI
> или `AGENTS.md`.
>
> **Актуальность:** 2026-09-08. Если код или утверждённые документы ниже им
> противоречат, приоритет имеют: (1) безопасность и `AGENTS.md`, (2) фактический
> код и действующий OpenAPI, (3) этот контекст, (4) старые отчёты и обсуждения.

## Как начинать задачу

1. Прочитать `AGENTS.md` каждого затрагиваемого репозитория и этот файл.
2. Прочитать `docs/status.md`, `docs/tasks.md` и документы, указанные в разделе
   задачи. Затем сверить их с кодом: часть roadmap может быть уже выполнена.
3. Не смешивать изменения серверной логики с UI/воспроизведением клиентов без
   явной необходимости. Один логический этап — одна ограниченная задача.
4. До изменения публичного HTTP-поведения обновить `api/openapi.yaml`, DTO и
   контрактные тесты. Действующие публичные пути в RockServer версионируются;
   текущий icon-roadmap планирует `/api/v1/stations/{id}/icon`.
5. Не читать, не печатать и не коммитить секреты. Не добавлять сеть в SQL migration,
   startup, readiness или request path, если это не оговорено отдельно.
6. Сохранять обратную совместимость. Если требуется несовместимое решение,
   остановиться, описать миграционный путь и запросить решение владельца.
7. Не создавать монолитные файлы (>400-500 строк). При проектировании или
   доработке сразу разделять зоны ответственности:
   - Доменные модели, типы и валидация (`domain.rs`).
   - Трейты хранилищ и внешних провайдеров (`domain.rs` или `traits.rs`).
   - Маршрутизация команд, бизнес-логика и оркестрация (`router.rs` / `service.rs`).
   - Сетевой протокол, фрейминг WebSocket, DTO и сборка роутера (`protocol.rs` / `routes.rs` / `builder.rs`).
   - Реализации хранилищ и детерминированные тестовые фейки (`in_memory.rs` / `fake.rs` / `persistence/`).
   - Корневые модули (`mod.rs` или `<name>.rs`) делать тонкими фасадами для реэкспорта.
   - Юнит-тесты выносить в отдельные файлы `tests.rs` внутри подмодулей.
   - Интеграционные тесты (`tests/`) декомпозировать на тематические файлы в подкаталоге `tests/<suite>/`,
     подключая их через `#[path = "..."] mod ...;` в едином бинарном таргете (особенно важно для тестов с БД),
     а общие хелперы и мок-провайдеры размещать в `common.rs`.

## Репозитории и ответственность

| Проект | Путь | Технологии | Граница ответственности |
|---|---|---|---|
| RockServer | `C:\repos\rockserver` | Rust 2024, Axum, SQLx/PostgreSQL, Vite/Preact web | API, поиск, каталог, импорт, storage, auth и operations. |
| RockCast | `C:\repos\rockcast` | Rust 2024, egui/eframe | Windows UI, playback, Google Cast, локальный каталог и клиентский icon cache. |
| RockMobile | `C:\repos\rockmobile` | Kotlin, Compose, Media3, Room | Android UI/playback, offline catalogue и клиентский image cache. |

Параллельный, но отдельный authoring source baseline-каталога: `C:\repos\rockcast-station-catalog`.
Клиентские snapshot-ы обновляются только его проверяемым release workflow, не ручным копированием.

## Фактическое состояние на дату контекста

### RockServer

- Сервис уже реализует поиск/ранжирование, public catalog, голосовые команды и
  streaming voice, PostgreSQL persistence, Radio Browser и shared-catalog import,
  embeddings/backfill, passkey account/device pairing и browser account centre.
- Импорт Radio Browser — отдельный bounded one-shot binary. Он не является HTTP
  handler-ом, не запускается при startup и не выполняется в readiness. Existing
  `import_runs` фиксирует каталоговые import runs.
- OpenAPI declares `GET /api/v1/stations/{station_id}/icon`, protected durable
  admin import start/status paths, and nullable `favicon_url` in station
  responses. The endpoint serves only ready WebP artifacts from
  `ROCKSERVER_STATION_ICON_DIR`; ready PostgreSQL stations expose its
  same-origin path and every other station remains `null` for the client
  placeholder.
- `docs/roadmap/station-icons.md` steps 2–9 are implemented locally. Existing
  mobile export `favicon_url` remains intentionally `NULL`; metrics, operator
  diagnostics and post-deploy acceptance remain planned.
- `GET /admin` — first-party Preact SPA с отдельным persisted administrator
  principal, логином и short-lived revocable Bearer session. Its Stations tab
  now starts an Origin-protected import, restores/polls persisted progress,
  renders the server icon URL or a placeholder, and provides bounded manual
  upload/replace/confirmed-removal controls.
- Current user accounts/passkey/browser sessions — это **не** будущая роль
  оператора admin и не должны переиспользоваться для неё без отдельного решения.

### RockCast

- Desktop client умеет local/Cast playback, station search/voice integration,
  local catalog fallback и безопасное pairing с RockServer.
- `Station` уже переносит `favicon_url`; есть `src/station_icons.rs` и bounded
  background cache. Пока server-owned icon API отсутствует, клиент использует
  explicit favicon URL либо conventional `/favicon.ico` homepage как переходный
  MVP fallback.
- После появления server icon API клиент должен предпочитать полученный
  RockServer URL. Прямые внешние загрузки нельзя считать постоянным контрактом.

### RockMobile

- Android client умеет server search/voice, Media3 playback, offline fallback и
  pairing/device sessions. Bundled baseline и extended SQLite catalog остаются
  офлайн-страховкой.
- Модель станции и DTO уже содержат `faviconUrl`; `StationIconLoader` временно
  получает explicit URL или `/favicon.ico` homepage и кеширует результат.
- После server-owned icon API mobile должен использовать только опубликованный
  URL, а при `null`/ошибке показывать локальный placeholder; playback не зависит
  от artwork.

## Архитектурные решения, которые нельзя незаметно менять

### Каталог и клиенты

```text
Radio Browser / shared catalog
        -> bounded RockServer importer -> PostgreSQL
        -> versioned API -> RockCast / RockMobile

Pinned client catalog -> local fallback when server is unavailable
```

- RockServer владеет серверным каталогом, поиском и импортом; клиенты владеют UI
  и воспроизведением.
- Importer, HTTP search и provider client разделены. HTTP не должен crawl-ить
  Radio Browser, обновлять embeddings, probe-ить streams или выполнять длительный
  импорт.
- Upsert import-а идемпотентен, использует source ownership и стабильные station/
  stream IDs. Не заменять IDs, не делать один долгий DB transaction и не выполнять
  сеть под DB lock.

### Station icons: утверждённый целевой контракт

```text
catalog source/homepage/manual admin upload
        -> explicit protected admin import job
        -> metadata in PostgreSQL + WebP file storage
        -> GET /api/v1/stations/{id}/icon
        -> faviconUrl only for ready files -> client cache/placeholder
```

- RockServer — канонический origin готовой иконки. Внешний URL внутренний
  (`source_url`) и никогда не отдаётся клиенту как fallback.
- Иконка не хранится в PostgreSQL: там только metadata. Persistent filesystem
  storage configurable through `ROCKSERVER_STATION_ICON_DIR`; future S3/MinIO
  remains possible behind abstraction. **Единственный хранимый и выдаваемый
  формат v1 — WebP:** любой допустимый исходник нормализуется в WebP до
  публикации; оригинал не сохраняется и не выдаётся.
- До готового, проверенного файла `faviconUrl` равен `null`; icon endpoint
  возвращает `404` с `Cache-Control: no-store`. Placeholder рисует клиент. Это
  заменяет раннюю идею server-generated fallback в MVP.
- Endpoint читает только ready metadata/file; он не скачивает URL, не парсит
  homepage, не создаёт job и не держит DB lock во время сети.
- v1 accepts raster PNG/JPEG/WebP/ICO after MIME **and signature** validation;
  SVG excluded. Body <= 2 MiB; decoded image <= 1024x1024 and <= 1,048,576 px;
  output is transparent-preserving square WebP <=256x256.
- Source priority: valid explicit catalog icon URL -> favicon of valid homepage
  -> no source. A lower priority source must not displace a higher priority one.
- Downloader is an untrusted-network boundary: validate redirect and every
  resolved address (SSRF/DNS-rebinding/private/local/link-local/reserved blocking),
  bounded timeouts/redirects/retries, size/type/decode limits and sanitized logs.
- Writes are atomic; mark metadata `ready` only after file commit. Retain old ready
  file until a replacement has been verified. Metadata states: `pending`, `ready`,
  `missing`, `retryable_error`, `permanent_error`.
- Ready response: `200 image/webp`, content hash strong ETag, Last-Modified and
  `Cache-Control: public, max-age=86400, must-revalidate`; matching ETag -> `304`.

### Admin authentication: утверждённый план

The repository decision record is `docs/admin-security-plan.md`; it supersedes
the earlier chat suggestion of an HttpOnly admin cookie. The chosen first
delivery is a server-rendered Axum + HTMX console with a **short-lived opaque
Bearer access token held only in browser memory**, not `localStorage`.

- Passwords: Argon2id PHC hash only. Tokens/session identifiers: random opaque
  values; persist and compare only hashes. Never log them.
- Bootstrap one administrator through protected deploy environment or terminal;
  no public admin registration. For staging, the initial password is an ignored
  deployment secret/environment value, consumed only to create the missing admin;
  a restart must never overwrite a stored password.
- Admin principals/sessions are separate from RockCast machine-client credentials
  and from passkey user/device sessions. Client credentials are revocable, shown
  once and never grant console access.
- Admin login requires durable account+source-IP throttling, progressive delay,
  temporary lockout, generic failures and a security audit event.
- Admin Bearer sessions are short-lived, server-revocable and rotated on
  login/refresh. State-changing admin requests also validate `Origin`.
- UI: strict CSP, escaped HTML, no third-party scripts, `Cache-Control: no-store`,
  clear in-memory token on logout or `401`; no sensitive persistence by default.
- Future public/open API behaviour must declare security schemes and `401`/`403`/
  `429` responses. Existing account-cleanup operator is a separate staging-only
  root-scoped one-shot tool; it is not an admin console.

## Delivery rules for Codex

- Respect each repository's `AGENTS.md`. In RockServer all changed public Rust
  APIs/modules/types need meaningful Rustdoc; non-obvious private logic needs a
  concise explanatory comment.
- No real network in unit tests. Use deterministic fakes/local test servers for
  importer, image, SSRF and HTTP cases. Do not use real LLMs or external providers.
- **Каждый новый или изменённый запрос к PostgreSQL обязан иметь проверку на
  disposable PostgreSQL базе.** Запускать соответствующий integration test с
  `TEST_DATABASE_URL` против одноразовой, не staging/production БД. Unit tests,
  mocks и in-memory fakes дополняют, но не заменяют эту проверку; в отчёте явно
  указывать, была ли disposable-БД доступна и результат теста.
- For RockServer changes update `docs/status.md` and chronological `docs/tasks.md`;
  update `docs/service-diagrams.html` only when current or explicitly planned
  architecture changed.
- Required RockServer handoff checks: `cargo fmt --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.
  Also run focused web/API/OpenAPI checks where applicable.
- For RockMobile, Gradle commands must run serially from its root with the
  repository's process-local `JAVA_TOOL_OPTIONS` instruction. Never start any
  build/test/dependency install while another Codex-started build is running.
- **Никогда не запускать компиляцию проектов параллельно.** Перед `cargo`, Gradle,
  npm/pnpm, Docker image build или любой проверкой, которая компилирует код,
  дождаться и проверить результат предыдущей Codex-задачи такого типа.
- Before editing, inspect dirty state; preserve unrelated user changes. Do not
  deploy, mutate staging data, run an irreversible cleanup or expose a secret
  unless the user explicitly authorizes that action.
- **Все Codex-задачи по RockServer выполняются только в основном checkout
  `C:\repos\rockserver` на ветке `master`.** Отдельные Git worktree для этого
  репозитория не создавать и не использовать. Перед началом проверить
  `git status` и `git branch --show-current`; если активный каталог или ветка не
  совпадают с этим правилом, перейти в основной checkout на `master` до любых
  правок. Не менять ветку и не создавать commit без явного требования задачи.

## External agent delegation: Antigravity CLI (`agy`) — утверждено 2026-09-08

Локальный Codex/ZCode-агент может делегировать ограниченные подзадачи внешнему
агенту Google Antigravity через установленный CLI `agy`
(`C:\Users\alex\AppData\Local\agy\bin\agy.exe`; после перезапуска хоста доступен
в PATH как `agy`). Это стандартная практика, а не разовая утилита.

**Когда делегировать.** Подходящие задачи: независимое второе мнение по большим
диффам и планам; исследовательские сравнения технологий (память, лицензии,
совместимость); ревью документации. Неподходящие: детерминированный код,
проверяемый тестами/fixtures; быстрые справочные вопросы (латентность одного
вызова — десятки секунд); работа с PostgreSQL-запросами, миграциями и
OpenAPI-контрактом, где обязательны локальные строгие проверки.

**Как вызывать.** Headless-режим: `agy -p "<prompt>" --output-format json`
(опционально `--json-schema` для структурированного результата, `--model` и
`--effort` под задачу). Рабочая папка вызова — не корень репозитория, а
конкретные файлы или временная директория. Модель выбирать сознательно
(например, Pro для анализа, Flash для простых ревью).

**Границы безопасности.** В промпты внешнему агенту никогда не передаются:
`production.env`, локальные `.env`, кейсторы, pairing/device secrets, токены,
содержимое БД и пользовательские данные. Любой код или текст от внешнего агента
попадает в репозиторий только после локальной проверки и обязательных проверок
репозитория (`cargo fmt/clippy/test` и т. д.). Ответственность за результат
остаётся на локальном агенте; каждый ключевой факт из внешнего ответа,
используемый в решении, сверяется с независимым источником.

**Стоимость.** Каждый вызов расходует квоту Google владельца. Не делать
массовых параллельных вызовов; один вызов — одна ограниченная подзадача.

## Ordered execution backlog

Statuses mean actual repository state, not an instruction to blindly implement.
Each task begins by re-validating its status and dependencies.

### RS-ADMIN

| ID | Status | Scope and acceptance boundary |
|---|---|---|
| RS-ADMIN-001 | Planned | Add durable admin principal, credential/session, login-attempt and security-event migrations plus domain/persistence interfaces and deterministic fakes. Separate from user/device and machine-client credentials; test every new/changed PostgreSQL query on a disposable DB via `TEST_DATABASE_URL`. |
| RS-ADMIN-002 | Planned | Bootstrap admin from protected terminal/deployment env; Argon2id hash, one-time creation, ignored `.env` on staging, no secret logs or password reset on restart. |
| RS-ADMIN-003 | Planned | Login/logout/refresh, opaque revocable Bearer sessions, middleware/API gate, role enforcement, durable throttling, generic failures and audit events. Add OpenAPI security/error cases. |
| RS-ADMIN-004 | Planned | Minimal protected Axum+HTMX console: login, in-memory bearer hand-off, stations/read models, strict CSP/no-store/Origin protection; no token persistence. |
| RS-ADMIN-005 | Complete (local, uncommitted) | Atomic administrator-session rotation, bounded request records with a 30-day retention decision, backup/incident runbook and security acceptance review. RockCast machine credentials are explicitly out of scope: current RockCast product routes are public and its user identity uses pairing. |

### RS-ICON

> **Статус направления:** владелец реактивировал направление 2026-09-22.
> Импорт не имеет CLI, cron, startup- или deploy-side пути: только явный,
> защищённый запуск durable background job из `https://rockplatform.win/admin`
> с сохранённым прогрессом. Текущие client-side favicon fallback-механизмы
> остаются переходным поведением до фактической реализации.

| ID | Status | Scope and acceptance boundary |
|---|---|---|
| RS-ICON-001 | Complete (documentation) | Contract is recorded in `docs/roadmap/station-icons.md` step 0. Re-check it against OpenAPI/DTOs before code; it is not live behaviour. |
| RS-ICON-002 | Complete locally (uncommitted) | Added the schema-only `station_icons` migration; it preserves station/stream IDs and counts and performs no network I/O. Repository access remains the next step. |
| RS-ICON-003 | Complete locally (uncommitted) | Shared and Radio Browser imports normalize `favicon_source_url`, persist it as internal metadata, and flag a changed automatic source for refresh while retaining a ready artifact. |
| RS-ICON-004 | Complete locally (uncommitted) | Added content-addressed WebP storage with canonical keys, an atomic filesystem backend and deterministic storage tests. Downloader, endpoint and metadata publication remain separate steps. |
| RS-ICON-005 | Complete locally (uncommitted) | Added bounded raster validation and WebP normalization plus a redirect-limited public-address fetcher. It blocks local/private/link-local DNS results; it is not callable until the admin job is added. |
| RS-ICON-006 | Complete locally (uncommitted) | Added durable job/item persistence, one-active-job guard, snapshot selection, persisted counters and safe interruption semantics. It remains unreachable until the protected admin endpoints/UI are wired. |
| RS-ICON-007 | Complete locally (uncommitted) | Read-only WebP icon endpoint, OpenAPI and router checks are wired; ready artifact integration coverage needs a disposable PostgreSQL database. |
| RS-ICON-008 | Planned | Add nullable RockServer `faviconUrl` to domain/persistence/search/voice/public DTOs only for available ready files; preserve ranking/order and reverse-proxy public-base correctness. |
| RS-ICON-009 | Complete locally (uncommitted) | Protected same-origin admin controls start import, poll durable progress/history, and show server-hosted thumbnail/placeholder in the station list. |
| RS-ICON-010 | Complete locally (uncommitted) | Bounded raw raster upload, manual replace and explicitly confirmed removal reuse the normalizer; automatic source metadata survives and the request audit trail remains safe. |
| RS-ICON-011 | Planned | Deploy compatible schema and persistent icon volume with backup, coverage/endpoint/admin smoke checks and partial-success semantics. Deploy never triggers import. |

Recommended sequencing: RS-ICON-002..007 establish safe server storage and
read-only delivery; RS-ICON-009..010 add the administrator import and manual
override controls. RS-ICON-008 remains the client catalog contract stage, and
RS-ICON-011 deploys without triggering network work.

## Start prompt for a future Codex task

```text
Work on [TASK-ID] in the RockServer/RockCast/RockMobile project set.

First read C:\repos\rockserver\docs\codex-project-context.md and the relevant
AGENTS.md files. Then read RockServer docs/status.md, docs/tasks.md and the
task's linked roadmap/design document, and inspect the actual implementation.

Treat the code and current OpenAPI as the source of truth for what is already
implemented. Do not duplicate completed work. Keep the change limited to the
task, preserve unrelated working-tree changes, and do not deploy or mutate staging.

Do not run compilation, build, compiling test, or dependency-install commands
in parallel with another Codex-started command of that type in any project.
Wait for the earlier command to finish and verify its result first.

For every new or changed PostgreSQL query, add and run a relevant integration
test against a disposable database using TEST_DATABASE_URL. Unit tests, mocks
and in-memory fakes do not replace that check. State availability and result of
the disposable-database test in the final report.

Work only in the primary C:\repos\rockserver checkout on branch master. Do not
create or use a separate Git worktree for RockServer; if you are elsewhere,
switch to that checkout before editing. Inspect and preserve unrelated
working-tree changes before editing.

Implement the smallest correct slice, update the required documentation and
contract/tests, run the required checks, then report: decision made, files
changed, verification performed, known limitations, and the next task.
```

## Reference documents

- `C:\repos\rockserver\AGENTS.md`
- `C:\repos\rockserver\src\ARCHITECTURE.md` — compact source map for cross-module navigation.
- `C:\repos\rockserver\docs\status.md`
- `C:\repos\rockserver\docs\tasks.md`
- `C:\repos\rockserver\docs\architecture.md`
- `C:\repos\rockserver\docs\admin-security-plan.md`
- `C:\repos\rockserver\docs\roadmap\station-icons.md`
- `C:\repos\rockserver\api\openapi.yaml`
- `C:\repos\rockcast\AGENTS.md`, `C:\repos\rockcast\docs\README.md`
- `C:\repos\rockmobile\AGENTS.md`, `C:\repos\rockmobile\docs\README.md`
