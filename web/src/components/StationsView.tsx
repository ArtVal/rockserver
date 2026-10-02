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
  onSearchChange,
  onSearchSubmit,
  onSearchClear,
  searchError = "",
  selectedTag = "",
  activeTab = "stations",
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
  onSearchChange?: (query: string) => void;
  onSearchSubmit?: () => void;
  onSearchClear?: () => void;
  searchError?: string;
  selectedTag?: string;
  activeTab?: string;
}) {
  const [viewMode, setViewMode] = useState<"grid" | "table">("grid");

  const title =
    activeTab === "favorites"
      ? "Моё Избранное"
      : activeTab === "history"
      ? "История эфира"
      : "Эфирные радиопотоки";

  const subtitle = searchQuery
    ? `Результаты поиска по запросу «${searchQuery}»`
    : activeTab === "favorites"
    ? `Сохранённые любимые станции (${stations.length})`
    : activeTab === "history"
    ? `Недавно прослушанные станции (${stations.length})`
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
            <h1 class="tuner-title">{title}</h1>
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

      {/* Prominent Tuner Search Bar */}
      <form
        class="deck-panel tuner-search-deck"
        onSubmit={(e) => {
          e.preventDefault();
          onSearchSubmit?.();
        }}
      >
        <div class="tuner-search-wrap">
          <span class="search-icon" aria-hidden="true">⌕</span>
          <input
            type="search"
            class="tuner-search-input"
            value={searchQuery}
            onInput={(e) => onSearchChange?.(e.currentTarget.value)}
            placeholder="Поиск радиостанции, жанра, тега или стиля..."
            aria-label="Поиск станции, жанра или потока"
          />
          {searchQuery && (
            <button
              type="button"
              class="search-clear-btn"
              onClick={() => onSearchClear?.()}
              aria-label="Очистить поиск"
            >
              ✕
            </button>
          )}
          <button
            type="submit"
            class="search-submit-btn"
            aria-label="Искать станции"
          >
            Найти
          </button>
        </div>
      </form>

      {/* Search error state (e.g. rate limit 429) */}
      {searchError && (
        <div class="deck-panel search-error-banner" role="alert">
          <div class="search-error-content">
            <span class="search-error-icon">⚠</span>
            <span class="search-error-text">{searchError}</span>
          </div>
          {onSearchSubmit && (
            <button type="button" class="search-retry-btn" onClick={() => onSearchSubmit()}>
              Повторить
            </button>
          )}
        </div>
      )}

      {/* Loading state */}
      {loading && (
        <div class="deck-panel empty-deck" role="status">
          <p>Поиск и настройка тюнера…</p>
        </div>
      )}

      {/* Empty state */}
      {!loading && !searchError && stations.length === 0 && (
        <div class="deck-panel empty-deck">
          <p class="empty-title">
            {activeTab === "favorites"
              ? "В избранном пока нет станций"
              : activeTab === "history"
              ? "История прослушивания пока пуста"
              : "Станций не найдено"}
          </p>
          <p class="empty-hint">
            {activeTab === "favorites"
              ? "Нажмите звёздочку ★ на карточке любой радиостанции, чтобы сохранить её в избранное."
              : activeTab === "history"
              ? "Включите любую станцию в каталоге, и она автоматически появится в вашей истории."
              : "Попробуйте изменить поисковый запрос или выбрать другой фильтр жанра."}
          </p>
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
            const iconUrl = station.favicon_url || `/api/v1/stations/${encodeURIComponent(station.id)}/icon`;

            return (
              <div
                key={station.id}
                class={`deck-panel station-card ${isCurrent ? "deck-panel-active" : ""}`}
                onClick={() => onPlayStation(station)}
              >
                <div class="station-card-top">
                  <div class="station-icon-wrap">
                    <img
                      src={iconUrl}
                      alt=""
                      class="station-icon"
                      loading="lazy"
                      onError={(e) => {
                        e.currentTarget.style.display = "none";
                        const fallback = e.currentTarget.nextElementSibling as HTMLElement;
                        if (fallback) fallback.style.display = "grid";
                      }}
                    />
                    <div
                      class="station-icon-fallback"
                      style={{ display: "none" }}
                    >
                      {station.name.slice(0, 2).toUpperCase()}
                    </div>
                  </div>

                  <div class="station-top-meta">
                    <span class="bitrate-badge">{bitrateLabel}</span>
                    {onToggleFavorite && (
                      <button
                        type="button"
                        class={`star-btn ${isFav ? "active" : ""}`}
                        aria-label={isFav ? "Удалить из избранного" : "В избранное"}
                        onClick={(e) => {
                          e.stopPropagation();
                          onToggleFavorite(station.id);
                        }}
                      >
                        {isFav ? "★" : "☆"}
                      </button>
                    )}
                  </div>
                </div>

                <div class="station-card-body">
                  <div class="station-title-row">
                    <h2 class="station-name truncate" title={station.name}>
                      {station.name}
                    </h2>
                  </div>

                  <div class="station-tags-row">
                    {station.tags.slice(0, 3).map((tag) => (
                      <span key={tag} class="station-tag">
                        #{tag}
                      </span>
                    ))}
                    {station.country_code && (
                      <span class="station-tag country-tag">{station.country_code}</span>
                    )}
                  </div>

                  {isCurrent && (
                    <div class={`live-track-preview ${isPlaying ? "playing" : "paused"}`}>
                      {isPlaying ? (
                        <div class="vu-equalizer">
                          <span class="vu-bar" />
                          <span class="vu-bar" />
                          <span class="vu-bar" />
                          <span class="vu-bar" />
                        </div>
                      ) : (
                        <span class="track-paused-icon" aria-hidden="true">⏸</span>
                      )}
                      <span class="live-track-text truncate" title={currentTrackTitle || "Прямой эфир"}>
                        {currentTrackTitle || (isPlaying ? "Прямой эфир" : "Пауза")}
                      </span>
                    </div>
                  )}
                </div>

                <div class="station-card-footer">
                  <button
                    type="button"
                    class={`card-play-btn ${isCurrent && isPlaying ? "playing" : ""}`}
                    aria-label={isCurrent && isPlaying ? "Пауза" : "Слушать поток"}
                    onClick={(e) => {
                      e.stopPropagation();
                      onPlayStation(station);
                    }}
                  >
                    {isCurrent && isPlaying ? "⏸ Пауза" : "▶ Слушать"}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Table view */}
      {!loading && stations.length > 0 && viewMode === "table" && (
        <div class="deck-panel stations-table-deck">
          <table class="stations-table" aria-label="Список радиостанций">
            <thead>
              <tr>
                <th style={{ width: "40px" }} />
                <th>Станция</th>
                <th>Жанр / Теги</th>
                <th>Формат</th>
                <th>Страна</th>
                <th style={{ width: "90px" }} />
              </tr>
            </thead>
            <tbody>
              {stations.map((station) => {
                const isCurrent = activeStationId === station.id;
                const isFav = favorites.includes(station.id);
                const formatLabel = station.codec ? `${station.codec} ${station.bitrate_kbps ? station.bitrate_kbps + 'k' : ''}` : "MP3";

                return (
                  <tr
                    key={station.id}
                    class={`station-table-row ${isCurrent ? "table-row-active" : ""}`}
                    onClick={() => onPlayStation(station)}
                  >
                    <td>
                      {isCurrent && isPlaying ? (
                        <div class="vu-equalizer">
                          <span class="vu-bar" />
                          <span class="vu-bar" />
                          <span class="vu-bar" />
                          <span class="vu-bar" />
                        </div>
                      ) : (
                        <span class="table-play-icon">▶</span>
                      )}
                    </td>
                    <td>
                      <div class="table-station-info">
                        <div class="table-station-cell">
                          <div class="station-icon-wrap table-thumb">
                            <img
                              src={station.favicon_url || `/api/v1/stations/${encodeURIComponent(station.id)}/icon`}
                              alt=""
                              class="station-icon"
                              loading="lazy"
                              onError={(e) => {
                                e.currentTarget.style.display = "none";
                                const fallback = e.currentTarget.nextElementSibling as HTMLElement;
                                if (fallback) fallback.style.display = "grid";
                              }}
                            />
                            <div class="station-icon-fallback" style={{ display: "none" }}>
                              {station.name.slice(0, 2).toUpperCase()}
                            </div>
                          </div>
                          <strong>{station.name}</strong>
                        </div>
                        {isCurrent && (
                          <small class="table-track truncate">
                            {isPlaying ? "▶ " : "⏸ "}
                            {currentTrackTitle || (isPlaying ? "Прямой эфир" : "Пауза")}
                          </small>
                        )}
                      </div>
                    </td>
                    <td>
                      <div class="table-tags">
                        {station.tags.slice(0, 2).map((t) => (
                          <span key={t} class="station-tag">#{t}</span>
                        ))}
                      </div>
                    </td>
                    <td><span class="bitrate-badge">{formatLabel}</span></td>
                    <td><span class="country-cell">{station.country_code || "—"}</span></td>
                    <td onClick={(e) => e.stopPropagation()}>
                      {onToggleFavorite && (
                        <button
                          type="button"
                          class={`star-btn ${isFav ? "active" : ""}`}
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
