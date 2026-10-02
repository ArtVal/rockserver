import type { StationItem } from "../api";
import { ListFooter, type ListPaging } from "./ListFooter";

/** Renders catalog, favorites, history, and voice candidates with the same station actions. */
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
  onSearchSubmit,
  searchError = "",
  selectedTag = "",
  activeTab = "stations",
  favoriteCount = 0,
  viewMode = "table",
  onViewModeChange,
  paging,
  voiceQuery = "",
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
  onSearchSubmit?: () => void;
  searchError?: string;
  selectedTag?: string;
  activeTab?: string;
  favoriteCount?: number;
  viewMode?: "grid" | "table";
  onViewModeChange?: (mode: "grid" | "table") => void;
  paging: ListPaging;
  /** Recognized voice query whose candidates are shown instead of the text issuance. */
  voiceQuery?: string;
}) {
  /** The full list size (synced records or server total), not only the shown slice. */
  const totalListSize = paging.total ?? stations.length;

  const title =
    activeTab === "favorites"
      ? "Моё Избранное"
      : activeTab === "history"
      ? "История эфира"
      : voiceQuery
      ? "Голосовой поиск"
      : "Эфирные радиопотоки";

  const subtitle = voiceQuery
    ? `Голосовой запрос: «${voiceQuery}». Выберите станцию — воспроизведение не запускается автоматически.`
    : searchQuery
    ? `Результаты поиска по запросу «${searchQuery}»`
    : activeTab === "favorites"
    ? `Сохранённые станции (${favoriteCount})`
    : activeTab === "history"
    ? `Недавно прослушанные станции (${totalListSize})`
    : selectedTag
    ? `Станции по фильтру #${selectedTag}`
    : "Выберите станцию и включите эфир";

  const format = (station: StationItem) => [
    station.codec?.trim().toUpperCase(),
    Number.isFinite(station.bitrate_kbps) && station.bitrate_kbps! > 0
      ? `${Math.round(station.bitrate_kbps!)} кбит/с` : undefined,
  ].filter(Boolean).join(" · ");
  const metadata = (station: StationItem, includeFormat = true) => [
    ...station.tags.slice(0, 3).map((tag) => `#${tag}`),
    station.country_code,
    station.language,
    includeFormat && format(station),
  ].filter(Boolean);
  const stationName = (station: StationItem) =>
    station.name && station.name !== "Радиостанция" && station.name !== station.id
      ? station.name : "Станция без названия";

  return (
    <section class="stations-view">
      {/* Tuner banner */}
      <div class="deck-panel tuner-banner">
        <div class="tuner-banner-glow" aria-hidden="true" />
        <div class="tuner-banner-content">
          <div>
            <div class="tuner-selector-tag">
              <span class="live-dot" />
              <span>Прямой эфир</span>
            </div>
            <h1 class="tuner-title">{title}</h1>
            <p class="tuner-subtitle">{subtitle}</p>
          </div>

          <div class="view-mode-toggle" role="group" aria-label="Режим отображения">
            <button
              type="button"
              class={`mode-toggle-btn ${viewMode === "grid" ? "active" : ""}`}
              aria-pressed={viewMode === "grid"}
              onClick={() => onViewModeChange?.("grid")}
            >
              Сетка
            </button>
            <button
              type="button"
              class={`mode-toggle-btn ${viewMode === "table" ? "active" : ""}`}
              aria-pressed={viewMode === "table"}
              onClick={() => onViewModeChange?.("table")}
            >
              Таблица
            </button>
          </div>
        </div>
      </div>

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
              : voiceQuery
              ? "По голосовому запросу ничего не найдено"
              : "Станций не найдено"}
          </p>
          <p class="empty-hint">
            {activeTab === "favorites"
              ? "Нажмите звёздочку ★ на карточке любой радиостанции, чтобы сохранить её в избранное."
              : activeTab === "history"
              ? "Включите любую станцию в каталоге, и она автоматически появится в вашей истории."
              : voiceQuery
              ? "Произнесите запрос иначе или введите его текстом в поиске выше — текстовый поиск остаётся доступен."
              : "Измените или очистите поиск, либо сбросьте фильтр жанра кнопкой #all."}
          </p>
        </div>
      )}

      {/* Grid view */}
      {!loading && stations.length > 0 && viewMode === "grid" && (
        <div class="stations-grid" aria-label="Список радиостанций">
          {stations.map((station) => {
            const isCurrent = activeStationId === station.id;
            const isFav = favorites.includes(station.id);
            const iconUrl = station.favicon_url || `/api/v1/stations/${encodeURIComponent(station.id)}/icon`;

            return (
              <div
                key={station.id}
                class={`deck-panel station-card ${isCurrent ? "deck-panel-active" : ""}`}
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
                      {stationName(station).slice(0, 2).toUpperCase()}
                    </div>
                  </div>

                  <div class="station-top-meta">
                    {format(station) && <span class="bitrate-badge">{format(station)}</span>}
                    {onToggleFavorite && (
                      <button
                        type="button"
                        class={`star-btn ${isFav ? "active" : ""}`}
                        aria-label={`${isFav ? "Удалить из избранного" : "В избранное"}: ${stationName(station)}`}
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
                    <h2 class="station-name" title={stationName(station)}>
                      {stationName(station)}
                    </h2>
                  </div>

                  <div class="station-tags-row">
                    {metadata(station, false).map((value, index) => (
                      <span key={`${index}-${value}`} class="station-tag">{value}</span>
                    ))}
                    {metadata(station).length === 0 && <span class="station-meta-empty">Данные о станции не указаны</span>}
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
                    aria-label={`${isCurrent && isPlaying ? "Пауза" : "Слушать"}: ${stationName(station)}`}
                    onClick={(e) => {
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
                <th>Станция</th>
                <th>Данные</th>
                <th>Действия</th>
              </tr>
            </thead>
            <tbody>
              {stations.map((station) => {
                const isCurrent = activeStationId === station.id;
                const isFav = favorites.includes(station.id);

                return (
                  <tr
                    key={station.id}
                    class={`station-table-row ${isCurrent ? "table-row-active" : ""}`}
                  >
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
                              {stationName(station).slice(0, 2).toUpperCase()}
                            </div>
                          </div>
                          <strong>{stationName(station)}</strong>
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
                        {metadata(station).map((value, index) => (
                          <span key={`${index}-${value}`} class="station-tag">{value}</span>
                        ))}
                        {metadata(station).length === 0 && <span class="station-meta-empty">Данные не указаны</span>}
                      </div>
                    </td>
                    <td>
                      <div class="table-actions">
                        <button type="button" class="card-play-btn" onClick={() => onPlayStation(station)}
                          aria-label={`${isCurrent && isPlaying ? "Пауза" : "Слушать"}: ${stationName(station)}`}>
                          {isCurrent && isPlaying ? "⏸ Пауза" : "▶ Слушать"}
                        </button>
                      {onToggleFavorite && (
                        <button
                          type="button"
                          class={`star-btn ${isFav ? "active" : ""}`}
                          aria-label={`${isFav ? "Удалить из избранного" : "В избранное"}: ${stationName(station)}`}
                          onClick={() => onToggleFavorite(station.id)}
                        >
                          {isFav ? "★" : "☆"}
                        </button>
                      )}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* Infinite-scroll footer: sentinel, loading, retry, manual load, end of list */}
      {!loading && stations.length > 0 && <ListFooter paging={paging} />}
    </section>
  );
}
