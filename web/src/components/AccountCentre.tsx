import { useEffect, useRef, useState } from "preact/hooks";
import { api, type BrowserAccount, type BrowserDevice, type StationItem } from "../api";
import { Header } from "./Header";
import { SidebarNav, type NavTab } from "./SidebarNav";
import { StationsView } from "./StationsView";
import { HardwareHud, type JustConnected } from "./HardwareHud";
import { YandexHomeCard } from "./YandexHomeCard";
import { PlayerDeck } from "./PlayerDeck";

export type AccountState = "loading" | "anonymous" | "authenticated" | "expired" | "unavailable";

export interface AccountCentreProps {
  account?: BrowserAccount;
  accountState: AccountState;
  accountName: string;
  accountMessage: string;
  csrf: string;
  authBusy: boolean;
  deviceBusy: string;
  logoutBusy: boolean;
  justConnected?: JustConnected;
  yandexHomeStatus?: string;
  onAuthenticate: () => Promise<void>;
  onRegister: () => void;
  onRetry: () => Promise<void>;
  onRename: (device: BrowserDevice) => Promise<void>;
  onRevoke: (device: BrowserDevice) => Promise<void>;
  onLogout: () => Promise<void>;
}

/** Renders the safe browser account and native-device cabinet with modern studio tuner UI. */
export function AccountCentre({
  account,
  accountState,
  accountName,
  accountMessage,
  csrf,
  authBusy,
  deviceBusy,
  logoutBusy,
  justConnected,
  yandexHomeStatus,
  onAuthenticate,
  onRegister,
  onRetry,
  onRename,
  onRevoke,
  onLogout,
}: AccountCentreProps) {
  const [activeTab, setActiveTab] = useState<NavTab>("stations");
  const [searchQuery, setSearchQuery] = useState("");
  const [activeSearch, setActiveSearch] = useState("");
  const [selectedTag, setSelectedTag] = useState("");
  const [searchError, setSearchError] = useState("");

  const handleSearchChange = (query: string) => {
    setSearchQuery(query);
  };

  const stationCache = useRef<Map<string, StationItem[]>>(new Map());

  const handleSearchSubmit = (query?: string) => {
    const q = (query !== undefined ? query : searchQuery).trim();
    if (q) {
      stationCache.current.delete(q);
      setActiveSearch(q);
      setSelectedTag("");
    } else {
      const activeQuery = selectedTag || "rock";
      stationCache.current.delete(activeQuery);
      setActiveSearch("");
    }
    if (activeTab !== "stations" && activeTab !== "favorites" && activeTab !== "history") {
      setActiveTab("stations");
    }
  };

  const handleSearchClear = () => {
    setSearchQuery("");
    setActiveSearch("");
    setSearchError("");
  };

  const handleTagSelect = (tag: string) => {
    setSelectedTag(tag);
    setSearchQuery("");
    setActiveSearch("");
    setSearchError("");
    if (activeTab !== "stations") {
      setActiveTab("stations");
    }
  };

  // Debounced typing: triggers auto-search only after user stops typing for 700ms.
  // Prevents sending network requests on every keystroke (avoids 429 rate limits).
  useEffect(() => {
    if (!searchQuery.trim()) {
      if (activeSearch) setActiveSearch("");
      return;
    }
    const timer = setTimeout(() => {
      setActiveSearch(searchQuery.trim());
      setSelectedTag("");
      if (activeTab !== "stations" && activeTab !== "favorites" && activeTab !== "history") {
        setActiveTab("stations");
      }
    }, 700);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  const [stations, setStations] = useState<StationItem[]>([]);
  const [stationsLoading, setStationsLoading] = useState(false);
  const [favorites, setFavorites] = useState<string[]>(() => {
    try {
      const stored = localStorage.getItem("rockserver_player_favorites");
      return stored ? (JSON.parse(stored) as string[]) : [];
    } catch {
      return [];
    }
  });
  const [history, setHistory] = useState<StationItem[]>(() => {
    try {
      const stored = localStorage.getItem("rockserver_player_history");
      return stored ? (JSON.parse(stored) as StationItem[]) : [];
    } catch {
      return [];
    }
  });
  const [currentStation, setCurrentStation] = useState<StationItem>();
  const [isPlaying, setIsPlaying] = useState(false);
  const [trackTitle, setTrackTitle] = useState("");
  const [volume, setVolume] = useState<number>(() => {
    try {
      const stored = localStorage.getItem("rockserver_player_volume");
      return stored ? Math.max(0, Math.min(100, Number(stored))) : 80;
    } catch {
      return 80;
    }
  });

  const handleVolumeChange = (newVol: number) => {
    setVolume(newVol);
    try {
      localStorage.setItem("rockserver_player_volume", String(newVol));
    } catch {
      // localStorage may fail in restricted sandboxes
    }
  };

  // Search or fetch stations when active search or selected tag changes.
  // Note: Backend limits max items per request to 20.
  useEffect(() => {
    let active = true;
    const query = activeSearch.trim() || selectedTag || "rock";

    // Fast-path: return cached stations immediately if available in session
    const cached = stationCache.current.get(query);
    if (cached && cached.length > 0) {
      setStations(cached);
      setStationsLoading(false);
      setSearchError("");
      return;
    }

    const loadStations = async () => {
      setStationsLoading(true);
      setSearchError("");
      try {
        const res = await api.searchStations(query, 20);
        if (active) {
          const list = res.stations ?? [];
          setStations(list);
          if (list.length > 0) {
            stationCache.current.set(query, list);
          }
        }
      } catch (err: unknown) {
        if (active) {
          // Do NOT clear existing stations so cards do not disappear!
          const apiErr = err as { code?: string; message?: string; status?: number };
          const isRateLimited =
            apiErr?.code === "rate_limited" ||
            apiErr?.status === 429 ||
            (typeof apiErr?.message === "string" && apiErr.message.includes("rate limit"));
          if (isRateLimited) {
            setSearchError("Слишком частые запросы. Подождите несколько секунд перед следующим переключением.");
          } else {
            setSearchError("Не удалось загрузить станции. Попробуйте повторить запрос.");
          }
        }
      } finally {
        if (active) setStationsLoading(false);
      }
    };
    void loadStations();
    return () => {
      active = false;
    };
  }, [activeSearch, selectedTag]);

  const handleToggleFavorite = (stationId: string) => {
    setFavorites((prev) => {
      const next = prev.includes(stationId)
        ? prev.filter((id) => id !== stationId)
        : [...prev, stationId];
      try {
        localStorage.setItem("rockserver_player_favorites", JSON.stringify(next));
      } catch {
        // ignore storage error
      }
      return next;
    });
  };

  const handlePlayStation = (station: StationItem) => {
    setHistory((prev) => {
      const next = [station, ...prev.filter((s) => s.id !== station.id)].slice(0, 30);
      try {
        localStorage.setItem("rockserver_player_history", JSON.stringify(next));
      } catch {
        // ignore
      }
      return next;
    });

    if (currentStation?.id === station.id) {
      setIsPlaying(!isPlaying);
    } else {
      setCurrentStation(station);
      setIsPlaying(true);
      setTrackTitle(station.tags.slice(0, 2).join(" · ") || "Прямой эфир");
    }
  };

  const handleTogglePlay = () => {
    if (!currentStation && stations.length > 0) {
      handlePlayStation(stations[0]);
    } else {
      setIsPlaying(!isPlaying);
    }
  };

  const displayedStations =
    activeTab === "favorites"
      ? stations.filter((s) => favorites.includes(s.id))
      : activeTab === "history"
      ? history
      : stations;

  const handleNextStation = () => {
    if (!displayedStations.length) return;
    const currentIndex = currentStation ? displayedStations.findIndex((s) => s.id === currentStation.id) : -1;
    const nextIndex = (currentIndex + 1) % displayedStations.length;
    handlePlayStation(displayedStations[nextIndex]);
  };

  const handlePrevStation = () => {
    if (!displayedStations.length) return;
    const currentIndex = currentStation ? displayedStations.findIndex((s) => s.id === currentStation.id) : 0;
    const prevIndex = (currentIndex - 1 + displayedStations.length) % displayedStations.length;
    handlePlayStation(displayedStations[prevIndex]);
  };

  if (accountState === "loading")
    return (
      <main aria-busy="true">
        <header>
          <span>ROCK</span>
          <h1>Rock-аккаунт</h1>
        </header>
        <section role="status">
          <h2>Загружаем аккаунт…</h2>
          <p>Проверяем вход в этом браузере.</p>
        </section>
      </main>
    );
  if (accountState === "unavailable")
    return (
      <main>
        <header>
          <span>ROCK</span>
          <h1>Rock-аккаунт</h1>
        </header>
        <section role="alert">
          <h2>Сервис временно недоступен</h2>
          <p>{accountMessage || "Попробуйте обновить данные позже."}</p>
          <button onClick={onRetry}>Повторить</button>
        </section>
      </main>
    );
  if (accountState === "expired")
    return (
      <main>
        <header>
          <span>ROCK</span>
          <h1>Rock-аккаунт</h1>
        </header>
        <section role="alert">
          <h2>Сессия браузера завершена</h2>
          <p>Войдите с passkey ещё раз, чтобы увидеть устройства.</p>
          <button onClick={onAuthenticate} disabled={authBusy}>
            {authBusy ? "Проверяем…" : "Войти с passkey"}
          </button>
        </section>
      </main>
    );
  if (accountState === "anonymous")
    return (
      <main>
        <header>
          <span>ROCK</span>
          <h1>Rock-аккаунт</h1>
        </header>
        <section>
          <h2>Вы не вошли</h2>
          <p>
            Вход открывает существующий Rock-аккаунт. Создание аккаунта создаёт новый аккаунт с passkey.
          </p>
          <button onClick={onAuthenticate} disabled={authBusy}>
            {authBusy ? "Проверяем…" : "Войти с passkey"}
          </button>
          <button className="secondary" onClick={onRegister} disabled={authBusy}>
            Создать Rock-аккаунт
          </button>
          {accountMessage && <p role="status">{accountMessage}</p>}
        </section>
        <footer>Passkey и данные сессии не сохраняются в браузере.</footer>
      </main>
    );
  if (!account) return null;

  return (
    <div class="cabinet-layout">
      {/* Top Application Header */}
      <Header
        accountName={accountName}
        searchQuery={searchQuery}
        onSearchChange={handleSearchChange}
        onSearchSubmit={handleSearchSubmit}
        onLogout={onLogout}
        logoutBusy={logoutBusy}
      />

      <main class="cabinet-main-grid">
        {/* Left Column: Navigation & Presets */}
        <SidebarNav
          activeTab={activeTab}
          onTabChange={setActiveTab}
          selectedTag={selectedTag}
          onTagSelect={handleTagSelect}
          favoritesCount={favorites.length}
        />

        {/* Center Column: Tuner & Station Grid OR Hardware Devices view */}
        <div class="cabinet-center-col">
          {accountMessage && (
            <p class="account-alert" role="status">
              {accountMessage}
            </p>
          )}

          <p class="badge" role="status">
            ✓ Выполнен вход в браузере
          </p>

          {activeTab === "devices" ? (
            <div class="cabinet-center-devices">
              <HardwareHud
                account={account}
                deviceBusy={deviceBusy}
                justConnected={justConnected}
                onRename={onRename}
                onRevoke={onRevoke}
              />
              <YandexHomeCard
                account={account}
                csrf={csrf}
                onChanged={onRetry}
                initialStatus={yandexHomeStatus}
              />
            </div>
          ) : (
            <StationsView
              stations={displayedStations}
              activeStationId={currentStation?.id}
              isPlaying={isPlaying}
              currentTrackTitle={trackTitle}
              onPlayStation={handlePlayStation}
              favorites={favorites}
              onToggleFavorite={handleToggleFavorite}
              loading={stationsLoading}
              searchQuery={searchQuery}
              onSearchChange={handleSearchChange}
              onSearchSubmit={handleSearchSubmit}
              onSearchClear={handleSearchClear}
              searchError={searchError}
              selectedTag={selectedTag}
              activeTab={activeTab}
            />
          )}
        </div>

        {/* Right Column: Hardware & Smart Home HUD */}
        <aside class="cabinet-right-col">
          {activeTab !== "devices" ? (
            <>
              <HardwareHud
                account={account}
                deviceBusy={deviceBusy}
                justConnected={justConnected}
                onRename={onRename}
                onRevoke={onRevoke}
              />
              <YandexHomeCard
                account={account}
                csrf={csrf}
                onChanged={onRetry}
                initialStatus={yandexHomeStatus}
              />
            </>
          ) : (
            <div class="deck-panel guide-deck">
              <div class="panel-kicker">Управление устройствами</div>
              <p class="sync-desc">
                Здесь отображаются все подключённые устройства Windows RockCast и RockMobile, а также интеграция с умным домом Яндекс.
              </p>
            </div>
          )}
        </aside>
      </main>

      {/* Persistent Bottom Audio Player Deck */}
      <PlayerDeck
        currentStation={currentStation}
        isPlaying={isPlaying}
        trackTitle={trackTitle}
        onTogglePlay={handleTogglePlay}
        onNextStation={handleNextStation}
        onPrevStation={handlePrevStation}
        volume={volume}
        onVolumeChange={handleVolumeChange}
        onTrackTitleChange={setTrackTitle}
        onPlaybackStateChange={setIsPlaying}
        isFavorite={currentStation ? favorites.includes(currentStation.id) : false}
        onToggleFavorite={() =>
          currentStation && handleToggleFavorite(currentStation.id)
        }
      />

      <footer class="cabinet-footer">
        Passkey и данные сессии не сохраняются в браузере.
      </footer>
    </div>
  );
}
