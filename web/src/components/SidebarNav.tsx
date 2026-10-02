export type NavTab = "stations" | "favorites" | "history" | "devices";

/** Navigation between the four cabinet sections. */
export function SidebarNav({
  activeTab,
  onTabChange,
}: {
  activeTab: NavTab;
  onTabChange: (tab: NavTab) => void;
}) {
  return (
    <aside class="sidebar-nav">
      <div class="deck-panel nav-section">
        <nav class="nav-links" aria-label="Разделы кабинета">
          <button
            type="button"
            class={`nav-link-btn ${activeTab === "stations" ? "active" : ""}`}
            onClick={() => onTabChange("stations")}
          >
            <span class="nav-icon" aria-hidden="true">◉</span>
            <span class="nav-label">Эфир</span>
          </button>

          <button
            type="button"
            class={`nav-link-btn ${activeTab === "favorites" ? "active" : ""}`}
            onClick={() => onTabChange("favorites")}
          >
            <span class="nav-icon" aria-hidden="true">★</span>
            <span class="nav-label">Избранное</span>
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
            <span class="nav-label">Устройства</span>
          </button>
        </nav>
      </div>
    </aside>
  );
}
