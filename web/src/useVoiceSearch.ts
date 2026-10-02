import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { StationItem } from "./api";
import {
  VoiceSearchSession,
  voiceFailure,
  type VoiceFailure,
  type VoiceSessionEvent,
} from "./voice";
import { createVoiceSocket, openVoiceMicrophone, voiceSearchSupported } from "./voiceBrowser";

export type VoiceSearchStatus = "idle" | "requesting" | "recording" | "processing" | "result" | "error";

export type VoiceSearch = {
  status: VoiceSearchStatus;
  /** Final recognized text (shown with the results). */
  transcript: string;
  /** Non-final transcript preview, if the recognizer mode emits one. */
  interim: string;
  stations: StationItem[];
  failure?: VoiceFailure;
  elapsedMs: number;
  supported: boolean;
  start: () => void;
  finish: () => void;
  cancel: () => void;
  dismiss: () => void;
};

/**
 * Cabinet state machine around one {@link VoiceSearchSession}.
 *
 * Exactly one session exists at a time; every terminal outcome (result,
 * error, cancel) and any unmount or page exit releases the microphone and
 * the WebSocket through `dispose()`. A cancel never leaves results behind,
 * so an obsolete transcript cannot change the visible catalog.
 */
export function useVoiceSearch(): VoiceSearch {
  const [status, setStatus] = useState<VoiceSearchStatus>("idle");
  const [transcript, setTranscript] = useState("");
  const [interim, setInterim] = useState("");
  const [stations, setStations] = useState<StationItem[]>([]);
  const [failure, setFailure] = useState<VoiceFailure | undefined>(undefined);
  const [elapsedMs, setElapsedMs] = useState(0);
  const sessionRef = useRef<VoiceSearchSession | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startedAtRef = useRef(0);

  const stopTimer = useCallback(() => {
    if (timerRef.current !== null) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const teardown = useCallback(() => {
    sessionRef.current?.dispose();
    sessionRef.current = null;
    stopTimer();
  }, [stopTimer]);

  const resetOutput = useCallback(() => {
    setTranscript("");
    setInterim("");
    setStations([]);
    setFailure(undefined);
    setElapsedMs(0);
  }, []);

  const handleEvent = useCallback(
    (event: VoiceSessionEvent) => {
      switch (event.type) {
        case "started":
          setStatus("recording");
          startedAtRef.current = Date.now();
          setElapsedMs(0);
          stopTimer();
          timerRef.current = setInterval(() => {
            setElapsedMs(Date.now() - startedAtRef.current);
          }, 250);
          return;
        case "transcript":
          if (event.isFinal) setTranscript(event.text);
          else setInterim(event.text);
          return;
        case "result":
          stopTimer();
          setTranscript(event.transcript);
          setStations(event.stations);
          setStatus("result");
          sessionRef.current = null;
          return;
        case "error":
          stopTimer();
          setFailure(event.failure);
          setStatus("error");
          sessionRef.current = null;
          return;
        case "cancelled":
          stopTimer();
          resetOutput();
          setStatus("idle");
          sessionRef.current = null;
          return;
      }
    },
    [resetOutput, stopTimer]
  );

  const start = useCallback(() => {
    if (sessionRef.current) return;
    if (!voiceSearchSupported()) {
      resetOutput();
      setFailure(voiceFailure("unsupported"));
      setStatus("error");
      return;
    }
    teardown();
    resetOutput();
    setStatus("requesting");
    const session = new VoiceSearchSession({
      socketFactory: createVoiceSocket,
      microphoneFactory: openVoiceMicrophone,
      onEvent: handleEvent,
    });
    sessionRef.current = session;
    // A rejected start (permission, connection) has already emitted the
    // failure event, so the catch only prevents an unhandled rejection.
    session.start().catch(() => undefined);
  }, [handleEvent, resetOutput, teardown]);

  const finish = useCallback(() => {
    setStatus((current) => (current === "recording" ? "processing" : current));
    sessionRef.current?.finish();
  }, []);

  const cancel = useCallback(() => {
    sessionRef.current?.cancel();
  }, []);

  /** Drops results/errors and returns to the plain catalog view. */
  const dismiss = useCallback(() => {
    teardown();
    resetOutput();
    setStatus("idle");
  }, [resetOutput, teardown]);

  // Release the mic, audio graph, and socket when the page is left or the
  // cabinet unmounts (logout, registration screen).
  useEffect(() => {
    const onPageHide = () => teardown();
    window.addEventListener("pagehide", onPageHide);
    return () => {
      window.removeEventListener("pagehide", onPageHide);
      teardown();
    };
  }, [teardown]);

  return {
    status,
    transcript,
    interim,
    stations,
    failure,
    elapsedMs,
    supported: voiceSearchSupported(),
    start,
    finish,
    cancel,
    dismiss,
  };
}
