import type { VoiceFailure } from "../voice";
import type { VoiceSearchStatus } from "../useVoiceSearch";

/**
 * Voice search states rendered next to the catalog: permission request,
 * live recording with finish/cancel, recognition, a recognized transcript with
 * a dismiss action, and failures with retry. The catalog itself shows the
 * candidate list; this panel only narrates the voice interaction.
 */
export function VoiceSearchPanel({
  status,
  transcript,
  interim,
  failure,
  elapsedMs,
  onFinish,
  onCancel,
  onDismiss,
  onRetry,
}: {
  status: VoiceSearchStatus;
  transcript: string;
  interim: string;
  failure?: VoiceFailure;
  elapsedMs: number;
  onFinish: () => void;
  onCancel: () => void;
  onDismiss: () => void;
  onRetry: () => void;
}) {
  if (status === "idle") return null;
  const totalSeconds = Math.floor(elapsedMs / 1000);
  const timer = `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, "0")}`;

  return (
    <section class="deck-panel voice-panel" role="region" aria-label="Голосовой поиск">
      {status === "requesting" && (
        <p class="voice-status" role="status">
          <span class="list-spinner voice-spinner" aria-hidden="true" />
          Запрашиваем доступ к микрофону…
        </p>
      )}

      {status === "recording" && (
        <>
          <div class="voice-recording-row">
            <span class="voice-rec-dot" aria-hidden="true" />
            <p class="voice-status" role="status" aria-live="polite">
              Слушаем… Произнесите название станции или жанр
            </p>
            <span class="voice-timer font-mono-code">{timer}</span>
          </div>
          {interim && <p class="voice-interim truncate">«{interim}»</p>}
          <div class="voice-actions">
            <button type="button" class="card-play-btn" onClick={onFinish}>
              Завершить
            </button>
            <button type="button" class="secondary" onClick={onCancel}>
              Отменить
            </button>
          </div>
        </>
      )}

      {status === "processing" && (
        <>
          <p class="voice-status" role="status" aria-live="polite">
            <span class="list-spinner voice-spinner" aria-hidden="true" />
            Распознаём запрос…
          </p>
          <div class="voice-actions">
            <button type="button" class="secondary" onClick={onCancel}>
              Отменить
            </button>
          </div>
        </>
      )}

      {status === "error" && failure && (
        <>
          <p class="voice-error" role="alert">
            {failure.message}
          </p>
          <div class="voice-actions">
            <button type="button" class="card-play-btn" onClick={onRetry}>
              Попробовать снова
            </button>
            <button type="button" class="secondary" onClick={onDismiss}>
              Закрыть
            </button>
          </div>
        </>
      )}

      {status === "result" && (
        <div class="voice-result-row">
          <p class="voice-status" role="status">
            Распознано: <span class="voice-transcript">«{transcript}»</span>
          </p>
          <div class="voice-actions">
            <button type="button" class="secondary" onClick={onDismiss}>
              Показать каталог
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
