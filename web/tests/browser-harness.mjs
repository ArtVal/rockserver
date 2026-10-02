import { createServer } from "node:http";
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
createServer(async (request, response) => {
  const url = request.url ?? "";
  const stationMatch = url.match(/^\/api\/v1\/stations\/([^/]+)\/(stream|events|now-playing)$/);
  const deviceMatch = url.match(/^\/api\/v1\/browser\/devices\/([^/?]+)$/);
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
  response.writeHead(200, { "content-type": "text/html" }); response.end(index);
}).listen(port, "127.0.0.1");
