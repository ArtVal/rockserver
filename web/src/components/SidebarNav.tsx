export type NavTab = "stations" | "favorites" | "history" | "devices";

const GENRE_PRESETS = [
  { id: "all", label: "#all" },
  { id: "rock", label: "#rock" },
  { id: "electronic", label: "#electronic" },
  { id: "synthwave", label: "#synthwave" },
  { id: "jazz", label: "#jazz" },
  { id: "classical", label: "#classical" },
  { id: "ambient", label: "#ambient" },
];

/**
 * Left sidebar navigation providing view selection, frequency preset filters,
 * and cloud sync status.
 */
export function SidebarNav({
  activeTab,
  onTabChange,
  selectedTag,
  onTagSelect,
  favoritesCount = 0,
}: {
  activeTab: NavTab;
  onTabChange: (tab: NavTab) => void;
  selectedTag: string;
  onTagSelect: (tag: string) => void;
  favoritesCount?: number;
}) {
  return (
    <aside class="sidebar-nav">
      {/* View selector deck */}
      <div class="deck-panel nav-section">
        <div class="panel-kicker">Консоль</div>
        <nav class="nav-links" aria-label="Разделы кабинета">
          <button
            type="button"
            class={`nav-link-btn ${activeTab === "stations" ? "active" : ""}`}
            onClick={() => onTabChange("stations")}
          >
            <span class="nav-icon" aria-hidden="true">◉</span>
            <span class="nav-label">Эфир & Станции</span>
            <span class="live-pill">LIVE</span>
          </button>

          <button
            type="button"
            class={`nav-link-btn ${activeTab === "favorites" ? "active" : ""}`}
            onClick={() => onTabChange("favorites")}
          >
            <span class="nav-icon" aria-hidden="true">★</span>
            <span class="nav-label">Моё Избранное</span>
            {favoritesCount > 0 && (
              <span class="nav-count">{favoritesCount}</span>
            )}
          </button>

          <button
            type="button"
            class={`nav-link-btn ${activeTab === "history" ? "active" : ""}`}
            onClick={() => onTabChange("history")}
          >
            <span class="nav-icon" aria-hidden="true">⏱</span>
            <span class="nav-label">История</span>
          </button>

          <button
            type="button"
            class={`nav-link-btn ${activeTab === "devices" ? "active" : ""}`}
            onClick={() => onTabChange("devices")}
          >
            <span class="nav-icon" aria-hidden="true">◈</span>
            <span class="nav-label">Оборудование</span>
          </button>
        </nav>
      </div>

      {/* Frequency presets deck */}
      <div class="deck-panel presets-section">
        <div class="panel-header-row">
          <span class="panel-kicker">Частотные фильтры</span>
          <span class="presets-badge">PRESETS</span>
        </div>
        <div class="genre-presets-grid">
          {GENRE_PRESETS.map((preset) => {
            const isSelected = selectedTag === preset.id || (!selectedTag && preset.id === "all");
            return (
              <button
                key={preset.id}
                type="button"
                class={`preset-chip ${isSelected ? "selected" : ""}`}
                onClick={() => onTagSelect(preset.id === "all" ? "" : preset.id)}
              >
                {preset.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Cloud sync status */}
      <div class="deck-panel sync-status-panel">
        <div class="sync-header">
          <span class="sync-title">Cloud Sync (LWW)</span>
          <span class="sync-badge">ONLINE</span>
        </div>
        <p class="sync-desc">
          Избранное синхронизируется между браузером, Windows RockCast и RockMobile.
        </p>
      </div>
    </aside>
  );
}
