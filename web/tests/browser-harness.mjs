import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const port = Number(process.argv[2] ?? 5174);
const authenticated = process.argv.includes("--authenticated");
const stage3 = process.argv.includes("--stage3");
const stage4 = process.argv.includes("--stage4");
const stage5Expired = process.argv.includes("--stage5-expired");
const stage5Down = process.argv.includes("--stage5-down");
const stage5ApproveFail = process.argv.includes("--stage5-approve-fail");
// Stage 5 fixtures slow the pairing lookup, session restore, and approve calls so every
// intermediate UI state stays observable in a browser; approve hits are counted to prove
// a double click still produces exactly one confirmation request.
const stage5 = stage5Expired || stage5Down || stage5ApproveFail || process.argv.includes("--stage5");
// Stage 6 fixtures drive the devices section: a mutable device list, Yandex Home states,
// and counted device mutations. All data is synthetic and clearly labelled "fixture".
const stage6 = process.argv.includes("--stage6");
const stage6EmptyDevices = process.argv.includes("--stage6-empty");
const stage6YandexOff = process.argv.includes("--stage6-yandex-off");
const stage6YandexEmpty = process.argv.includes("--stage6-yandex-empty");
const stage6YandexError = process.argv.includes("--stage6-yandex-error");
const stage6YandexExpired = process.argv.includes("--stage6-yandex-expired");
const stage6Slow = process.argv.includes("--stage6-slow");
const stage6DeviceFail = process.argv.includes("--stage6-device-fail");
const stage6YandexDown = process.argv.includes("--stage6-yandex-down");
// Stage 8 fixtures drive paginated search: a 64-station pool served in 20-row
// pages whose first rows repeat the previous page (ranking drift duplicates), a
// counted first next-page failure, slow pages for mid-flight query changes, and
// a synced favourites/history snapshot for progressive local reveal. Every
// search request is logged so a QA pass can verify one request per page and no
// catalog paging while scrolling personal lists.
// Stage 9 adds a minimal RFC 6455 WebSocket voice fixture on
// /api/v1/voice/stream: it validates the start→ready→audio→commit/cancel
// order and chunk limits, logs protocol violations, and injects every voice
// outcome (silence, empty result, timeouts, provider failure, HTTP 429
// upgrade rejection, mid-recording disconnect, duplicate late result). Stage 9
// implies the stage 8 cabinet fixtures; client-side microphone behaviour is
// driven by the page URL parameter `voice-fixture=tone|silence|denied|…`.
const stage9 = process.argv.includes("--stage9");
const stage9Silence = process.argv.includes("--stage9-silence");
const stage9Empty = process.argv.includes("--stage9-empty");
const stage9Timeout = process.argv.includes("--stage9-timeout");
const stage9Provider = process.argv.includes("--stage9-provider");
const stage9RateLimited = process.argv.includes("--stage9-429");
const stage9Drop = process.argv.includes("--stage9-drop");
const stage9Late = process.argv.includes("--stage9-late");
const stage8 = process.argv.includes("--stage8") || stage9;
const stage8PageFail = process.argv.includes("--stage8-page-fail");
const stage8Slow = process.argv.includes("--stage8-slow");
let stage8SearchCalls = 0;
let stage8PageFailures = 0;
// Pool is derived per query so a mid-flight query change is observable: any row
// from a superseded issuance would carry the old query in its name.
const stage8Pools = new Map();
const stage8PoolFor = (query) => {
  const key = (query || "rock").slice(0, 40);
  if (!stage8Pools.has(key)) {
    stage8Pools.set(key, Array.from({ length: 64 }, (_, i) => ({
      id: `st-${String(i + 1).padStart(2, "0")}`,
      name: `${key} Radio ${String(i + 1).padStart(2, "0")}`,
      tags: [key], country_code: "RU", codec: "AAC", bitrate_kbps: 128,
    })));
  }
  return stage8Pools.get(key);
};
const stage8Favourites = Array.from({ length: 26 }, (_, i) => ({
  record_id: `fav-${i + 1}`, station_id: `fav-st-${i + 1}`,
  added_at: `2026-09-${String((i % 28) + 1).padStart(2, "0")}T10:00:00Z`,
  updated_at: `2026-09-${String((i % 28) + 1).padStart(2, "0")}T10:00:00Z`,
}));
const stage8FavouriteStations = stage8Favourites.map((record, i) => ({
  id: record.station_id, name: `Избранная волна ${i + 1}`, tags: ["rock"], country_code: "RU",
}));
const stage8History = Array.from({ length: 30 }, (_, i) => ({
  record_id: `hist-${i + 1}`, station_id: `hist-st-${i + 1}`,
  started_at: "2026-09-01T10:00:00Z",
  last_played_at: `2026-10-0${(i % 9) + 1}T1${i % 10}:00:00Z`,
  updated_at: `2026-10-0${(i % 9) + 1}T1${i % 10}:00:00Z`,
  metadata: { name: `Историческая волна ${i + 1}` },
}));
let approveCalls = 0;
let searchFailures = 0;
let deviceCalls = 0;
let yandexDisconnectCalls = 0;
const stage6State = {
  yandexConnected: !stage6YandexOff,
  devices: stage6EmptyDevices ? [] : [
    { device_id: "dev-desktop", device_display_name: "Основной компьютер", device_type: "rockcast_windows", connected_at: "2026-09-20T10:12:00Z", last_seen_at: "2026-10-02T09:30:00Z", session_status: "active" },
    { device_id: "dev-phone", device_display_name: "Телефон в коридоре", device_type: "rockmobile_android", connected_at: "2026-09-28T18:05:00Z", session_status: "inactive" },
  ],
};
const stage6Sensors = { devices: [
  { id: "y-climate", name: "Датчик климата", device_type: "devices.types.sensor.climate", room_name: "Гостиная", properties: [
    { property_type: "devices.properties.float", instance: "temperature", name: "Температура", value: 22.4, unit: "°C", formatted_value: "22,4", updated_at: "2026-10-02T09:40:00Z" },
    { property_type: "devices.properties.float", instance: "humidity", name: "Влажность", value: 46, unit: "%", formatted_value: "46", updated_at: "2026-10-02T09:41:00Z" },
  ] },
  { id: "y-co2", name: "Датчик CO₂", room_name: "Кухня", properties: [
    { property_type: "devices.properties.float", instance: "co2_level", name: "Углекислый газ", value: 621, unit: "ppm", formatted_value: "621" },
  ] },
  { id: "y-socket", name: "Розетка у окна", properties: [] },
], sensors: [] };
const index = await readFile(resolve("dist/index.html"));
const preview = JSON.stringify({ request_id: "00000000-0000-0000-0000-000000000000", device_display_name: "Этот телефон", device_type: "rockmobile_android", verification_phrase: "AMBER FJORD", short_code: "A1B2C3D4", expires_at: "2030-01-01T12:00:00Z", status: "pending" });

/** Minimal 8 kHz mono 16-bit WAV header with a huge declared size, so the response drips like an endless radio stream. */
const wavHeader = () => {
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(0x7ffff000, 4);
  header.write("WAVE", 8);
  header.write("fmt ", 12);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(8000, 24);
  header.writeUInt32LE(16000, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(0x7fffef00, 40);
  return header;
};

// Deterministic player fixtures: station "one" streams immediately, "two" stalls first (buffering state),
// "three" answers with a server error (stream error + retry path).
const nowPlayingTitles = { one: "Сталь и Туман — Проверочный трек", two: null, three: "No Metadata FM — прямой эфир" };

const delayed = (ms) => new Promise(resolve => setTimeout(resolve, ms));
const jsonError = (code) => JSON.stringify({ code, message: "fixture", request_id: "fixture", details: {} });


/** Serves built UI assets with deterministic, credential-free API responses for browser QA. */
const server = createServer(async (request, response) => {
  const url = request.url ?? "";
  const stationMatch = url.match(/^\/api\/v1\/stations\/([^/]+)\/(stream|events|now-playing)$/);
  const deviceMatch = url.match(/^\/api\/v1\/browser\/devices\/([^/?]+)$/);
  if (stage8 && url === "/api/v1/auth/browser-session") {
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    return response.end(JSON.stringify({ account_display_name: "Алексей", csrf_token: "test-csrf" }));
  }
  if (stage8 && url === "/api/v1/browser/account") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ account_display_name: "Алексей", device_limit: 5, devices: [], yandex_home_connected: false }));
  }
  if (stage8 && url === "/api/v1/browser/sync") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({
      server_revision: 1, server_time: new Date().toISOString(),
      favourites: { records: stage8Favourites },
      history: { records: stage8History },
      stations: stage8FavouriteStations,
    }));
  }
  if (stage8 && url === "/api/v1/search") {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const { query, limit, offset } = JSON.parse(Buffer.concat(chunks).toString());
    stage8SearchCalls += 1;
    console.log(`[stage8] search #${stage8SearchCalls} "${query}" offset=${offset} limit=${limit}`);
    if (stage8Slow) await delayed(1200);
    if (offset > 0 && stage8PageFail && stage8PageFailures++ === 0) {
      response.writeHead(503, { "content-type": "application/json" });
      return response.end(jsonError("server_unavailable"));
    }
    const pageOffset = Math.max(0, Number(offset) || 0);
    const pageSize = Math.max(1, Math.min(Number(limit) || 20, 20));
    const pool = stage8PoolFor(query);
    const page = pool.slice(pageOffset, pageOffset + pageSize);
    // Simulate ranking drift: pages after the first repeat the two previous rows.
    if (pageOffset > 0 && page.length > 2) {
      page[0] = pool[pageOffset - 2];
      page[1] = pool[pageOffset - 1];
    }
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    return response.end(JSON.stringify({
      request_id: "fixture", stations: page,
      total: pool.length,
      has_more: pageOffset + page.length < pool.length,
    }));
  }
  if (stage6 && url === "/api/v1/browser/account") {
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    return response.end(JSON.stringify({ account_display_name: "Алексей", device_limit: 5, devices: stage6State.devices, yandex_home_connected: stage6State.yandexConnected }));
  }
  if (stage6 && url === "/api/v1/browser/sync") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ server_revision: 1, server_time: new Date().toISOString(), favourites: { records: [] }, history: { records: [] }, stations: [] }));
  }
  if (stage6 && url === "/api/v1/search") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ request_id: "fixture", stations: [
      { id: "one", name: "Rock FM", tags: ["rock"], country_code: "RU", codec: "AAC", bitrate_kbps: 128 },
      { id: "two", name: "ГУСЬ.Рок", tags: ["rock"], country_code: "RU" },
    ] }));
  }
  if (stage6 && deviceMatch && (request.method === "PATCH" || request.method === "DELETE")) {
    deviceCalls += 1;
    console.log(`[stage6] device ${request.method} #${deviceCalls} for ${deviceMatch[1]}`);
    await delayed(600);
    if (request.headers["x-csrf-token"] !== "test-csrf") {
      response.writeHead(403, { "content-type": "application/json" });
      return response.end(jsonError("csrf_required"));
    }
    if (stage6DeviceFail) {
      response.writeHead(503, { "content-type": "application/json" });
      return response.end(jsonError("server_unavailable"));
    }
    if (request.method === "DELETE") stage6State.devices = stage6State.devices.filter(d => d.device_id !== deviceMatch[1]);
    else {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      const name = JSON.parse(Buffer.concat(chunks).toString()).device_display_name;
      const device = stage6State.devices.find(d => d.device_id === deviceMatch[1]);
      if (device) device.device_display_name = name;
    }
    response.writeHead(204);
    return response.end();
  }
  if (stage6 && url === "/api/v1/browser/yandex-home/sensors") {
    if (stage6Slow) await delayed(2000);
    if (stage6YandexError) {
      response.writeHead(502, { "content-type": "application/json" });
      return response.end(jsonError("yandex_home_unavailable"));
    }
    if (stage6YandexExpired) {
      // Mirrors the server: a dead token is revoked before the reconnect-required error.
      stage6State.yandexConnected = false;
      response.writeHead(409, { "content-type": "application/json" });
      return response.end(jsonError("yandex_home_reconnect_required"));
    }
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    return response.end(JSON.stringify(stage6YandexEmpty ? { devices: [], sensors: [] } : stage6Sensors));
  }
  if (stage6 && request.method === "POST" && url === "/api/v1/browser/yandex-home/authorize") {
    if (request.headers["x-csrf-token"] !== "test-csrf") {
      response.writeHead(403, { "content-type": "application/json" });
      return response.end(jsonError("csrf_required"));
    }
    stage6State.yandexConnected = true;
    response.writeHead(200, { "content-type": "application/json" });
    // Simulates the real OAuth round trip: an outside URL that lands back on the cabinet.
    return response.end(JSON.stringify({ authorization_url: `http://127.0.0.1:${port}/?yandex_home=connected` }));
  }
  if (stage6 && request.method === "DELETE" && url === "/api/v1/browser/yandex-home") {
    yandexDisconnectCalls += 1;
    console.log(`[stage6] yandex disconnect #${yandexDisconnectCalls}`);
    await delayed(600);
    if (request.headers["x-csrf-token"] !== "test-csrf") {
      response.writeHead(403, { "content-type": "application/json" });
      return response.end(jsonError("csrf_required"));
    }
    if (stage6YandexDown) {
      response.writeHead(503, { "content-type": "application/json" });
      return response.end(jsonError("server_unavailable"));
    }
    stage6State.yandexConnected = false;
    response.writeHead(204);
    return response.end();
  }
  if (stage3 && url === "/api/v1/browser/account") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ account_display_name: "Тест", device_limit: 5, devices: [], yandex_home_connected: false }));
  }
  if (stage3 && url === "/api/v1/browser/sync") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ server_revision: 1, server_time: new Date().toISOString(), favourites: { records: [] }, history: { records: [] }, stations: [] }));
  }
  if (stage3 && url === "/api/v1/search") {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const query = JSON.parse(Buffer.concat(chunks).toString()).query;
    const stations = query === "empty" ? [] : query === "error" && searchFailures++ === 0 ? null : [
      { id: "one", name: "Rock FM", tags: [query], country_code: "RU", codec: "AAC", bitrate_kbps: 127.62 },
      { id: "two", name: "Rock FM", tags: [], language: "en" },
      { id: "three", name: "No Metadata FM", tags: [] },
    ];
    response.writeHead(stations ? 200 : 503, { "content-type": "application/json" });
    return response.end(JSON.stringify(stations ? { request_id: "fixture", stations } : { code: "server_unavailable", message: "", request_id: "fixture", details: {} }));
  }
  if (stage4 && stationMatch && stationMatch[2] === "stream") {
    const id = stationMatch[1];
    if (id === "three") {
      response.writeHead(503, { "content-type": "application/json" });
      return response.end('{"code":"upstream_unavailable","message":"fixture stream failure","request_id":"fixture","details":{}}');
    }
    response.writeHead(200, { "content-type": "audio/wav", "cache-control": "no-store" });
    const slow = id === "two";
    const silence = Buffer.alloc(8000);
    response.write(wavHeader());
    const push = (times = 1) => {
      if (response.destroyed || response.writableEnded) return false;
      for (let i = 0; i < times; i += 1) response.write(silence);
      return true;
    };
    // Chromium holds a chunked response without Content-Length until roughly 512 KB of media
    // data is buffered, so open with a large burst (~40 s of silence) and keep dripping afterwards.
    // Station "two" delays the burst to keep the buffering state observable.
    const initial = setTimeout(() => push(80), slow ? 2000 : 50);
    const drip = setInterval(() => {
      if (!push()) clearInterval(drip);
    }, 400);
    request.socket.on("close", () => {
      clearTimeout(initial);
      clearInterval(drip);
    });
    return;
  }
  if (stage4 && stationMatch && stationMatch[2] === "events") {
    response.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store", connection: "keep-alive" });
    let counter = 1;
    const send = () => {
      if (response.destroyed || response.writableEnded) return false;
      response.write(`event: snapshot\ndata: ${JSON.stringify({ station_id: stationMatch[1], state: "fresh", rawTitle: `Живые метаданные — трек №${counter++}` })}\n\n`);
      return true;
    };
    send();
    const beat = setInterval(() => {
      if (!send()) clearInterval(beat);
    }, 2000);
    request.socket.on("close", () => clearInterval(beat));
    return;
  }
  if (stage4 && stationMatch && stationMatch[2] === "now-playing") {
    const id = stationMatch[1];
    const title = nowPlayingTitles[id];
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    return response.end(JSON.stringify({ station_id: id, state: title ? "fresh" : "missing", rawTitle: title, updated_at: new Date().toISOString() }));
  }
  if (stage5 && request.method === "POST" && /^\/api\/v1\/pairing-requests\/[^/]+\/approve$/.test(url)) {
    approveCalls += 1;
    console.log(`[stage5] approve request #${approveCalls}`);
    await delayed(1000);
    if (stage5ApproveFail) {
      response.writeHead(409, { "content-type": "application/json" });
      return response.end(jsonError("pairing_not_approvable"));
    }
    if (request.headers["x-csrf-token"] !== "test-csrf") {
      response.writeHead(403, { "content-type": "application/json" });
      return response.end(jsonError("csrf_required"));
    }
    // With stage 6 fixtures the approved device joins the account list, so the cabinet
    // handoff can show the refreshed device list and the "just connected" marker.
    if (stage6 && !stage6State.devices.some(d => d.device_id === "dev-paired")) {
      stage6State.devices = [...stage6State.devices, {
        device_id: "dev-paired",
        device_display_name: JSON.parse(preview).device_display_name,
        device_type: JSON.parse(preview).device_type,
        connected_at: new Date().toISOString(),
        session_status: "active",
      }];
    }
    response.writeHead(204);
    return response.end();
  }
  if (url.startsWith("/api/v1/pairing-requests/lookup")) {
    if (stage5) {
      await delayed(stage5Down ? 600 : 1200);
      if (stage5Expired) {
        response.writeHead(404, { "content-type": "application/json" });
        return response.end(jsonError("pairing_not_found"));
      }
      if (stage5Down) {
        response.writeHead(503, { "content-type": "application/json" });
        return response.end(jsonError("server_unavailable"));
      }
    }
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" }); return response.end(preview);
  }
  if (url === "/api/v1/auth/browser-session") {
    if (stage5) await delayed(2500);
    if (!authenticated) { response.writeHead(401, { "content-type": "application/json" }); return response.end('{"code":"authentication_required","message":"","request_id":"","details":{}}'); }
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" }); return response.end('{"account_display_name":"Алексей","csrf_token":"test-csrf"}');
  }
  if (url.startsWith("/assets/")) {
    try {
      const asset = await readFile(resolve(`dist${url}`));
      response.writeHead(200, { "content-type": url.endsWith(".js") ? "text/javascript" : "text/css" });
      return response.end(asset);
    } catch { response.writeHead(404); return response.end(); }
  }
  if (url === "/voice-worklet.js") {
    try {
      const asset = await readFile(resolve("dist/voice-worklet.js"));
      response.writeHead(200, { "content-type": "text/javascript" });
      return response.end(asset);
    } catch { response.writeHead(404); return response.end(); }
  }
  response.writeHead(200, { "content-type": "text/html" }); response.end(index);
});

// ---- Stage 9: minimal RFC 6455 server for /api/v1/voice/stream ----
// Only what the voice client needs: handshake, masked client-frame parsing,
// unmasked text/close/pong replies. Protocol violations are logged so a QA
// pass can assert the real client never commits them.
const VOICE_WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
let stage9Sessions = 0;

const wsFrame = (payload, opcode = 1) => {
  const length = payload.length;
  const header = length < 126
    ? Buffer.from([0x80 | opcode, length])
    : length < 65536
    ? Buffer.from([0x80 | opcode, 126, length >> 8, length & 0xff])
    : Buffer.concat([Buffer.from([0x80 | opcode, 127]), (() => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(length)); return b; })()]);
  return Buffer.concat([header, payload]);
};

// The first candidate reuses the stage 4 playable station id "one" so a QA
// pass can verify real audio pause/restore around a voice recording.
const fixtureStations = (label, count) => Array.from({ length: count }, (_, i) => ({
  id: i === 0 ? "one" : `voice-st-${i + 1}`,
  name: `${label} радио ${i + 1}`,
  stream_url: `http://127.0.0.1:${port}/api/v1/stations/${i === 0 ? "one" : `voice-st-${i + 1}`}/stream`,
  homepage_url: "",
  favicon_url: "",
  tags: ["rock"],
  language: "ru",
  country_code: "RU",
  codec: "AAC",
  bitrate_kbps: 128,
  score: Math.round((0.95 - i * 0.1) * 100) / 100,
  reason: "fixture",
  health: "unknown",
}));

/** One deterministic voice fixture session over an upgraded socket. */
const startVoiceFixtureSession = (socket) => {
  stage9Sessions += 1;
  const sessionId = stage9Sessions;
  const requestId = `fixture-voice-${sessionId}`;
  const log = (message) => console.log(`[stage9] session#${sessionId} ${message}`);
  let buffer = Buffer.alloc(0);
  let ready = false;
  let committed = false;
  let cancelled = false;
  let finished = false;
  let audioBytes = 0;

  const sendText = (event) => socket.write(wsFrame(Buffer.from(JSON.stringify(event))));
  const finish = () => {
    if (finished) return;
    finished = true;
    setTimeout(() => {
      try { socket.write(wsFrame(Buffer.alloc(0), 8)); socket.destroy(); } catch { /* already gone */ }
    }, 50);
  };
  const sendError = (code, message) => {
    sendText({ type: "error", code, message, request_id: requestId, details: {} });
    finish();
  };
  const sendResult = (transcript, stations) => {
    sendText({
      type: "result",
      request_id: requestId,
      transcript,
      normalized_query: { original: transcript, locale: "ru-RU", terms: [], tags: [], language: "ru", country_code: null },
      selected_station: stations[0] ?? null,
      stations,
    });
  };

  const handleText = (payload) => {
    let event;
    try { event = JSON.parse(payload.toString("utf8")); } catch { log("violation: non-JSON text frame"); sendError("protocol_error", "fixture"); return; }
    if (event.type === "start") {
      if (ready) { log("violation: duplicate start"); sendError("protocol_error", "fixture"); return; }
      if (event.sample_rate_hz !== 16000) { log(`violation: sample_rate_hz=${event.sample_rate_hz}`); sendError("validation_failed", "fixture"); return; }
      if (!Number.isInteger(event.limit) || event.limit < 1 || event.limit > 10) { log(`violation: limit=${event.limit}`); sendError("validation_failed", "fixture"); return; }
      log(`start locale=${event.locale} limit=${event.limit} mode=${event.recognizer_mode ?? "buffered_v1"}`);
      sendText({ type: "ready", request_id: requestId, audio_format: "pcm_s16le", sample_rate_hz: 16000 });
      ready = true;
      if (stage9Provider) { log("provider unavailable injected right after ready"); sendError("speech_provider_unavailable", "fixture"); }
      return;
    }
    if (event.type === "commit") {
      if (!ready || committed || cancelled) { log("violation: commit out of order"); sendError("protocol_error", "fixture"); return; }
      committed = true;
      log(`commit audio_bytes=${audioBytes}`);
      if (stage9Silence) { sendError("speech_not_recognized", "fixture"); return; }
      if (stage9Timeout) { sendError("voice_timeout", "fixture"); return; }
      const transcript = stage9Empty ? "несуществующее радио" : "рок радиостанцию";
      if (!stage9Empty) sendText({ type: "transcript", request_id: requestId, transcript, is_final: true });
      sendResult(transcript, fixtureStations(stage9Empty ? "Пустое" : "Рок", stage9Empty ? 0 : 6));
      if (stage9Late) { log("late duplicate result injected"); sendResult("поздний дубль", fixtureStations("Дубль", 6)); }
      finish();
      return;
    }
    if (event.type === "cancel") {
      if (cancelled || !ready) { log("violation: cancel out of order"); sendError("protocol_error", "fixture"); return; }
      cancelled = true;
      log("cancel");
      sendError("cancelled", "fixture");
      return;
    }
    log(`violation: unexpected text event ${JSON.stringify(event.type)}`);
    sendError("protocol_error", "fixture");
  };

  const handleBinary = (payload) => {
    if (!ready) { log("violation: audio before ready"); sendError("protocol_error", "fixture"); return; }
    if (payload.length === 0 || payload.length % 2 !== 0 || payload.length > 32768) {
      log(`violation: chunk ${payload.length} bytes`);
      sendError("audio_chunk_invalid", "fixture");
      return;
    }
    audioBytes += payload.length;
    if (audioBytes > 2 * 1024 * 1024) { log("violation: session above 2 MiB"); sendError("audio_too_large", "fixture"); return; }
    if (stage9Drop && audioBytes > 32000) { log("drop connection mid-recording"); finished = true; socket.destroy(); }
  };

  socket.on("data", (chunk) => {
    if (finished) return;
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      if (buffer.length < 2) return;
      const opcode = buffer[0] & 0x0f;
      const masked = (buffer[1] & 0x80) !== 0;
      let length = buffer[1] & 0x7f;
      let offset = 2;
      if (length === 126) {
        if (buffer.length < 4) return;
        length = buffer.readUInt16BE(2);
        offset = 4;
      } else if (length === 127) {
        if (buffer.length < 10) return;
        length = Number(buffer.readBigUInt64BE(2));
        offset = 10;
      }
      const maskOffset = offset;
      if (masked) offset += 4;
      if (buffer.length < offset + length) return;
      const payload = Buffer.from(buffer.subarray(offset, offset + length));
      if (masked) {
        const mask = buffer.subarray(maskOffset, maskOffset + 4);
        for (let i = 0; i < payload.length; i += 1) payload[i] ^= mask[i % 4];
      }
      buffer = buffer.subarray(offset + length);
      if (opcode === 0x1) handleText(payload);
      else if (opcode === 0x2) handleBinary(payload);
      else if (opcode === 0x8) { log("client closed"); finished = true; socket.destroy(); return; }
      else if (opcode === 0x9) socket.write(wsFrame(payload, 0xa));
    }
  });
  socket.on("error", () => { finished = true; });
};

server.on("upgrade", (request, socket) => {
  const path = (request.url ?? "").split("?")[0];
  if (!stage9 || path !== "/api/v1/voice/stream") { socket.destroy(); return; }
  if (stage9RateLimited) {
    console.log("[stage9] voice upgrade rejected with HTTP 429");
    socket.write("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nConnection: close\r\n\r\n");
    socket.destroy();
    return;
  }
  const key = request.headers["sec-websocket-key"];
  if (!key) { socket.destroy(); return; }
  const accept = createHash("sha1").update(`${key}${VOICE_WS_GUID}`).digest("base64");
  socket.write(
    `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`
  );
  console.log(`[stage9] voice stream upgrade #${stage9Sessions + 1}`);
  startVoiceFixtureSession(socket);
});

server.listen(port, "127.0.0.1");
