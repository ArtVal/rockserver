import { useState } from "preact/hooks";
import type { StationItem } from "../api";

/**
 * Center stage catalog and search results view.
 * Renders the radio tuner card grid with station icons, bitrates,
 * live stream equalizer animations, and playback controls.
 */
export function StationsView({
  stations,
  activeStationId,
  isPlaying,
  currentTrackTitle,
  onPlayStation,
  favorites = [],
  onToggleFavorite,
  loading = false,
  searchQuery = "",
  selectedTag = "",
}: {
  stations: StationItem[];
  activeStationId?: string;
  isPlaying: boolean;
  currentTrackTitle?: string;
  onPlayStation: (station: StationItem) => void;
  favorites?: string[];
  onToggleFavorite?: (stationId: string) => void;
  loading?: boolean;
  searchQuery?: string;
  selectedTag?: string;
}) {
  const [viewMode, setViewMode] = useState<"grid" | "table">("grid");

  const subtitle = searchQuery
    ? `Результаты поиска по запросу «${searchQuery}»`
    : selectedTag
    ? `Станции по фильтру #${selectedTag}`
    : "Чистый поток без рекламы, ретрансляция с низкой задержкой";

  return (
    <section class="stations-view">
      {/* Tuner banner */}
      <div class="deck-panel tuner-banner">
        <div class="tuner-banner-glow" aria-hidden="true" />
        <div class="tuner-banner-content">
          <div>
            <div class="tuner-selector-tag">
              <span class="live-dot" />
              <span>TUNER SELECTOR</span>
            </div>
            <h1 class="tuner-title">Эфирные радиопотоки</h1>
            <p class="tuner-subtitle">{subtitle}</p>
          </div>

          <div class="view-mode-toggle" role="radiogroup" aria-label="Режим отображения">
            <button
              type="button"
              class={`mode-toggle-btn ${viewMode === "grid" ? "active" : ""}`}
              onClick={() => setViewMode("grid")}
            >
              Сетка
            </button>
            <button
              type="button"
              class={`mode-toggle-btn ${viewMode === "table" ? "active" : ""}`}
              onClick={() => setViewMode("table")}
            >
              Таблица
            </button>
          </div>
        </div>
      </div>

      {/* Loading state */}
      {loading && (
        <div class="deck-panel empty-deck" role="status">
          <p>Поиск и настройка тюнера…</p>
        </div>
      )}

      {/* Empty state */}
      {!loading && stations.length === 0 && (
        <div class="deck-panel empty-deck">
          <p class="empty-title">Станций не найдено</p>
          <p class="empty-hint">Попробуйте изменить поисковый запрос или выбрать другой фильтр жанра.</p>
        </div>
      )}

      {/* Grid view */}
      {!loading && stations.length > 0 && viewMode === "grid" && (
        <div class="stations-grid" aria-label="Список радиостанций">
          {stations.map((station) => {
            const isCurrent = activeStationId === station.id;
            const isFav = favorites.includes(station.id);
            const bitrateLabel = station.bitrate_kbps
              ? `${station.bitrate_kbps} KBPS`
              : station.codec
              ? station.codec.toUpperCase()
              : "192 KBPS";

            return (
              <article
                key={station.id}
                class={`deck-panel station-card ${isCurrent ? "deck-panel-active" : ""}`}
              >
                <div class="card-top-row">
                  <div class="station-meta-wrap">
                    <div class="station-thumb">
                      <span class="station-thumb-placeholder" aria-hidden="true">◉</span>
                      {station.favicon_url && (
                        <img
                          class="station-thumb-img"
                          src={station.favicon_url}
                          alt=""
                          onError={(e) => {
                            e.currentTarget.hidden = true;
                          }}
                        />
                      )}
                      {isCurrent && isPlaying && (
                        <span class="station-thumb-badge">PLAYING</span>
                      )}
                    </div>
                    <div class="station-names">
                      <h3 class="station-name-title" title={station.name}>
                        {station.name}
                      </h3>
                      <div class="station-badges">
                        <span class="bitrate-badge">{bitrateLabel}</span>
                        <span class="genre-text">
                          {station.tags.slice(0, 2).join(", ") || station.language || "Radio"}
                        </span>
                      </div>
                    </div>
                  </div>

                  {onToggleFavorite && (
                    <button
                      type="button"
                      class={`star-btn ${isFav ? "active" : ""}`}
                      onClick={() => onToggleFavorite(station.id)}
                      title={isFav ? "Удалить из избранного" : "Добавить в избранное"}
                      aria-label={isFav ? "В избранном" : "В избранное"}
                    >
                      {isFav ? "★" : "☆"}
                    </button>
                  )}
                </div>

                <div class="card-bottom-row">
                  <div class="now-playing-wrap">
                    <span class="now-playing-label">NOW PLAYING</span>
                    <span class="now-playing-title truncate">
                      {isCurrent && currentTrackTitle
                        ? currentTrackTitle
                        : station.tags.length > 0
                        ? station.tags.join(" · ")
                        : "Прямой эфир"}
                    </span>
                  </div>

                  <div class="card-action">
                    {isCurrent && isPlaying ? (
                      <div class="vu-equalizer" aria-label="Воспроизводится">
                        <span class="vu-bar" />
                        <span class="vu-bar" />
                        <span class="vu-bar" />
                        <span class="vu-bar" />
                      </div>
                    ) : (
                      <button
                        type="button"
                        class="play-card-btn"
                        onClick={() => onPlayStation(station)}
                        title={`Включить ${station.name}`}
                        aria-label={`Включить ${station.name}`}
                      >
                        ▶
                      </button>
                    )}
                  </div>
                </div>
              </article>
            );
          })}
        </div>
      )}

      {/* Table view */}
      {!loading && stations.length > 0 && viewMode === "table" && (
        <div class="deck-panel table-panel">
          <table class="stations-table" aria-label="Таблица радиостанций">
            <thead>
              <tr>
                <th>Станция</th>
                <th>Жанр / Теги</th>
                <th>Качество</th>
                <th>Действия</th>
              </tr>
            </thead>
            <tbody>
              {stations.map((station) => {
                const isCurrent = activeStationId === station.id;
                const isFav = favorites.includes(station.id);
                return (
                  <tr key={station.id} class={isCurrent ? "row-active" : ""}>
                    <td class="table-station-col">
                      <div class="table-station-cell">
                        <span class="station-thumb-placeholder small" aria-hidden="true">◉</span>
                        <strong>{station.name}</strong>
                      </div>
                    </td>
                    <td>{station.tags.slice(0, 3).join(", ") || "—"}</td>
                    <td>
                      <span class="bitrate-badge">
                        {station.bitrate_kbps ? `${station.bitrate_kbps}k` : "192k"}
                      </span>
                    </td>
                    <td class="table-actions-col">
                      <button
                        type="button"
                        class="secondary small-btn"
                        onClick={() => onPlayStation(station)}
                      >
                        {isCurrent && isPlaying ? "Пауза" : "Играть"}
                      </button>
                      {onToggleFavorite && (
                        <button
                          type="button"
                          class="link-button star-table-btn"
                          onClick={() => onToggleFavorite(station.id)}
                        >
                          {isFav ? "★" : "☆"}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
