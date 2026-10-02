import assert from "node:assert/strict";import test from "node:test";import { readFile } from "node:fs/promises";const componentFiles = [  "../src/app.tsx",  "../src/components/AccountCentre.tsx",  "../src/components/PairingView.tsx",  "../src/components/Header.tsx",  "../src/components/SidebarNav.tsx",  "../src/components/StationsView.tsx",  "../src/components/HardwareHud.tsx",  "../src/components/YandexHomeCard.tsx",  "../src/components/PlayerDeck.tsx"];const app = (await Promise.all(componentFiles.map(f => readFile(new URL(f, import.meta.url), "utf8")))).join("\n");const api = await readFile(new URL("../src/api.ts", import.meta.url), "utf8");const css = await readFile(new URL("../src/style.css", import.meta.url), "utf8");test("secure pairing reads a fragment secret once, immediately removes it, and keeps bounded legacy query support", () => {  assert.match(app, /new URLSearchParams\(location\.search\)/);  assert.match(app, /new URLSearchParams\(location\.hash\.slice\(1\)\)/);  assert.match(app, /const fragmentSecret = fragment\.get\("secret"\) \?\? ""/);  assert.match(app, /LEGACY_QUERY_SECRET_ROLLOUT_END/);  assert.match(app, /Date\.now\(\) <= LEGACY_QUERY_SECRET_ROLLOUT_END \? legacySecret : ""/);  assert.match(app, /params\.delete\("secret"\);[\s\S]*history\.replaceState/);  assert.match(app, /const handoff = useRef\(\{[\s\S]*approvalSecret/);  assert.match(app, /approvalSecret: inMemoryApprovalSecret/);  assert.doesNotMatch(app, /console\.(log|debug|info|warn|error)\(/);});test("secure pairing keeps only its current URL context and cannot approve from a terminal state", () => {  assert.match(app, /const isPairing = Boolean\(code && approvalSecret\)/);  assert.match(app, /isPairing && !showCabinet\) void lookup\(\)/);  assert.match(app, /disabled=\{pairingState !== "authenticated"\}/);  assert.match(app, /setPairingState\("approved"\)/);  assert.match(app, /setPairingState\("terminal"\)/);});test("a live browser session can approve pairing without a second passkey", () => {  assert.match(app, /current === "approved" \|\| current === "approving" \|\| current === "terminal" \|\| current === "unavailable" \? current : "authenticated"/);  assert.match(app, /pairingState === "authenticated" \|\| pairingState === "approving"/);  assert.match(app, /Сессия завершена. Войдите с passkey/);  assert.doesNotMatch(app, /Для подключения устройства требуется свежая проверка passkey/);});test("registration remains distinct from browser authentication", () => {  assert.match(app, /const \[registrationName, setRegistrationName\]/);  assert.match(app, /const \[authenticatedAccountName, setAuthenticatedAccountName\]/);  assert.match(app, /onInput=\{event => setRegistrationName\(event\.currentTarget\.value\)\}/);  assert.match(app, /Создать Rock-аккаунт/);  assert.doesNotMatch(app, /onInput=\{event => setAuthenticatedAccountName/);});test("cabinet has exclusive loading, anonymous, authenticated, expired, and unavailable states", () => {  assert.match(app, /type AccountState = "loading" \| "anonymous" \| "authenticated" \| "expired" \| "unavailable"/);  assert.match(app, /accountState === "loading"/);  assert.match(app, /accountState === "unavailable"/);  assert.match(app, /accountState === "expired"/);  assert.match(app, /accountState === "anonymous"/);  assert.match(app, /Сессия браузера завершена/);  assert.match(app, /Сервис временно недоступен/);});test("account loading fetches a fresh browser session and exact current device projection", () => {  assert.match(api, /browserSession\(\).*\/v1\/auth\/browser-session/);  assert.match(api, /browserAccount\(\).*\/v1\/browser\/account/);  assert.match(app, /const nextAccount = await api\.browserAccount\(\)/);  assert.match(app, /setAccount\(undefined\); setCsrf\(""/);  assert.match(app, /setAccountState\("expired"\)/);});test("device actions are confirmed, independently busy, and refresh account data", () => {  assert.match(api, /renameDevice\(.*\/v1\/browser\/devices/);  assert.match(api, /revokeDevice\(.*\/v1\/browser\/devices/);  assert.match(app, /const \[deviceBusy, setDeviceBusy\]/);  assert.match(app, /const \[logoutBusy, setLogoutBusy\]/);  assert.match(app, /await api\.renameDevice[\s\S]*await refreshAccount\(\)/);  assert.match(app, /await api\.revokeDevice[\s\S]*await refreshAccount\(\)/);  assert.match(app, /Это не завершит вход в текущем браузере/);});test("cabinet explains browser safety and distinguishes empty and full device limits", () => {  assert.doesNotMatch(app, /✓ Выполнен вход в браузере/);  assert.match(app, /Браузер подтверждает устройство, но не является RockMobile или RockCast/);  assert.match(app, /Подключённые устройства/);  assert.match(app, /aria-label="Список подключённых устройств"/);  assert.doesNotMatch(app, /из \{account\.device_limit\} устройств/);  assert.match(app, /Подключённых устройств пока нет/);  assert.match(app, /Лимит устройств достигнут/);  assert.match(app, /Passkey подтверждает вход в этот браузер/);  assert.match(app, /не удаляет passkey из браузера или Google Password Manager/);});test("device presentation renders one product prefix and no secret or identifier DOM fields", () => {  assert.match(app, /deviceName\(device\)/);  assert.doesNotMatch(app, /\{preview\.request_id\}/);  assert.doesNotMatch(app, /access_token|refresh_token|desktop_token|approval_secret/);});test("pairing success offers cabinet handoff with an in-memory one-time connected marker", () => {  assert.match(app, /const \[justConnected, setJustConnected\]/);  assert.match(app, /setShowCabinet\(true\)/);  assert.match(app, /Только что подключено/);  assert.match(app, /Вернуться в RockMobile/);  assert.match(app, /Вернитесь в RockCast или закройте браузер/);});test("accessible controls have semantic status and visible focus styles across mobile layouts", () => {  assert.match(app, /role="alert"/);  assert.match(app, /role="status"/);  assert.match(app, /aria-label=\{.*Проверочная фраза/);  assert.match(css, /:focus-visible/);  assert.match(css, /@media \(max-width: 480px\)/);  assert.match(css, /button, \.button \{ width: 100%/);});test("player deck embeds html5 audio node, volume controls, and real-time sse now-playing synchronization", () => {  assert.match(app, /audio\s+ref=\{audioRef\}/);  assert.match(app, /\/api\/v1\/stations\/\$\{encodeURIComponent\(currentStation\.id\)\}\/stream/);  assert.match(app, /\/api\/v1\/stations\/\$\{encodeURIComponent\(currentStation\.id\)\}\/events/);  assert.match(app, /api[\s\S]*\.stationNowPlaying\(currentStation\.id\)/);  assert.match(app, /new EventSource\(eventsUrl\)/);  assert.match(app, /master-play-btn/);  assert.match(app, /volume-slider/);  assert.match(app, /localStorage\.getItem\("rockserver_player_volume"\)/);});test("stations view provides tuner selector, bitrate badges, and live vu-equalizer animation", () => {  assert.match(app, /tuner-selector-tag[\s\S]*Прямой эфир/);  assert.doesNotMatch(app, /TUNER SELECTOR/);  assert.match(app, /vu-equalizer/);  assert.match(app, /vu-bar/);  assert.match(app, /bitrate-badge/);  assert.match(app, /station\.favicon_url/);});test("cabinet keeps one search, four sections, catalog filters, and persistent player", () => {  assert.match(app, /Поиск станции, жанра или потока/);  assert.equal((app.match(/type="search"/g) ?? []).length, 1);  for (const label of ["Эфир", "Избранное", "История", "Устройства"]) assert.match(app, new RegExp(`<span class="nav-label">${label}</span>`));  assert.match(app, /#\{tag\}/);  assert.doesNotMatch(app, /Cloud Sync \(LWW\)|RELAY ONLINE/);  assert.match(app, /<\/main>[\s\S]*<PlayerDeck/);  assert.match(css, /\.sidebar-nav \{[\s\S]*position: fixed; bottom: 0/);});test("css styling implements cyber-tuner palette, vu-bounce keyframes, and fixed bottom audio deck", () => {  assert.match(css, /@keyframes vuBounce/);  assert.match(css, /\.player-deck-bar/);  assert.match(css, /backdrop-filter:\s*blur\(16px\)/);  assert.match(css, /@media \(max-width: 1200px\)/);  assert.match(css, /@media \(max-width: 768px\)/);});test("dim text keeps WCAG AA contrast on deck, space, and lightened chip backgrounds", () => {  assert.match(css, /--text-dim: #7a92a5;/);  assert.doesNotMatch(css, /--text-dim: #546676;/);});

test("stage 3 uses real metadata, aligned favorites, explicit actions, and retryable search", async () => {
  const view = await readFile(new URL("../src/components/StationsView.tsx", import.meta.url), "utf8");
  const centre = await readFile(new URL("../src/components/AccountCentre.tsx", import.meta.url), "utf8");
  const sync = await readFile(new URL("../src/usePersonalSync.ts", import.meta.url), "utf8");
  assert.doesNotMatch(view, /192 KBPS|: "MP3"|Чистый поток без рекламы/);
  assert.match(view, /Math\.round\(station\.bitrate_kbps!\)/);
  assert.match(view, /station\.country_code[\s\S]*station\.language/);
  assert.match(view, /Данные о станции не указаны/);
  assert.match(view, /Данные не указаны/);
  assert.match(view, /Станция без названия/);
  assert.match(view, /onSearchSubmit\(\)/);
  assert.match(view, /aria-pressed=\{viewMode === "table"\}/);
  assert.equal((view.match(/onClick=\{\(\) => onPlayStation\(station\)\}/g) ?? []).length, 1);
  assert.match(centre, /const savedStations = favorites\.map/);
  assert.match(centre, /favoriteCount=\{savedStations\.length\}/);
  assert.match(centre, /setSearchAttempt\(\(attempt\) => attempt \+ 1\)/);
  assert.match(sync, /history\.find\(\(h\) => h\.id === stId\)\?\.name/);
});

test("stage 4 keeps one persistent audio node, honest statuses, retry, and reserved mobile space", async () => {
  const player = await readFile(new URL("../src/components/PlayerDeck.tsx", import.meta.url), "utf8");
  const centre = await readFile(new URL("../src/components/AccountCentre.tsx", import.meta.url), "utf8");
  const harness = await readFile(new URL("./browser-harness.mjs", import.meta.url), "utf8");
  assert.equal((player.match(/useRef<HTMLAudioElement>/g) ?? []).length, 1);
  assert.equal((player.match(/<audio\b/g) ?? []).length, 2); // idle and active branches render exclusively
  assert.match(player, /aria-live="polite"/);
  assert.match(player, /Повторить/);
  assert.match(player, /setRetryCount\(\(count\) => count \+ 1\)/);
  assert.match(player, /Подключение к эфиру…/);
  assert.match(player, /Прямой эфир/);
  assert.match(player, /Поток временно недоступен или требуется вход/);
  assert.match(player, /Math\.round\(currentStation\.bitrate_kbps\)/);
  assert.doesNotMatch(player, /320 KBPS|DIRECT RELAY|LIVE ICY|CAST: RockCast/);
  assert.match(player, /master-play-btn/);
  assert.match(player, /volume-slider/);
  assert.match(player, /player-star-btn/);
  assert.match(player, /player-status-badge/);
  assert.match(centre, /setTrackTitle\(""\)/);
  assert.doesNotMatch(centre, /setTrackTitle\("Подключение/);
  assert.match(css, /\.player-status-badge\.playing/);
  assert.match(css, /\.player-retry-btn/);
  assert.match(css, /\.player-deck-bar \{ bottom: calc\(62px \+ env\(safe-area-inset-bottom\)\)/);
  assert.match(css, /\.cabinet-layout \{ padding-bottom: 210px; \}/);
  assert.match(css, /@media \(max-width: 900px\)[\s\S]*\.player-deck-inner \{ flex-wrap: wrap/);
  assert.match(harness, /--stage4/);
  assert.match(harness, /\/\(stream\|events\|now-playing\)\$/);
});

test("stage 5 pairing screen renders explicit loading, cancelled, unavailable, and terminal stages in mockup order", async () => {
  const view = await readFile(new URL("../src/components/PairingView.tsx", import.meta.url), "utf8");
  const appSource = await readFile(new URL("../src/app.tsx", import.meta.url), "utf8");
  const harness = await readFile(new URL("./browser-harness.mjs", import.meta.url), "utf8");
  // Explicit local states: cancelled is a UI decision, unavailable is retryable.
  assert.match(view, /\| "cancelled"/);
  assert.match(view, /Подключение отменено/);
  assert.match(view, /Сервис недоступен/);
  assert.match(view, /Повторить попытку/);
  assert.match(appSource, /setPairingState\("cancelled"\)/);
  assert.match(appSource, /onCancel=\{cancelPairing\}/);
  assert.match(appSource, /onRetry=\{retryPairing\}/);
  // The link is never called invalid while data is still loading, and the login form
  // never appears while the session restore is in flight.
  assert.match(view, /Загружаем данные подключения/);
  assert.match(view, /Проверяем вашу сессию/);
  assert.match(view, /pairingState === "terminal" \? \([\s\S]*?Ссылка подключения недействительна[\s\S]*?\) : pairingState === "unavailable"/);
  assert.match(view, /pairingState === "unavailable" \? \([\s\S]*?!preview \? \(/);
  assert.match(view, /pairingState === "loading" \? \([\s\S]*?pairingState === "authenticated" \|\| pairingState === "approving"/);
  // Device facts per the device-link mockup, with the verification phrase highlighted.
  assert.match(view, /Проверьте, что это ваше устройство/);
  assert.match(view, /className="pairing-facts"/);
  assert.match(view, /className="pairing-phrase"/);
  assert.match(view, /className="pairing-code"/);
  assert.match(view, /Действует до/);
  assert.match(view, /Сверьте фразу и код с экраном/);
  // Double confirmation is impossible while the first request is in flight.
  assert.match(appSource, /const approveBusy = useRef\(false\)/);
  assert.match(appSource, /if \(approveBusy\.current\) return;/);
  // A server outage during approve keeps the request approvable; a dead link is terminal.
  assert.match(appSource, /code === "auth_unavailable" \|\| code === "server_unavailable"[\s\S]*"authenticated"[\s\S]*"terminal"/);
  assert.match(css, /\.pairing-phrase \{ color: var\(--accent-amber\)/);
  assert.match(css, /@keyframes pairingSpin/);
  assert.doesNotMatch(view + appSource, /approval_secret|\{preview\.request_id\}/);
  assert.match(harness, /--stage5/);
  assert.match(harness, /stage5-approve-fail/);
});

test("stage 6 renders honest device cards, QR empty state, and every Yandex Home state", async () => {
  const hud = await readFile(new URL("../src/components/HardwareHud.tsx", import.meta.url), "utf8");
  const yandex = await readFile(new URL("../src/components/YandexHomeCard.tsx", import.meta.url), "utf8");
  const centre = await readFile(new URL("../src/components/AccountCentre.tsx", import.meta.url), "utf8");
  const appSource = await readFile(new URL("../src/app.tsx", import.meta.url), "utf8");
  const harness = await readFile(new URL("./browser-harness.mjs", import.meta.url), "utf8");
  // Device cards: real product type, session-derived status, guarded dates, disabled-while-busy.
  assert.match(hud, /device-product-tag[\s\S]*deviceProductName\(device\.device_type\)/);
  assert.match(hud, /isActive \? "В сети" : "Офлайн"/);
  assert.match(hud, /device\.session_status === "active"/);
  assert.match(hud, /device\.last_seen_at \? \(/);
  assert.match(hud, /meta-unknown">нет данных/);
  assert.equal((hud.match(/disabled=\{busy\}/g) ?? []).length, 2);
  // Empty list explains the QR path instead of faking an online device.
  assert.match(hud, /Подключённых устройств пока нет/);
  assert.match(hud, /покажет QR-код/);
  assert.match(hud, /Отсканируйте QR-код камерой телефона/);
  assert.match(hud, /Сверьте фразу и код с экраном устройства/);
  assert.match(hud, /Браузер подтверждает устройство, но не является RockMobile или RockCast/);
  // Yandex Home states per the mockup: each outcome has its own copy and action.
  assert.match(yandex, /type YandexView = "loading" \| "ready" \| "empty" \| "error" \| "expired"/);
  assert.match(yandex, /Обновляем показания…/);
  assert.match(yandex, /Датчиков с доступными показаниями не найдено\./);
  assert.match(yandex, /Не удалось получить данные Яндекс Дома\./);
  assert.match(yandex, /Доступ к Яндекс Дому истёк\. Подключите его снова\./);
  assert.match(yandex, /Подключите аккаунт Яндекса, чтобы видеть показания датчиков/);
  assert.match(yandex, /"Подключить снова"/);
  assert.match(yandex, /Повторить/);
  assert.match(yandex, /Обновить показания/);
  assert.match(yandex, /aria-live="polite"/);
  assert.match(yandex, /yh-state-badge/);
  // Sensors keep rooms, units only when reported, per-property times; no zero placeholders.
  assert.match(yandex, /const propertyValue = \(prop/);
  assert.match(yandex, /prop\.unit\?\.trim\(\) \?\? ""/);
  assert.match(yandex, /prop\.formatted_value\.includes\(unit\)/);
  assert.match(yandex, /prop\.updated_at && \(/);
  assert.match(yandex, /sensor-prop-time/);
  assert.match(yandex, /device\.properties\.length \? \(/);
  assert.match(yandex, /Доступных показаний нет/);
  assert.match(yandex, /device\.room_name && \(/);
  // Existing OAuth redirect, confirmed disconnect, and refresh stay wired.
  assert.match(yandex, /location\.assign\(\(await api\.yandexHomeAuthorization\(csrf\)\)\.authorization_url\)/);
  assert.match(yandex, /window\.confirm\(\s*"Отключить Яндекс Дом\? RockServer перестанет видеть показания датчиков\."/);
  assert.match(yandex, /api\.disconnectYandexHome\(csrf\)/);
  // QR success lands on the devices tab; section header matches the cabinet style.
  assert.match(appSource, /initialTab=\{justConnected \? "devices" : "stations"\}/);
  assert.match(centre, /initialTab = "stations"/);
  assert.match(centre, /useState<NavTab>\(initialTab\)/);
  assert.match(centre, /deck-panel devices-banner/);
  assert.match(centre, /Аккаунт и подключения/);
  assert.match(centre, /devices-title">Устройства</);
  assert.match(centre, /RockCast, RockMobile и Яндекс Дом/);
  // Account gate screens keep the four states and their texts in cabinet styling.
  assert.match(centre, /Загружаем аккаунт…/);
  assert.match(centre, /Сервис временно недоступен/);
  assert.match(centre, /Сессия браузера завершена/);
  assert.match(centre, /Вы не вошли/);
  assert.match(centre, /Создать Rock-аккаунт/);
  assert.match(centre, /AccountGate/);
  assert.match(css, /\.account-gate \{/);
  assert.match(css, /\.devices-banner \{/);
  assert.match(css, /\.devices-empty \{/);
  assert.match(css, /\.yh-state-badge/);
  assert.match(css, /\.sensor-prop-time/);
  assert.match(css, /min-height: 44px/);
  // Harness fixtures cover the full state matrix without real OAuth or live sensors.
  for (const flag of ["--stage6", "--stage6-empty", "--stage6-yandex-off", "--stage6-yandex-empty", "--stage6-yandex-error", "--stage6-yandex-expired", "--stage6-slow", "--stage6-device-fail", "--stage6-yandex-down"])
    assert.match(harness, new RegExp(flag));
  assert.match(harness, /yandex_home_reconnect_required/);
  assert.match(harness, /#\$\{deviceCalls\} for \$\{deviceMatch\[1\]\}/);
});

test("stage 8 defaults to the table view and pages the catalog one request at a time", async () => {
  const view = await readFile(new URL("../src/components/StationsView.tsx", import.meta.url), "utf8");
  const centre = await readFile(new URL("../src/components/AccountCentre.tsx", import.meta.url), "utf8");
  const pagesHook = await readFile(new URL("../src/useStationPages.ts", import.meta.url), "utf8");
  const paging = await readFile(new URL("../src/paging.ts", import.meta.url), "utf8");
  const footer = await readFile(new URL("../src/components/ListFooter.tsx", import.meta.url), "utf8");
  const harness = await readFile(new URL("./browser-harness.mjs", import.meta.url), "utf8");
  // Table is the default catalog presentation and the choice is owned by the cabinet,
  // so it survives section switches.
  assert.match(centre, /useState<"grid" \| "table">\("table"\)/);
  assert.match(centre, /onViewModeChange=\{setViewMode\}/);
  // One page request at a time; a ref guard covers the gap before the state update.
  assert.match(pagesHook, /const busy = useRef\(false\)/);
  assert.match(pagesHook, /current\.loading \|\| current\.loadingMore \|\| !current\.hasMore \|\| busy\.current/);
  // Late responses of a superseded issuance are dropped by the generation counter.
  assert.match(pagesHook, /generation\.current \+= 1/);
  assert.match(pagesHook, /if \(gen !== generation\.current\) return;/);
  // Next offset counts server page sizes; duplicate rows merge by station ID (paging.mjs).
  assert.match(paging, /const serverOffset = previousOffset \+ page\.length/);
  assert.match(paging, /new Set\(current\.map\(\(station\) => station\.id\)\)/);
  assert.match(paging, /MAX_SEARCH_OFFSET = 10_000/);
  assert.match(paging, /if \(page\.length === 0\) hasMore = false/);
  // The server offset boundary is stated, not silently lifted.
  assert.match(footer, /Достигнут предел постраничного поиска/);
  // Footer exposes sentinel preloading, loading, retry, manual load, and end states.
  assert.match(footer, /IntersectionObserver/);
  assert.match(footer, /rootMargin: "600px 0px"/);
  assert.match(footer, /Загрузка станций…/);
  assert.match(footer, /Повторить/);
  assert.match(footer, /Загрузить ещё/);
  assert.match(footer, /Показаны все/);
  assert.match(view, /<ListFooter paging=\{paging\} \/>/);
  assert.match(centre, /onLoadMore: pages\.loadMore/);
  // Personal tabs reveal already-synced records in slices without catalog page
  // requests: the paging hook runs once for the catalog issuance only.
  assert.equal((centre.match(/useStationPages\(/g) ?? []).length, 1);
  assert.match(centre, /PERSONAL_PAGE_SIZE = 24/);
  assert.match(centre, /filteredPersonal\.slice\(0, personalVisible\)/);
  assert.match(centre, /setPersonalVisible\(\(visible\) => visible \+ PERSONAL_PAGE_SIZE\)/);
  assert.match(centre, /loadingMore: false,\s*\n\s*error: "",/);
  // Counters describe the full list, not only the shown slice.
  assert.match(view, /paging\.total \?\? stations\.length/);
  // Harness fixtures page a 64-station pool with drift duplicates, a first
  // next-page failure, slow pages for mid-flight query changes, and synced
  // favourites/history for progressive local reveal.
  for (const flag of ["--stage8", "--stage8-page-fail", "--stage8-slow"]) assert.match(harness, new RegExp(flag));
  assert.match(harness, /\[stage8\] search/);
  assert.match(harness, /has_more/);
});

test("stage 9 voice search is an explicit mic button beside the single search with anonymous same-origin streaming", async () => {
  const header = await readFile(new URL("../src/components/Header.tsx", import.meta.url), "utf8");
  const centre = await readFile(new URL("../src/components/AccountCentre.tsx", import.meta.url), "utf8");
  const view = await readFile(new URL("../src/components/StationsView.tsx", import.meta.url), "utf8");
  const voice = await readFile(new URL("../src/voice.ts", import.meta.url), "utf8");
  const pcm = await readFile(new URL("../src/voicePcm.ts", import.meta.url), "utf8");
  const browser = await readFile(new URL("../src/voiceBrowser.ts", import.meta.url), "utf8");
  const hook = await readFile(new URL("../src/useVoiceSearch.ts", import.meta.url), "utf8");
  const fixture = await readFile(new URL("../src/voiceFixture.ts", import.meta.url), "utf8");
  const harness = await readFile(new URL("./browser-harness.mjs", import.meta.url), "utf8");
  const appSource = await readFile(new URL("../src/app.tsx", import.meta.url), "utf8");
  // One search box remains; the mic trigger is an explicit labelled button.
  assert.match(header, /voice-mic-btn/);
  assert.match(header, /aria-label=\{voiceActive \? "Отменить голосовой поиск" : "Голосовой поиск станции"\}/);
  assert.doesNotMatch(header, /disabled=\{voiceBusy\}/);
  const panel = await readFile(new URL("../src/components/VoiceSearchPanel.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(panel, /onFinish|onCancel|Завершить/);
  assert.match(centre, /onVoiceSearch=\{voice.toggle\}/);
  assert.match(header, /onVoiceSearch/);
  assert.equal((header.match(/type="search"/g) ?? []).length, 1);
  // Protocol: PCM s16le 16 kHz frames, buffered_v1, limit capped at 10, cancel
  // frame, exactly one terminal, and no result applied after cancel.
  assert.match(voice, /sample_rate_hz: VOICE_STREAM_LIMITS\.sampleRateHz/);
  assert.match(voice, /recognizer_mode: "buffered_v1"/);
  assert.match(voice, /Math\.min\(Math\.max\(options\.limit \?\? 10, 1\), 10\)/);
  assert.match(voice, /type: "cancel"/);
  assert.match(voice, /terminalSeen/);
  assert.match(voice, /if \(this\.cancelled\) return;/);
  // Real anti-aliased rate conversion, not naive decimation and not MediaRecorder.
  assert.match(pcm, /blackman/);
  assert.match(pcm, /sinc\(/);
  assert.match(pcm, /PcmDownsampler/);
  // Anonymous same-origin WebSocket: no Authorization header, no device or
  // deployment tokens anywhere in the voice pipeline.
  assert.match(browser, /\/api\/v1\/voice\/stream/);
  assert.match(browser, /new WebSocket\(/);
  assert.match(browser, /PcmDownsampler/);
  assert.doesNotMatch(browser + voice + hook + fixture, /new MediaRecorder|MediaRecorder\./);
  // No credential usage (comments may explain why they are absent).
  assert.doesNotMatch(browser + voice + hook + fixture, /setRequestHeader|Bearer |deployment_token|device_secret|access_token/);
  // Lifecycle: mic, audio graph, and socket are released on unmount and pagehide.
  assert.match(hook, /pagehide/);
  assert.match(hook, /sessionRef\.current\?\.dispose\(\)/);
  assert.match(voice, /close\(\)/);
  // Recording pauses the player and restores only what it paused itself.
  assert.match(centre, /wasPlayingBeforeVoiceRef\.current = isPlaying;/);
  assert.match(centre, /currentStation\?\.id === stationIdBeforeVoiceRef\.current/);
  assert.match(centre, /setIsPlaying\(false\)/);
  assert.match(centre, /setIsPlaying\(true\)/);
  // RockCast flow: show voice candidates while a cleaned transcript starts the
  // normal paged search, then expose the same load-more path as text search.
  assert.match(centre, /cleanVoiceQuery\(voice\.transcript\)/);
  assert.match(centre, /useStationPages\(voiceQuery \|\| activeSearch/);
  assert.match(centre, /voicePreview \? voice\.stations : pages\.stations/);
  assert.match(centre, /hasMore: pages\.hasMore/);
  assert.match(centre, /voiceQuery=\{voiceResultActive \? voice\.transcript : ""\}/);
  assert.match(view, /Голосовой запрос: «\$\{voiceQuery\}»/);
  assert.match(view, /По голосовому запросу ничего не найдено/);
  assert.match(view, /воспроизведение не запускается автоматически/);
  // Fixture seam is URL-parameter-gated and credential-free.
  assert.match(fixture, /voice-fixture/);
  assert.match(appSource, /maybeInstallVoiceFixture\(\)/);
  assert.match(fixture, /NotAllowedError/);
  assert.match(fixture, /NotFoundError/);
  // Harness serves the worklet and a validating voice WebSocket fixture for
  // every outcome family.
  assert.match(harness, /voice-worklet\.js/);
  assert.match(harness, /Sec-WebSocket-Accept/);
  for (const flag of ["--stage9", "--stage9-silence", "--stage9-empty", "--stage9-timeout", "--stage9-provider", "--stage9-429", "--stage9-drop", "--stage9-late"])
    assert.match(harness, new RegExp(flag));
  assert.match(harness, /speech_not_recognized/);
  assert.match(harness, /audio before ready/);
});
