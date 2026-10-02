/**
 * Deterministic QA fixtures for voice search, installed only when the page is
 * opened with a `voice-fixture=…` URL parameter (local harness browser QA).
 *
 * The synthetic microphone generates real PCM through the production
 * `PcmDownsampler` at a non-native 44.1 kHz rate, so the harness run exercises
 * the real session orchestration, resampling, and WebSocket protocol against
 * the harness fixture server; only the audio hardware is fake. Failure modes:
 * `denied` (permission rejected), `nomicrophone` (device missing), and
 * `unsupported` (browser without voice APIs). The parameter is removed from
 * the URL right after installation and no credentials are involved anywhere.
 */
import { PcmDownsampler } from "./voicePcm";
import type { VoiceMicrophone, VoiceMicrophoneFactory } from "./voice";

export type VoiceFixtureMode = "tone" | "silence" | "denied" | "nomicrophone" | "unsupported";

/** Observability for QA assertions: how often the fake mic opened and closed. */
export type VoiceFixtureState = { opened: number; closed: number; samples: number };

const FIXTURE_INPUT_RATE = 44_100;
const FIXTURE_BLOCK = 4_410; // 100 ms of synthetic input per tick
const FIXTURE_TONE_HZ = 440;

export function maybeInstallVoiceFixture(): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(location.search);
  const mode = params.get("voice-fixture") as VoiceFixtureMode | null;
  if (!mode) return;
  params.delete("voice-fixture");
  history.replaceState(
    null,
    "",
    `${location.pathname}${params.size ? `?${params}` : ""}${location.hash}`
  );
  const state: VoiceFixtureState = { opened: 0, closed: 0, samples: 0 };
  (window as unknown as Record<string, unknown>).__rockserverVoiceFixtureState = state;
  const overrides: {
    openMicrophone?: VoiceMicrophoneFactory;
    supported?: () => boolean;
  } = {};
  if (mode === "unsupported") {
    overrides.supported = () => false;
  } else {
    overrides.openMicrophone = (onChunk) =>
      fixtureMicrophone(mode, state, onChunk) as Promise<VoiceMicrophone>;
  }
  (window as unknown as Record<string, unknown>).__rockserverVoiceQa = overrides;
}

/** Synthetic microphone: permission failures or a 44.1 kHz tone/silence stream. */
function fixtureMicrophone(
  mode: VoiceFixtureMode,
  state: VoiceFixtureState,
  onChunk: (pcm: Int16Array) => void
): Promise<VoiceMicrophone> {
  state.opened += 1;
  if (mode === "denied")
    return Promise.reject(
      Object.assign(new Error("fixture: permission denied"), { name: "NotAllowedError" })
    );
  if (mode === "nomicrophone")
    return Promise.reject(
      Object.assign(new Error("fixture: no microphone"), { name: "NotFoundError" })
    );
  const downsampler = new PcmDownsampler(FIXTURE_INPUT_RATE);
  let ticks = 0;
  let phase = 0;
  const step = (2 * Math.PI * FIXTURE_TONE_HZ) / FIXTURE_INPUT_RATE;
  let closed = false;
  // Real-time pacing keeps the 60 s session cap measured in true seconds.
  const timer = setInterval(() => {
    if (closed) return;
    const amplitude = mode === "tone" && ++ticks > 4 && ticks <= 16 ? 0.3 : 0;
    const block = new Float32Array(FIXTURE_BLOCK);
    for (let i = 0; i < FIXTURE_BLOCK; i += 1) {
      block[i] = amplitude * Math.sin(phase);
      phase += step;
    }
    state.samples += FIXTURE_BLOCK;
    const pcm = downsampler.push(block);
    if (pcm.length > 0) onChunk(pcm);
  }, 100);
  return Promise.resolve({
    close: () => {
      if (closed) return;
      closed = true;
      clearInterval(timer);
      state.closed += 1;
    },
  });
}
