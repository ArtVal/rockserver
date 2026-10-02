import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import ts from "typescript";

// voice.ts and voicePcm.ts only type-import from api.ts, so the transpiled
// modules run standalone: these tests execute the real session state machine
// and the real anti-aliased downsampler against fake sockets and microphones.

const loadModule = async (relativePath) => {
  const source = await readFile(new URL(relativePath, import.meta.url), "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};

const { VoiceSearchSession, VOICE_STREAM_LIMITS } = await loadModule("../src/voice.ts");
const { PcmDownsampler, VOICE_SAMPLE_RATE_HZ } = await loadModule("../src/voicePcm.ts");

/** WebSocket double: records everything the session sends, lets tests drive the server side. */
class FakeVoiceSocket {
  constructor() {
    this.sent = [];
    this.onopen = null;
    this.onmessage = null;
    this.onclose = null;
    this.onerror = null;
    this.closeCalled = false;
  }
  send(data) { this.sent.push(data); }
  close() { this.closeCalled = true; }
  serverOpen() { this.onopen?.(); }
  serverSend(event) { this.onmessage?.({ data: JSON.stringify(event) }); }
  serverClose() { this.onclose?.(); }
  get textFrames() { return this.sent.filter((data) => typeof data === "string").map((data) => JSON.parse(data)); }
  get binaryFrames() { return this.sent.filter((data) => typeof data !== "string"); }
  get binaryBytes() { return this.binaryFrames.reduce((total, frame) => total + frame.byteLength, 0); }
}

/** Microphone double with explicit close tracking. */
const fakeMicrophone = () => {
  const state = { closeCount: 0 };
  let emitChunk = () => undefined;
  const factory = async (onChunk) => {
    emitChunk = onChunk;
    return { close: () => { state.closeCount += 1; } };
  };
  return {
    state,
    factory,
    emit: (count) =>
      emitChunk(Int16Array.from({ length: count }, (_, index) => ((index * 37) % 1000) - 500)),
  };
};

const fakeTimers = () => {
  const timers = [];
  return {
    timers,
    setTimeoutFn: (handler, ms) => {
      timers.push({ handler, ms });
      return timers.length;
    },
  };
};

/** Drives a session through permission, socket open, and the ready event. */
const startedSession = async (options = {}) => {
  const mic = fakeMicrophone();
  const socket = new FakeVoiceSocket();
  const events = [];
  const clock = fakeTimers();
  const session = new VoiceSearchSession({
    socketFactory: () => socket,
    microphoneFactory: options.microphoneFactory ?? mic.factory,
    onEvent: (event) => events.push(event),
    setTimeoutFn: clock.setTimeoutFn,
  });
  await session.start();
  socket.serverOpen();
  socket.serverSend({ type: "ready", request_id: "r1", audio_format: "pcm_s16le", sample_rate_hz: 16000 });
  return { session, socket, mic, events, clock };
};

test("stage 9 start frame matches the /api/v1/voice/stream contract", async () => {
  const { socket, events } = await startedSession();
  assert.deepEqual(socket.textFrames, [
    { type: "start", locale: "ru-RU", sample_rate_hz: 16000, recognizer_mode: "buffered_v1", limit: 10 },
  ]);
  assert.ok(events.some((event) => event.type === "started"));
});

test("stage 9 audio captured before ready is buffered and sent as bounded even PCM frames", async () => {
  const mic = fakeMicrophone();
  const socket = new FakeVoiceSocket();
  const events = [];
  const session = new VoiceSearchSession({
    socketFactory: () => socket,
    microphoneFactory: mic.factory,
    onEvent: (event) => events.push(event),
    setTimeoutFn: fakeTimers().setTimeoutFn,
  });
  await session.start();
  socket.serverOpen();
  mic.emit(9000);
  assert.equal(socket.binaryFrames.length, 0);
  socket.serverSend({ type: "ready", request_id: "r1", audio_format: "pcm_s16le", sample_rate_hz: 16000 });
  assert.equal(socket.binaryFrames.length, 1);
  assert.equal(socket.binaryFrames[0].byteLength, 16000);
  mic.emit(7000);
  assert.equal(socket.binaryFrames.length, 2);
  assert.equal(socket.binaryBytes, 32000);
  for (const frame of socket.binaryFrames) assert.equal(frame.byteLength % 2, 0);
  assert.ok(socket.binaryBytes <= socket.binaryFrames.length * VOICE_STREAM_LIMITS.maxChunkBytes);
});

test("stage 9 finish stops the mic, flushes the partial tail, and commits once", async () => {
  const { session, socket, mic } = await startedSession();
  mic.emit(5000);
  assert.equal(socket.binaryFrames.length, 0);
  session.finish();
  assert.equal(mic.state.closeCount, 1);
  assert.equal(socket.binaryFrames.length, 1);
  assert.equal(socket.binaryFrames[0].byteLength, 10000);
  assert.deepEqual(socket.textFrames.at(-1), { type: "commit" });
  assert.equal(socket.textFrames.filter((frame) => frame.type === "commit").length, 1);
});

test("stage 9 result maps stations, releases resources, and ignores a duplicate terminal", async () => {
  const { session, socket, mic, events } = await startedSession();
  mic.emit(9000);
  session.finish();
  socket.serverSend({ type: "transcript", request_id: "r1", transcript: "рок радио", is_final: true });
  socket.serverSend({
    type: "result",
    request_id: "r1",
    transcript: "рок радио",
    stations: [
      { id: "st-1", name: "Рок радио 1", tags: ["rock"], codec: null, bitrate_kbps: null },
      { id: "", name: "invalid" },
      { id: "st-2", name: "Рок радио 2" },
    ],
  });
  const result = events.find((event) => event.type === "result");
  assert.equal(result.stations.length, 2);
  assert.equal(result.stations[0].name, "Рок радио 1");
  assert.deepEqual(result.stations[0].tags, ["rock"]);
  assert.equal(result.stations[0].codec, undefined);
  assert.deepEqual(result.stations[1].tags, []);
  assert.equal(mic.state.closeCount, 1);
  assert.ok(socket.closeCalled);
  const eventsAfterTerminal = events.length;
  socket.serverSend({ type: "result", request_id: "r1", transcript: "дубль", stations: [] });
  socket.serverSend({ type: "error", code: "internal_error", message: "late", request_id: "r1", details: {} });
  assert.equal(events.length, eventsAfterTerminal);
});

test("stage 9 cancel sends the cancel frame, applies nothing, and ignores the late result", async () => {
  const { session, socket, mic, events } = await startedSession();
  mic.emit(9000);
  const bytesBeforeCancel = socket.binaryBytes;
  session.cancel();
  assert.equal(mic.state.closeCount, 1);
  assert.deepEqual(socket.textFrames.at(-1), { type: "cancel" });
  assert.equal(socket.binaryBytes, bytesBeforeCancel);
  socket.serverSend({ type: "error", code: "cancelled", message: "fixture", request_id: "r1", details: {} });
  assert.equal(events.at(-1).type, "cancelled");
  assert.ok(socket.closeCalled);
  const eventsAfterCancel = events.length;
  socket.serverSend({ type: "result", request_id: "r1", transcript: "поздний", stations: [{ id: "x", name: "X" }] });
  assert.equal(events.length, eventsAfterCancel);
  assert.ok(!events.some((event) => event.type === "result"));
});

test("stage 9 cancel during recognition closes the socket immediately and stays silent", async () => {
  const { session, socket, mic, events } = await startedSession();
  mic.emit(9000);
  session.finish();
  session.cancel();
  assert.equal(events.at(-1).type, "cancelled");
  assert.ok(!socket.textFrames.some((frame) => frame.type === "cancel"));
  assert.ok(socket.closeCalled);
  assert.equal(mic.state.closeCount, 1);
  const eventsAfterCancel = events.length;
  socket.serverSend({ type: "result", request_id: "r1", transcript: "поздний", stations: [] });
  assert.equal(events.length, eventsAfterCancel);
});

test("stage 9 denied permission or a missing microphone never opens a socket", async () => {
  for (const [name, error, kind] of [
    ["denied", Object.assign(new Error("denied"), { name: "NotAllowedError" }), "permission"],
    ["missing", Object.assign(new Error("no device"), { name: "NotFoundError" }), "no_microphone"],
  ]) {
    let socketsCreated = 0;
    const events = [];
    const session = new VoiceSearchSession({
      socketFactory: () => { socketsCreated += 1; return new FakeVoiceSocket(); },
      microphoneFactory: async () => { throw error; },
      onEvent: (event) => events.push(event),
      setTimeoutFn: fakeTimers().setTimeoutFn,
    });
    await assert.rejects(() => session.start(), { name: "Error" }, name);
    const failure = events.find((event) => event.type === "error");
    assert.equal(failure.failure.kind, kind, name);
    assert.equal(socketsCreated, 0, name);
  }
});

test("stage 9 losing the socket before ready reports a connection failure and releases the mic", async () => {
  const mic = fakeMicrophone();
  const socket = new FakeVoiceSocket();
  const events = [];
  const session = new VoiceSearchSession({
    socketFactory: () => socket,
    microphoneFactory: mic.factory,
    onEvent: (event) => events.push(event),
    setTimeoutFn: fakeTimers().setTimeoutFn,
  });
  await session.start();
  socket.serverOpen();
  socket.serverClose();
  const failure = events.find((event) => event.type === "error");
  assert.equal(failure.failure.kind, "connection");
  assert.equal(mic.state.closeCount, 1);
});

test("stage 9 terminal server error codes map to distinct user-facing failures", async () => {
  for (const [code, kind] of [
    ["speech_not_recognized", "not_recognized"],
    ["speech_provider_unavailable", "provider"],
    ["speech_provider_error", "provider"],
    ["voice_timeout", "timeout"],
    ["search_timeout", "timeout"],
    ["internal_error", "server"],
  ]) {
    const session = await startedSession();
    session.socket.serverSend({ type: "error", code, message: "fixture", request_id: "r1", details: {} });
    const failure = session.events.find((event) => event.type === "error");
    assert.equal(failure.failure.kind, kind, code);
    assert.ok(session.socket.closeCalled, code);
    assert.equal(session.mic.state.closeCount, 1, code);
    assert.ok(failure.failure.message.length > 0, code);
  }
});

test("stage 9 the 60-second audio cap auto-commits and never exceeds the server byte budget", async () => {
  const { session, socket, mic } = await startedSession();
  const totalSamples = 960000;
  const chunk = 40000;
  for (let sent = 0; sent < totalSamples; sent += chunk) mic.emit(chunk);
  assert.deepEqual(socket.textFrames.at(-1), { type: "commit" });
  assert.equal(socket.binaryBytes, totalSamples * 2);
  assert.equal(mic.state.closeCount, 1);
  const bytesAtCap = socket.binaryBytes;
  mic.emit(40000);
  assert.equal(socket.binaryBytes, bytesAtCap);
});

test("stage 9 dispose releases the mic and socket and ignores everything afterwards", async () => {
  const { session, socket, mic, events } = await startedSession();
  mic.emit(9000);
  session.dispose();
  assert.equal(mic.state.closeCount, 1);
  assert.ok(socket.closeCalled);
  const bytesAtDispose = socket.binaryBytes;
  const eventsAtDispose = events.length;
  mic.emit(9000);
  socket.serverSend({ type: "result", request_id: "r1", transcript: "late", stations: [] });
  assert.equal(socket.binaryBytes, bytesAtDispose);
  assert.equal(events.length, eventsAtDispose);
});

test("stage 9 the wall-timeout guard aborts a hung session with a timeout failure", async () => {
  const { socket, mic, events, clock } = await startedSession();
  mic.emit(9000);
  assert.equal(clock.timers.length, 1);
  clock.timers[0].handler();
  const failure = events.find((event) => event.type === "error");
  assert.equal(failure.failure.kind, "timeout");
  assert.equal(mic.state.closeCount, 1);
  assert.ok(socket.closeCalled);
  const eventsAfterTimeout = events.length;
  socket.serverSend({ type: "result", request_id: "r1", transcript: "late", stations: [] });
  assert.equal(events.length, eventsAfterTimeout);
});

test("stage 9 the default wall timer never invokes the host setTimeout on a foreign receiver", async () => {
  // Chromium's native setTimeout throws "Illegal invocation" when called with
  // a receiver; emulate that strictness to pin the wrapper default.
  const nativeSetTimeout = globalThis.setTimeout;
  let taintedReceiver = null;
  globalThis.setTimeout = function patchedSetTimeout(handler, ms) {
    if (this !== undefined && this !== globalThis) taintedReceiver = String(this);
    return nativeSetTimeout(handler, ms);
  };
  try {
    const mic = fakeMicrophone();
    const socket = new FakeVoiceSocket();
    const session = new VoiceSearchSession({
      socketFactory: () => socket,
      microphoneFactory: mic.factory,
      onEvent: () => undefined,
      // Deliberately no setTimeoutFn: the default wrapper must be used.
    });
    await session.start();
    socket.serverOpen();
    socket.serverSend({ type: "ready", request_id: "r1", audio_format: "pcm_s16le", sample_rate_hz: 16000 });
    session.dispose();
  } finally {
    globalThis.setTimeout = nativeSetTimeout;
  }
  assert.equal(taintedReceiver, null);
});

// ---- PcmDownsampler: real DSP executed on synthetic signals ----

const sineBlocks = (frequency, rate, amplitude, blocks, blockSize) => {
  const out = [];
  let phase = 0;
  const step = (2 * Math.PI * frequency) / rate;
  for (let block = 0; block < blocks; block += 1) {
    const samples = new Float32Array(blockSize);
    for (let i = 0; i < blockSize; i += 1) {
      samples[i] = amplitude * Math.sin(phase);
      phase += step;
    }
    out.push(samples);
  }
  return out;
};

const runThrough = (rate, blocks) => {
  const downsampler = new PcmDownsampler(rate);
  const outputs = [];
  for (const block of blocks) outputs.push(...downsampler.push(block));
  return outputs;
};

const rms = (values, skip = 0) => {
  const slice = values.slice(skip);
  return Math.sqrt(slice.reduce((total, value) => total + value * value, 0) / slice.length);
};

test("stage 9 downsampler keeps DC level, output rate, and int16 bounds for 48 kHz input", () => {
  const dcBlocks = Array.from({ length: 10 }, () => new Float32Array(4800).fill(0.5));
  const dc = runThrough(48000, dcBlocks);
  const expected = Math.round((10 * 4800 * 16000) / 48000);
  // The documented sub-millisecond tail (windows needing future samples) is
  // dropped instead of zero-padded: at most ceil(31 * 16000 / 48000) = 11 outputs.
  assert.ok(dc.length <= expected && dc.length >= expected - 12, `length ${dc.length}`);
  const settled = dc.slice(100);
  for (const value of settled) assert.ok(Math.abs(value - 16384) <= 2, `dc ${value}`);
  const loud = runThrough(48000, sineBlocks(440, 48000, 4, 10, 4800));
  assert.ok(loud.every((value) => value >= -32768 && value <= 32767));
  assert.ok(loud.some((value) => value === 32767));
});

test("stage 9 downsampler targets exactly 16 kHz for non-integer 44.1 kHz input", () => {
  const outputs = runThrough(44100, sineBlocks(440, 44100, 0.5, 10, 4410));
  const expected = Math.floor((10 * 4410 * VOICE_SAMPLE_RATE_HZ) / 44100);
  assert.ok(outputs.length <= expected && outputs.length >= expected - 12, `length ${outputs.length}`);
});

test("stage 9 downsampler low-passes above the 8 kHz output Nyquist before decimation", () => {
  const speech = runThrough(48000, sineBlocks(440, 48000, 0.8, 20, 4800));
  const aliasing = runThrough(48000, sineBlocks(15000, 48000, 0.8, 20, 4800));
  const speechRms = rms(speech, 200);
  const aliasRms = rms(aliasing, 200);
  // A 15 kHz tone is outside the 8 kHz output band: expect > 20 dB attenuation.
  assert.ok(speechRms > 1000, `speech rms ${speechRms}`);
  assert.ok(aliasRms < speechRms / 10, `alias rms ${aliasRms} vs speech ${speechRms}`);
});

test("stage 9 downsampler rejects impossible rates", () => {
  assert.throws(() => new PcmDownsampler(0));
  assert.throws(() => new PcmDownsampler(Number.NaN));
});
