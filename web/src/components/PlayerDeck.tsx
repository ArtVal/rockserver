import { useEffect, useRef, useState } from "preact/hooks";
import { api, type StationItem, type StationNowPlaying } from "../api";

/**
 * Persistent bottom audio deck for the cabinet.
 * Houses the single HTML5 audio streaming node connected to
 * /api/v1/stations/{id}/stream plus the SSE listener for ICY track titles.
 * The component is mounted once at cabinet level, so switching sections never
 * recreates the audio element and playback continues uninterrupted.
 *
 * Every transport state is spelled out for the user: idle hint, connecting,
 * live (with track title or an honest fallback), paused, and stream failure
 * with an explicit retry action. The desktop panel exposes station switching,
 * volume, and favorites; the compact mobile block keeps the station name,
 * the track/status line, and a large play/pause control.
 */
export function PlayerDeck({
  currentStation,
  isPlaying,
  onTogglePlay,
  onNextStation,
  onPrevStation,
  volume = 80,
  onVolumeChange,
  isFavorite = false,
  onToggleFavorite,
  trackTitle = "",
  onTrackTitleChange,
  onPlaybackStateChange,
}: {
  currentStation?: StationItem;
  isPlaying: boolean;
  onTogglePlay: () => void;
  onNextStation?: () => void;
  onPrevStation?: () => void;
  volume?: number;
  onVolumeChange?: (val: number) => void;
  isFavorite?: boolean;
  onToggleFavorite?: () => void;
  trackTitle?: string;
  onTrackTitleChange?: (title: string) => void;
  onPlaybackStateChange?: (playing: boolean) => void;
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [buffering, setBuffering] = useState(false);
  const [streamError, setStreamError] = useState("");
  // Bumped by the retry button to force a full reload of the same stream URL.
  const [retryCount, setRetryCount] = useState(0);

  // Sync volume to audio element
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = Math.max(0, Math.min(1, volume / 100));
    }
  }, [volume]);

  // (Re)load the stream when the station changes or the user retries a failure.
  // load() is required so a retry of the same URL restarts resource selection.
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    if (!currentStation) {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
      setStreamError("");
      setBuffering(false);
      return;
    }

    const streamUrl = `/api/v1/stations/${encodeURIComponent(currentStation.id)}/stream`;
    setStreamError("");
    setBuffering(isPlaying);
    audio.src = streamUrl;
    audio.load();

    if (isPlaying) {
      void audio.play().catch(() => {
        // Autoplay policy or transient network rejection
        setBuffering(false);
      });
    } else {
      audio.pause();
      setBuffering(false);
    }
  }, [currentStation?.id, retryCount]);

  // Handle play/pause state changes
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !currentStation) return;

    if (isPlaying) {
      if (audio.paused) {
        setBuffering(true);
        void audio.play().catch(() => {
          setBuffering(false);
          onPlaybackStateChange?.(false);
        });
      }
    } else {
      if (!audio.paused) {
        audio.pause();
        setBuffering(false);
      }
    }
  }, [isPlaying]);

  // Subscribe to real-time ICY title snapshots via SSE & read initial snapshot
  useEffect(() => {
    if (!currentStation) return;

    let active = true;

    // 1. Initial snapshot read from now-playing API
    void api
      .stationNowPlaying(currentStation.id)
      .then((snapshot) => {
        if (!active) return;
        const title = snapshot.rawTitle ?? snapshot.raw_title;
        if (title) {
          onTrackTitleChange?.(title);
        }
      })
      .catch(() => {
        // Fallback to tags or default stream label
      });

    // 2. Open EventSource for real-time live ICY updates
    const eventsUrl = `/api/v1/stations/${encodeURIComponent(currentStation.id)}/events`;
    const es = new EventSource(eventsUrl);

    const handleSnapshot = (event: MessageEvent) => {
      if (!active) return;
      try {
        const data = JSON.parse(event.data) as StationNowPlaying;
        const title = data.rawTitle ?? data.raw_title;
        if (title) {
          onTrackTitleChange?.(title);
        }
      } catch {
        // Non-JSON or corrupted frame ignored safely
      }
    };

    es.addEventListener("snapshot", handleSnapshot);
    es.onmessage = handleSnapshot;

    return () => {
      active = false;
      es.removeEventListener("snapshot", handleSnapshot);
      es.close();
    };
  }, [currentStation?.id]);

  /** Clears the failure and asks the cabinet to resume playback, which reloads the stream. */
  const handleStreamRetry = () => {
    if (!currentStation) return;
    setStreamError("");
    // Show "connecting" immediately: the reload effect only flips buffering on the next commit.
    setBuffering(true);
    setRetryCount((count) => count + 1);
    onPlaybackStateChange?.(true);
  };

  if (!currentStation) {
    return (
      <div class="player-deck-bar player-deck-idle" role="region" aria-label="Плеер">
        <audio ref={audioRef} preload="none" />
        <div class="player-deck-inner">
          <div class="player-idle-text">
            <span class="idle-dot" />
            <span>Выберите радиостанцию в каталоге, чтобы начать слушать прямой эфир</span>
          </div>
        </div>
      </div>
    );
  }

  const statusKind = streamError
    ? "error"
    : buffering
    ? "buffering"
    : isPlaying
    ? "playing"
    : "paused";
  const statusBadge = streamError
    ? "Ошибка"
    : buffering
    ? "Подключение"
    : isPlaying
    ? "В эфире"
    : "Пауза";
  const trackText = streamError
    ? "Поток временно недоступен или требуется вход"
    : buffering
    ? "Подключение к эфиру…"
    : trackTitle || (isPlaying ? "Прямой эфир" : "Пауза");
  const playActionLabel = buffering
    ? "Подключение к эфиру"
    : isPlaying
    ? "Приостановить"
    : "Слушать";
  // Codec and bitrate are shown only when the station actually reports them.
  const streamMeta = [
    currentStation.codec?.toUpperCase(),
    currentStation.bitrate_kbps ? `${Math.round(currentStation.bitrate_kbps)} КБИТ/С` : "",
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div class="player-deck-bar" role="region" aria-label="Плеер">
      {/* Underlying streaming audio node */}
      <audio
        ref={audioRef}
        preload="none"
        onPlay={() => {
          setBuffering(false);
          onPlaybackStateChange?.(true);
        }}
        onPause={() => {
          setBuffering(false);
          onPlaybackStateChange?.(false);
        }}
        onWaiting={() => setBuffering(true)}
        onPlaying={() => {
          setBuffering(false);
          setStreamError("");
        }}
        onError={() => {
          setBuffering(false);
          onPlaybackStateChange?.(false);
          setStreamError("Поток временно недоступен или требуется вход");
        }}
      />

      <div class="player-deck-inner">
        {/* Station identity, track or status */}
        <div class="player-station-info">
          <div class="player-cover-wrap">
            <span class="station-thumb-placeholder player-thumb" aria-hidden="true">◉</span>
            <img
              class="player-cover-img"
              src={currentStation.favicon_url || `/api/v1/stations/${encodeURIComponent(currentStation.id)}/icon`}
              alt=""
              onError={(e) => {
                e.currentTarget.style.display = "none";
              }}
            />
          </div>

          <div class="player-text-wrap">
            <div class="player-station-title-row">
              <span class="player-station-name truncate" title={currentStation.name}>
                {currentStation.name}
              </span>
              <span class={`player-status-badge ${statusKind}`}>{statusBadge}</span>
            </div>

            <div class="player-track-row" aria-live="polite">
              <span class={`track-status-dot ${statusKind}`} />
              <span
                class={`player-track-title truncate ${streamError ? "text-error" : ""}`}
                title={trackText}
              >
                {trackText}
              </span>
              {streamError && (
                <button type="button" class="player-retry-btn" onClick={handleStreamRetry}>
                  Повторить
                </button>
              )}
            </div>
          </div>

          {onToggleFavorite && (
            <button
              type="button"
              class={`player-star-btn ${isFavorite ? "active" : ""}`}
              onClick={onToggleFavorite}
              title={isFavorite ? "Удалить из избранного" : "В избранное"}
              aria-label={isFavorite ? "Удалить из избранного" : "В избранное"}
              aria-pressed={isFavorite}
            >
              {isFavorite ? "★" : "☆"}
            </button>
          )}
        </div>

        {/* Transport controls */}
        <div class="player-controls-wrap">
          <div class="player-buttons-row">
            {onPrevStation && (
              <button
                type="button"
                class="control-btn nav-step-btn"
                onClick={onPrevStation}
                title="Предыдущая станция"
                aria-label="Предыдущая станция"
              >
                ⏮
              </button>
            )}

            <button
              type="button"
              class={`master-play-btn ${
                buffering ? "is-buffering" : isPlaying ? "is-playing" : "is-paused"
              }`}
              onClick={onTogglePlay}
              title={playActionLabel}
              aria-label={playActionLabel}
            >
              {buffering ? "…" : isPlaying ? "⏸" : "▶"}
            </button>

            {onNextStation && (
              <button
                type="button"
                class="control-btn nav-step-btn"
                onClick={onNextStation}
                title="Следующая станция"
                aria-label="Следующая станция"
              >
                ⏭
              </button>
            )}
          </div>
        </div>

        {/* Stream metadata & volume */}
        <div class="player-extras-wrap">
          {streamMeta && <span class="player-meta font-mono-code">{streamMeta}</span>}

          {onVolumeChange && (
            <div class="volume-slider-wrap">
              <span class="volume-icon" aria-hidden="true">🔊</span>
              <input
                type="range"
                min="0"
                max="100"
                value={volume}
                onInput={(e) => onVolumeChange(Number(e.currentTarget.value))}
                class="volume-slider"
                aria-label="Громкость"
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
