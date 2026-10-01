/**
 * Top application header providing system status, tuner search bar,
 * account identity badge, and session logout controls.
 */
export function Header({
  accountName,
  searchQuery,
  onSearchChange,
  onSearchSubmit,
  onLogout,
  logoutBusy,
}: {
  accountName?: string;
  searchQuery: string;
  onSearchChange: (query: string) => void;
  onSearchSubmit?: () => void;
  onLogout?: () => void;
  logoutBusy?: boolean;
}) {
  return (
    <header class="app-header">
      <div class="header-inner">
        {/* Brand identity */}
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
              <span class="brand-version">CABINET v1.4</span>
            </div>
            <div class="brand-status">
              <span class="status-indicator-dot" />
              <span>RELAY ONLINE · 16.8K STATIONS</span>
            </div>
          </div>
        </div>

        {/* Global search tuner input */}
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
            <button type="submit" class="header-search-btn" aria-label="Искать">
              Найти
            </button>
          </div>
        </form>

        {/* Account identity and logout */}
        <div class="header-account">
          {accountName ? (
            <>
              <div class="account-pill" title={`Аккаунт: ${accountName}`}>
                <span class="account-avatar">
                  {accountName.slice(0, 2).toUpperCase()}
                </span>
                <div class="account-details">
                  <span class="account-name">{accountName}</span>
                  <span class="account-auth-tag">PASSKEY AUTH</span>
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
