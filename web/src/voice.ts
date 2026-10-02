/**
 * Client for the `/api/v1/voice/stream` WebSocket contract (api/openapi.yaml).
 *
 * Frame order: one JSON `start` text frame, then binary PCM s16le mono 16 kHz
 * chunks, then one `commit` (or `cancel`) text frame; the server answers
 * `ready`, zero or more `transcript` events, and exactly one terminal `result`
 * or `error`. The session is transport-agnostic: a socket factory and a
 * microphone factory are injected, so Node tests run the real state machine
 * against fakes. The browser wiring (getUserMedia + Web Audio + same-origin
 * anonymous WebSocket) lives in `voiceBrowser.ts`.
 */
import type { StationItem } from "./api";
import { SpeechEndDetector } from "./voiceActivity";

/** Server-published limits of /api/v1/voice/stream mirrored client-side. */
export const VOICE_STREAM_LIMITS = {
  sampleRateHz: 16_000,
  maxChunkBytes: 32_768,
  maxSessionBytes: 2_097_152,
  maxSessionAudioSeconds: 60,
  wallTimeoutSeconds: 75,
} as const;

/** PCM samples buffered per binary frame: 8 000 samples = 16 000 bytes ≤ 32 768. */
const CHUNK_SAMPLES = VOICE_STREAM_LIMITS.sampleRateHz / 2;
/** Recording hard cap in samples; the server rejects sessions beyond 60 s of audio. */
const MAX_SESSION_SAMPLES =
  VOICE_STREAM_LIMITS.sampleRateHz * VOICE_STREAM_LIMITS.maxSessionAudioSeconds;
/** Local wall guard: the server closes the session at 75 s, so abort shortly after. */
const WALL_TIMEOUT_MS = (VOICE_STREAM_LIMITS.wallTimeoutSeconds + 5) * 1000;

export type VoiceFailureKind =
  | "unsupported"
  | "permission"
  | "no_microphone"
  | "connection"
  | "timeout"
  | "not_recognized"
  | "provider"
  | "server";

export type VoiceFailure = { kind: VoiceFailureKind; message: string };

const FAILURE_MESSAGES: Record<VoiceFailureKind, string> = {
  unsupported: "Голосовой поиск не поддерживается в этом браузере.",
  permission: "Доступ к микрофону запрещён. Разрешите его в настройках браузера и попробуйте снова.",
  no_microphone: "Микрофон не найден или занят другим приложением. Проверьте его и попробуйте снова.",
  connection:
    "Не удалось соединиться с сервером распознавания. Возможно, превышен лимит запросов — подождите около минуты и повторите.",
  timeout: "Время голосового запроса истекло. Попробуйте произнести запрос ещё раз.",
  not_recognized: "Речь не распознана. Произнесите запрос громче и чётче.",
  provider: "Сервис распознавания временно недоступен. Повторите попытку позже.",
  server: "Сервер распознавания вернул ошибку. Повторите попытку позже.",
};

export const voiceFailure = (kind: VoiceFailureKind, message?: string): VoiceFailure => ({
  kind,
  message: message ?? FAILURE_MESSAGES[kind],
});

/** Maps a rejected getUserMedia call to a user-facing failure. */
export function microphoneFailure(error: unknown): VoiceFailure {
  const name = (error as DOMException | null)?.name ?? "";
  if (name === "NotAllowedError" || name === "SecurityError" || name === "AbortError")
    return voiceFailure("permission");
  if (
    name === "NotFoundError" ||
    name === "OverconstrainedError" ||
    name === "NotReadableError" ||
    name === "TypeError"
  )
    return voiceFailure("no_microphone");
  return voiceFailure("server");
}

/** Terminal `error` event codes grouped into client failure kinds. */
const SERVER_ERROR_KINDS: Partial<Record<string, VoiceFailureKind>> = {
  speech_not_recognized: "not_recognized",
  speech_provider_unavailable: "provider",
  speech_provider_error: "provider",
  speech_timeout: "timeout",
  voice_timeout: "timeout",
  search_timeout: "timeout",
  audio_too_large: "timeout",
};

/** Minimal WebSocket surface the session needs; the browser factory wraps the real one. */
export interface VoiceSocket {
  send(data: string | ArrayBufferLike): void;
  close(): void;
  onopen: (() => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: (() => void) | null;
  onerror: (() => void) | null;
}

export type VoiceSocketFactory = () => VoiceSocket;

/** A microphone delivering 16 kHz mono Int16 chunks; `close` releases all hardware. */
export interface VoiceMicrophone {
  close(): void;
}

export type VoiceMicrophoneFactory = (
  onChunk: (pcm: Int16Array) => void
) => Promise<VoiceMicrophone>;

export type VoiceSessionEvent =
  | { type: "started" }
  | { type: "processing" }
  | { type: "transcript"; text: string; isFinal: boolean }
  | { type: "result"; transcript: string; stations: StationItem[] }
  | { type: "error"; failure: VoiceFailure }
  | { type: "cancelled" };

/** Station candidate of the voice `result` event (server StationResult shape). */
type VoiceStationDto = Partial<StationItem> & { id: string; name: string };

const stationFromDto = (dto: VoiceStationDto): StationItem => ({
  id: dto.id,
  name: dto.name,
  stream_url: dto.stream_url ?? undefined,
  homepage_url: dto.homepage_url ?? undefined,
  favicon_url: dto.favicon_url ?? undefined,
  tags: dto.tags ?? [],
  language: dto.language ?? undefined,
  country_code: dto.country_code ?? undefined,
  codec: dto.codec ?? undefined,
  bitrate_kbps: dto.bitrate_kbps ?? undefined,
  health: dto.health ?? undefined,
});

const frameBuffer = (frame: Int16Array): ArrayBuffer =>
  frame.buffer.slice(frame.byteOffset, frame.byteOffset + frame.byteLength) as ArrayBuffer;

export type VoiceSessionState = "idle" | "connecting" | "recording" | "processing" | "done";

/**
 * One voice search attempt. Owns the microphone and the WebSocket and
 * guarantees their release on every outcome (result, error, cancel, dispose).
 * Audio captured before `ready` is buffered and only sent once recording is
 * live. After a terminal event or `cancel()`, later messages are ignored so an
 * obsolete result can never change what the user sees.
 */
export class VoiceSearchSession {
  private readonly onEvent: (event: VoiceSessionEvent) => void;
  private readonly socketFactory: VoiceSocketFactory;
  private readonly microphoneFactory: VoiceMicrophoneFactory;
  private readonly locale: string;
  private readonly limit: number;
  private readonly setTimeoutFn: (handler: () => void, ms: number) => ReturnType<typeof setTimeout>;

  private socket: VoiceSocket | null = null;
  private microphone: VoiceMicrophone | null = null;
  private state: VoiceSessionState = "idle";
  private cancelled = false;
  private terminalSeen = false;
  private committed = false;
  private totalSamples = 0;
  private readonly speechEnd = new SpeechEndDetector();
  private pendingSamples: number[] = [];
  private wallTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(options: {
    socketFactory: VoiceSocketFactory;
    microphoneFactory: VoiceMicrophoneFactory;
    onEvent: (event: VoiceSessionEvent) => void;
    locale?: string;
    limit?: number;
    /** Injectable timer for tests. */
    setTimeoutFn?: (handler: () => void, ms: number) => ReturnType<typeof setTimeout>;
  }) {
    this.socketFactory = options.socketFactory;
    this.microphoneFactory = options.microphoneFactory;
    this.onEvent = options.onEvent;
    this.locale = options.locale ?? "ru-RU";
    // The stream path caps `limit` at 10 (VoiceStreamStart schema).
    this.limit = Math.min(Math.max(options.limit ?? 10, 1), 10);
    // Native setTimeout rejects a foreign `this` ("Illegal invocation"), so the
    // default is an unbound wrapper rather than the bare host function.
    this.setTimeoutFn =
      options.setTimeoutFn ?? ((handler, ms) => setTimeout(handler, ms));
  }

  /**
   * Opens the microphone (permission prompt), then the WebSocket session.
   * Resolves when recording is live (`ready` received); rejects with a
   * VoiceFailure if permission, connection, or validation fails.
   */
  async start(): Promise<void> {
    if (this.state !== "idle") return;
    this.state = "connecting";
    this.wallTimer = this.setTimeoutFn(() => {
      this.finishWith(
        this.cancelled
          ? { type: "cancelled" }
          : { type: "error", failure: voiceFailure("timeout") }
      );
    }, WALL_TIMEOUT_MS);
    // Permission first: a denied prompt must not burn an anonymous upgrade
    // from the server's tight rate budget.
    let microphone: VoiceMicrophone;
    try {
      microphone = await this.microphoneFactory((pcm) => this.handleAudio(pcm));
    } catch (error) {
      const failure = microphoneFailure(error);
      this.finishWith({ type: "error", failure });
      throw Object.assign(new Error(failure.message), { failure });
    }
    // The session may already be terminal (cancel or wall timeout while the
    // permission prompt was open); release the mic and open no socket.
    if (this.terminalSeen) {
      microphone.close();
      return;
    }
    this.microphone = microphone;
    const socket = this.socketFactory();
    this.socket = socket;
    socket.onopen = () => {
      socket.send(
        JSON.stringify({
          type: "start",
          locale: this.locale,
          sample_rate_hz: VOICE_STREAM_LIMITS.sampleRateHz,
          recognizer_mode: "buffered_v1",
          limit: this.limit,
        })
      );
    };
    socket.onmessage = (event) => this.handleMessage(event.data);
    // Browsers fire close after error; handling both keeps fake sockets honest.
    const disconnected = () => {
      if (!this.terminalSeen && this.state !== "idle" && this.state !== "done")
        this.finishWith(
          this.cancelled ? { type: "cancelled" } : { type: "error", failure: voiceFailure("connection") }
        );
    };
    socket.onerror = disconnected;
    socket.onclose = disconnected;
  }

  /** Speech ended (or the audio cap fired): stop mic, flush, commit. */
  finish(): void {
    if (this.state !== "recording") return;
    this.releaseMicrophone();
    this.drainFrames();
    if (this.pendingSamples.length > 0) {
      // A trailing partial frame is valid PCM as long as it holds whole samples.
      const tail = Int16Array.from(this.pendingSamples);
      this.pendingSamples = [];
      this.sendRaw(tail);
    }
    this.committed = true;
    this.state = "processing";
    this.onEvent({ type: "processing" });
    this.socket?.send(JSON.stringify({ type: "commit" }));
  }

  /** Second microphone click: cancel immediately and ignore every later outcome. */
  cancel(): void {
    if (this.state === "idle" || this.state === "done") return;
    this.cancelled = true;
    this.releaseMicrophone();
    this.pendingSamples = [];
    if (this.state === "recording" && this.socket && !this.committed) {
      // Send the protocol cancellation, but do not wait to release the UI.
      this.committed = true;
      this.state = "processing";
      this.socket.send(JSON.stringify({ type: "cancel" }));
    }
    // Connecting (no socket yet) or already committed: nothing to wait for.
    this.finishWith({ type: "cancelled" });
  }

  /** Hard teardown without protocol frames; used on unmount, page exit, and dismissal. */
  dispose(): void {
    this.cancelled = true;
    this.terminalSeen = true;
    this.state = "done";
    this.releaseMicrophone();
    this.pendingSamples = [];
    this.clearWallTimer();
    if (this.socket) {
      this.detachSocket(this.socket);
      this.socket = null;
    }
  }

  /** Buffers microphone PCM; whole 8 000-sample frames go out once recording is live. */
  private handleAudio(pcm: Int16Array): void {
    if (this.cancelled) return;
    if (this.state !== "connecting" && this.state !== "recording") return;
    const remaining = Math.max(0, MAX_SESSION_SAMPLES - this.totalSamples);
    if (remaining === 0) return;
    const accepted = remaining >= pcm.length ? pcm : pcm.subarray(0, remaining);
    this.totalSamples += accepted.length;
    for (let i = 0; i < accepted.length; i += 1) this.pendingSamples.push(accepted[i]);
    this.speechEnd.push(accepted);
    if (this.state === "recording") {
      this.drainFrames();
      this.finishAutomatically();
    }
  }

  /** Defer committing pre-ready speech until the server accepts PCM frames. */
  private finishAutomatically(): void {
    if (this.speechEnd.outcome === "silence") {
      this.socket?.send(JSON.stringify({ type: "cancel" }));
      this.finishWith({ type: "error", failure: voiceFailure("not_recognized") });
    } else if (this.speechEnd.outcome === "speech" || this.totalSamples >= MAX_SESSION_SAMPLES) {
      this.finish();
    }
  }

  private drainFrames(): void {
    while (this.pendingSamples.length >= CHUNK_SAMPLES) {
      const frame = Int16Array.from(this.pendingSamples.splice(0, CHUNK_SAMPLES));
      this.sendRaw(frame);
    }
  }

  private sendRaw(frame: Int16Array): void {
    if (!this.socket || this.committed || this.cancelled) return;
    this.socket.send(frameBuffer(frame));
  }

  private handleMessage(data: unknown): void {
    if (typeof data !== "string" || this.terminalSeen) return;
    let event: Record<string, unknown>;
    try {
      event = JSON.parse(data) as Record<string, unknown>;
    } catch {
      return;
    }
    switch (event.type) {
      case "ready":
        if (this.state === "connecting") {
          this.state = "recording";
          this.drainFrames();
          this.onEvent({ type: "started" });
          this.finishAutomatically();
        }
        return;
      case "transcript":
        if (this.state === "recording" || this.state === "processing")
          this.onEvent({
            type: "transcript",
            text: typeof event.transcript === "string" ? event.transcript : "",
            isFinal: event.is_final === true,
          });
        return;
      case "result": {
        if (this.cancelled) return;
        const transcript = typeof event.transcript === "string" ? event.transcript : "";
        const stations = Array.isArray(event.stations)
          ? (event.stations as VoiceStationDto[])
              .filter((station) => typeof station?.id === "string" && station.id !== "")
              .map(stationFromDto)
          : [];
        this.finishWith({ type: "result", transcript, stations });
        return;
      }
      case "error": {
        const code = typeof event.code === "string" ? event.code : "";
        if (this.cancelled || code === "cancelled") {
          this.finishWith({ type: "cancelled" });
          return;
        }
        const kind = SERVER_ERROR_KINDS[code] ?? "server";
        this.finishWith({ type: "error", failure: voiceFailure(kind) });
        return;
      }
      default:
        return;
    }
  }

  /** Emits one terminal event, releases resources, and freezes the session. */
  private finishWith(terminal: VoiceSessionEvent): void {
    if (this.terminalSeen) return;
    this.terminalSeen = true;
    this.state = "done";
    this.releaseMicrophone();
    this.pendingSamples = [];
    this.clearWallTimer();
    if (this.socket) {
      this.detachSocket(this.socket);
      this.socket = null;
    }
    this.onEvent(terminal);
  }

  private detachSocket(socket: VoiceSocket): void {
    socket.onopen = null;
    socket.onmessage = null;
    socket.onclose = null;
    socket.onerror = null;
    socket.close();
  }

  private releaseMicrophone(): void {
    this.microphone?.close();
    this.microphone = null;
  }

  private clearWallTimer(): void {
    if (this.wallTimer !== null) {
      clearTimeout(this.wallTimer);
      this.wallTimer = null;
    }
  }
}
