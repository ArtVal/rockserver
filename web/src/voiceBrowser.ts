/**
 * Browser wiring for the voice stream client.
 *
 * The WebSocket is a plain same-origin connection WITHOUT an Authorization
 * header: `/api/v1/voice/stream` then applies its anonymous limits, which is
 * the intended browser path. The browser session cookie is not a native
 * credential, and device secrets or deployment tokens must never reach this
 * client. The microphone graph captures at the device rate and converts to
 * 16 kHz mono Int16 PCM via the shared `PcmDownsampler`; audio never comes
 * from a compressed-container recorder API, only from raw capture taps.
 *
 * QA seam: `window.__rockserverVoiceQa` (installed only from the
 * `voice-fixture` URL parameter, see `voiceFixture.ts`) may replace the socket
 * factory, microphone factory, and support probe for deterministic harness
 * fixtures. It carries no credentials and is undefined in normal use.
 */
import { PcmDownsampler } from "./voicePcm";
import type { VoiceMicrophoneFactory, VoiceSocket, VoiceSocketFactory } from "./voice";

/** Same-origin anonymous voice stream endpoint; cookies are ignored by it for auth. */
export const voiceStreamUrl = () =>
  `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/api/v1/voice/stream`;

type VoiceQaOverrides = {
  createSocket?: VoiceSocketFactory;
  openMicrophone?: VoiceMicrophoneFactory;
  supported?: () => boolean;
};

const voiceQa = (): VoiceQaOverrides | undefined =>
  (window as unknown as { __rockserverVoiceQa?: VoiceQaOverrides }).__rockserverVoiceQa;

/** Adapts the native WebSocket to the session's minimal socket interface. */
const createBrowserVoiceSocket = (): VoiceSocket => {
  const ws = new WebSocket(voiceStreamUrl());
  ws.binaryType = "arraybuffer";
  const adapter: VoiceSocket = {
    send: (data) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(data as string | ArrayBuffer);
    },
    close: () => ws.close(),
    onopen: null,
    onmessage: null,
    onclose: null,
    onerror: null,
  };
  ws.onopen = () => adapter.onopen?.();
  ws.onmessage = (event) => adapter.onmessage?.({ data: event.data });
  ws.onclose = () => adapter.onclose?.();
  ws.onerror = () => adapter.onerror?.();
  return adapter;
};

const browserVoiceSearchSupported = (): boolean =>
  typeof WebSocket !== "undefined" &&
  typeof navigator !== "undefined" &&
  typeof navigator.mediaDevices?.getUserMedia === "function" &&
  (typeof AudioContext !== "undefined" ||
    typeof (window as { webkitAudioContext?: unknown }).webkitAudioContext !== "undefined");

/** Opens the QA socket override or the real same-origin anonymous WebSocket. */
export const createVoiceSocket: VoiceSocketFactory = () =>
  voiceQa()?.createSocket?.() ?? createBrowserVoiceSocket();

/** Reports whether this browser can run voice search at all. */
export const voiceSearchSupported = (): boolean =>
  voiceQa()?.supported?.() ?? browserVoiceSearchSupported();

/**
 * Captures the default microphone and delivers 16 kHz mono Int16 chunks.
 *
 * The capture graph is getUserMedia → AudioContext at the device rate →
 * AudioWorklet tap (ScriptProcessor fallback) → main-thread PcmDownsampler.
 * `close()` stops tracks, disconnects nodes, and closes the context, so the
 * mic indicator turns off on every outcome.
 */
const openBrowserMicrophone: VoiceMicrophoneFactory = async (onChunk) => {
  if (!navigator.mediaDevices?.getUserMedia)
    throw Object.assign(new Error("getUserMedia is unavailable"), { name: "TypeError" });
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    video: false,
  });
  const AudioContextCtor =
    window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  const context = new AudioContextCtor();
  const downsampler = new PcmDownsampler(context.sampleRate);
  const source = context.createMediaStreamSource(stream);
  let closed = false;
  const emit = (block: Float32Array) => {
    if (closed || block.length === 0) return;
    const pcm = downsampler.push(block);
    if (pcm.length > 0) onChunk(pcm);
  };
  const teardown = new Set<() => void>();
  try {
    if (context.audioWorklet) {
      await context.audioWorklet.addModule("/voice-worklet.js");
      const tap = new AudioWorkletNode(context, "rockserver-voice-tap", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        channelCount: 1,
        channelCountMode: "explicit",
      });
      tap.port.onmessage = (event) => emit(event.data as Float32Array);
      // A zero-gain destination keeps the worklet pulled without feedback.
      const mute = context.createGain();
      mute.gain.value = 0;
      source.connect(tap);
      tap.connect(mute);
      mute.connect(context.destination);
      teardown.add(() => {
        tap.port.onmessage = null;
        tap.disconnect();
        mute.disconnect();
      });
    } else {
      const processor = context.createScriptProcessor(4096, 1, 1);
      processor.onaudioprocess = (event) => emit(event.inputBuffer.getChannelData(0));
      const mute = context.createGain();
      mute.gain.value = 0;
      source.connect(processor);
      processor.connect(mute);
      mute.connect(context.destination);
      teardown.add(() => {
        processor.onaudioprocess = null;
        processor.disconnect();
        mute.disconnect();
      });
    }
    if (context.state === "suspended") await context.resume();
  } catch (error) {
    stream.getTracks().forEach((track) => track.stop());
    void context.close();
    teardown.forEach((dispose) => dispose());
    throw error;
  }
  return {
    close: () => {
      if (closed) return;
      closed = true;
      stream.getTracks().forEach((track) => track.stop());
      teardown.forEach((dispose) => dispose());
      source.disconnect();
      void context.close();
    },
  };
};

/** Opens the QA microphone override or the real capture graph. */
export const openVoiceMicrophone: VoiceMicrophoneFactory = (onChunk) =>
  voiceQa()?.openMicrophone?.(onChunk) ?? openBrowserMicrophone(onChunk);
