import { useEffect, useRef, useState } from "preact/hooks";
import { api, type StationItem, type StationNowPlaying } from "../api";

/**
 * Bottom persistent audio deck player.
 * Houses the HTML5 <audio> streaming node connected to /api/v1/stations/{id}/stream,
 * real-time SSE listener for ICY track title snapshots, buffer status,
 * master transport controls, and volume attenuation.
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

  // Sync volume to audio element
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = Math.max(0, Math.min(1, volume / 100));
    }
  }, [volume]);

  // Handle station change & stream playback
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    if (!currentStation) {
      audio.pause();
      audio.removeAttribute("src");
      setStreamError("");
      setBuffering(false);
      return;
    }

    const streamUrl = `/api/v1/stations/${encodeURIComponent(currentStation.id)}/stream`;
    setStreamError("");
    setBuffering(true);
    audio.src = streamUrl;

    if (isPlaying) {
      void audio.play().catch(() => {
        // Autoplay policy or transient network rejection
        setBuffering(false);
      });
    } else {
      audio.pause();
      setBuffering(false);
    }
  }, [currentStation?.id]);

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

  if (!currentStation) {
    return (
      <div class="player-deck-bar player-deck-idle" role="region" aria-label="Радиопроигрыватель">
        <audio ref={audioRef} preload="none" />
        <div class="player-deck-inner">
          <div class="player-idle-text">
            <span class="idle-dot" />
            <span>Выберите радиостанцию в каталоге для начала прямого эфира</span>
          </div>
        </div>
      </div>
    );
  }

  const displayTitle = streamError
    ? streamError
    : buffering
    ? "Подключение к эфиру…"
    : trackTitle || (isPlaying ? "Прямой эфир" : "Пауза");

  return (
    <div class="player-deck-bar" role="region" aria-label="Радиопроигрыватель">
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
        {/* Track & Station Identity */}
        <div class="player-station-info">
          <div class="player-cover-wrap">
            <span class="station-thumb-placeholder player-thumb" aria-hidden="true">◉</span>
            {currentStation.favicon_url && (
              <img
                class="player-cover-img"
                src={currentStation.favicon_url}
                alt=""
                onError={(e) => {
                  e.currentTarget.hidden = true;
                }}
              />
            )}
          </div>

          <div class="player-text-wrap">
            <div class="player-station-title-row">
              <span class="player-station-name truncate" title={currentStation.name}>
                {currentStation.name}
              </span>
              <span class="player-icy-badge">LIVE ICY</span>
            </div>

            <div class="player-track-row">
              <span
                class={`track-status-dot ${
                  streamError
                    ? "error"
                    : buffering
                    ? "buffering"
                    : isPlaying
                    ? "playing"
                    : "paused"
                }`}
              />
              <span
                class={`player-track-title truncate ${streamError ? "text-error" : ""}`}
                title={displayTitle}
              >
                {displayTitle}
              </span>
            </div>
          </div>

          {onToggleFavorite && (
            <button
              type="button"
              class={`player-star-btn ${isFavorite ? "active" : ""}`}
              onClick={onToggleFavorite}
              title={isFavorite ? "Удалить из избранного" : "В избранное"}
              aria-label={isFavorite ? "В избранном" : "В избранное"}
            >
              {isFavorite ? "★" : "☆"}
            </button>
          )}
        </div>

        {/* Playback Controls Center */}
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
              title={isPlaying ? "Приостановить" : "Воспроизвести"}
              aria-label={isPlaying ? "Приостановить" : "Воспроизвести"}
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

          <div class="player-status-line">
            <span class="relay-tag">DIRECT RELAY</span>
            <span>·</span>
            <span class="codec-tag font-mono-code">
              {currentStation.codec?.toUpperCase() || "MP3"} ·{" "}
              {currentStation.bitrate_kbps ? `${currentStation.bitrate_kbps} KBPS` : "320 KBPS"}
            </span>
          </div>
        </div>

        {/* Volume & Cast Actions */}
        <div class="player-extras-wrap">
          <button
            type="button"
            class="cast-device-btn"
            title="Отправить воспроизведение на RockCast"
          >
            <span class="cast-icon" aria-hidden="true">⎘</span>
            <span>CAST: RockCast</span>
          </button>

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
