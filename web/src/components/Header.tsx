/**
 * Top application header providing the cabinet search bar with the voice
 * search trigger, account identity badge, and session logout controls.
 */
export function Header({
  accountName,
  searchQuery,
  onSearchChange,
  onSearchSubmit,
  onSearchClear,
  onLogout,
  logoutBusy,
  onVoiceSearch,
  voiceActive = false,
  voiceBusy = false,
}: {
  accountName?: string;
  searchQuery: string;
  onSearchChange: (query: string) => void;
  onSearchSubmit?: () => void;
  onSearchClear?: () => void;
  onLogout?: () => void;
  logoutBusy?: boolean;
  /** Explicit user gesture that starts the voice permission + recording flow. */
  onVoiceSearch?: () => void;
  voiceActive?: boolean;
  voiceBusy?: boolean;
}) {
  return (
    <header class="app-header">
      <div class="header-inner">
        <div class="header-brand">
          <div class="brand-logo-wrap">
            <img
              src="/icon_logo.png"
              alt="RockCast"
              class="brand-logo"
              width="36"
              height="36"
            />
          </div>
          <div class="brand-text">
            <div class="brand-title">
              <span>ROCK</span>
              <span class="brand-accent">SERVER</span>
            </div>
          </div>
        </div>

        <div class="header-search-row">
          <form
            class="header-search"
            onSubmit={(e) => {
              e.preventDefault();
              onSearchSubmit?.();
            }}
          >
            <div class="search-input-wrap">
              <span class="search-icon" aria-hidden="true">⌕</span>
              <input
                type="search"
                value={searchQuery}
                onInput={(e) => onSearchChange(e.currentTarget.value)}
                placeholder="Поиск станции, жанра или потока..."
                aria-label="Поиск станции, жанра или потока"
              />
              {searchQuery && (
                <button type="button" class="header-search-clear" onClick={onSearchClear} aria-label="Очистить поиск">
                  ✕
                </button>
              )}
              <button type="submit" class="header-search-btn" aria-label="Искать">
                Найти
              </button>
            </div>
          </form>
          {onVoiceSearch && (
            <button
              type="button"
              class={`voice-mic-btn ${voiceActive ? "active" : ""}`}
              onClick={onVoiceSearch}
              disabled={voiceBusy}
              aria-label="Голосовой поиск станции"
              title="Голосовой поиск станции"
              aria-pressed={voiceActive}
            >
              {voiceActive ? "●" : "🎤"}
            </button>
          )}
        </div>

        <div class="header-account">
          {accountName ? (
            <>
              <div class="account-pill" title={`Аккаунт: ${accountName}`}>
                <span class="account-avatar">
                  {accountName.slice(0, 2).toUpperCase()}
                </span>
                <div class="account-details">
                  <span class="account-name">{accountName}</span>
                </div>
              </div>
              {onLogout && (
                <button
                  type="button"
                  class="secondary logout-btn"
                  onClick={onLogout}
                  disabled={logoutBusy}
                  title="Выйти из браузера"
                  aria-label="Выйти из браузера"
                >
                  {logoutBusy ? "Выходим…" : "Выйти"}
                </button>
              )}
            </>
          ) : (
            <span class="anonymous-tag">Гостевой режим</span>
          )}
        </div>
      </div>
    </header>
  );
}
