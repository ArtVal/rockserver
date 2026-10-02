import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import { api, type BrowserAccount, type BrowserDevice, type StationItem } from "../api";
import { usePersonalSync } from "../usePersonalSync";
import { Header } from "./Header";
import { SidebarNav, type NavTab } from "./SidebarNav";
import { StationsView } from "./StationsView";
import { HardwareHud, type JustConnected } from "./HardwareHud";
import { YandexHomeCard } from "./YandexHomeCard";
import { PlayerDeck } from "./PlayerDeck";

const GENRE_PRESETS = ["all", "rock", "electronic", "synthwave", "jazz", "classical", "ambient"];

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
  initialTab?: NavTab;
  justConnected?: JustConnected;
  yandexHomeStatus?: string;
  onAuthenticate: () => Promise<void>;
  onRegister: () => void;
  onRetry: () => Promise<void>;
  onRename: (device: BrowserDevice) => Promise<void>;
  onRevoke: (device: BrowserDevice) => Promise<void>;
  onLogout: () => Promise<void>;
}

/** Gate screen shown while the browser session is loading, missing, expired, or unreachable. */
function AccountGate({
  eyebrow,
  title,
  children,
  role,
}: {
  eyebrow: string;
  title: string;
  children: ComponentChildren;
  role?: "status" | "alert";
}) {
  return (
    <main class="account-gate">
      <section class="deck-panel account-gate-panel" role={role}>
        <p class="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
        {children}
      </section>
      <footer className="cabinet-footer">Passkey и данные сессии не сохраняются в браузере.</footer>
    </main>
  );
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
  initialTab = "stations",
  justConnected,
  yandexHomeStatus,
  onAuthenticate,
  onRegister,
  onRetry,
  onRename,
  onRevoke,
  onLogout,
}: AccountCentreProps) {
  const [activeTab, setActiveTab] = useState<NavTab>(initialTab);
  const [searchQuery, setSearchQuery] = useState("");
  const [activeSearch, setActiveSearch] = useState("");
  const [selectedTag, setSelectedTag] = useState("");
  const [searchError, setSearchError] = useState("");
  const [searchAttempt, setSearchAttempt] = useState(0);

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
    setSearchAttempt((attempt) => attempt + 1);
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
  const {
    favorites,
    favoriteStations,
    history,
    toggleFavorite,
    recordPlay,
  } = usePersonalSync(csrf);
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
  }, [activeSearch, selectedTag, searchAttempt]);

  const handleToggleFavorite = (stationId: string) => {
    const stationItem =
      stations.find((s) => s.id === stationId) ||
      favoriteStations.find((s) => s.id === stationId) ||
      history.find((s) => s.id === stationId) ||
      (currentStation?.id === stationId ? currentStation : undefined);
    toggleFavorite(stationId, stationItem);
  };

  const handlePlayStation = (station: StationItem) => {
    recordPlay(station);

    if (currentStation?.id === station.id) {
      setIsPlaying(!isPlaying);
    } else {
      setCurrentStation(station);
      setIsPlaying(true);
      // Statuses like "connecting" are rendered by PlayerDeck; an empty title
      // keeps the honest "Прямой эфир" fallback until real metadata arrives.
      setTrackTitle("");
    }
  };

  const handleTogglePlay = () => {
    if (!currentStation && stations.length > 0) {
      handlePlayStation(stations[0]);
    } else {
      setIsPlaying(!isPlaying);
    }
  };

  const filterBySearch = (list: StationItem[]) => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return list;
    return list.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        (s.tags && s.tags.some((t) => t.toLowerCase().includes(q)))
    );
  };

  // Build the list from the same IDs that drive each favorite button and count.
  const knownFavorites = new Map(
    [...favoriteStations, ...history, ...stations, ...(currentStation ? [currentStation] : [])]
      .map((station) => [station.id, station] as const)
  );
  const savedStations = favorites.map((id) => knownFavorites.get(id) ?? {
    id, name: "Станция без названия", tags: [],
  });

  const displayedStations =
    activeTab === "favorites"
      ? filterBySearch(savedStations)
      : activeTab === "history"
      ? filterBySearch(history)
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
      <AccountGate eyebrow="Rock-аккаунт" title="Загружаем аккаунт…" role="status">
        <p>Проверяем вход в этом браузере.</p>
      </AccountGate>
    );
  if (accountState === "unavailable")
    return (
      <AccountGate eyebrow="Rock-аккаунт" title="Сервис временно недоступен" role="alert">
        <p>{accountMessage || "Попробуйте обновить данные позже."}</p>
        <button onClick={onRetry}>Повторить</button>
      </AccountGate>
    );
  if (accountState === "expired")
    return (
      <AccountGate eyebrow="Rock-аккаунт" title="Сессия браузера завершена" role="alert">
        <p>Войдите с passkey ещё раз, чтобы увидеть устройства.</p>
        <button onClick={onAuthenticate} disabled={authBusy}>
          {authBusy ? "Проверяем…" : "Войти с passkey"}
        </button>
      </AccountGate>
    );
  if (accountState === "anonymous")
    return (
      <AccountGate eyebrow="Rock-аккаунт" title="Вы не вошли">
        <p>
          Вход открывает существующий Rock-аккаунт. Создание аккаунта создаёт новый аккаунт с passkey.
        </p>
        <div class="gate-actions">
          <button onClick={onAuthenticate} disabled={authBusy}>
            {authBusy ? "Проверяем…" : "Войти с passkey"}
          </button>
          <button className="secondary" onClick={onRegister} disabled={authBusy}>
            Создать Rock-аккаунт
          </button>
        </div>
        {accountMessage && <p role="status">{accountMessage}</p>}
      </AccountGate>
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
        onSearchClear={handleSearchClear}
        onLogout={onLogout}
        logoutBusy={logoutBusy}
      />

      <main class="cabinet-main-grid">
        <SidebarNav
          activeTab={activeTab}
          onTabChange={setActiveTab}
        />

        <div class="cabinet-center-col">
          {accountMessage && (
            <p class="account-alert" role="status">
              {accountMessage}
            </p>
          )}

          {activeTab === "devices" ? (
            <div class="cabinet-center-devices">
              <div class="deck-panel devices-banner">
                <div class="tuner-banner-glow" aria-hidden="true" />
                <p class="eyebrow">Аккаунт и подключения</p>
                <h1 class="devices-title">Устройства</h1>
                <p class="tuner-subtitle">RockCast, RockMobile и Яндекс Дом</p>
              </div>
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
            <>
              {activeTab === "stations" && (
                <div class="genre-presets-grid" aria-label="Жанры">
                  {GENRE_PRESETS.map((tag) => (
                    <button
                      key={tag}
                      type="button"
                      class={`preset-chip ${selectedTag === tag || (!selectedTag && tag === "all") ? "selected" : ""}`}
                      onClick={() => handleTagSelect(tag === "all" ? "" : tag)}
                    >
                      #{tag}
                    </button>
                  ))}
                </div>
              )}
              <StationsView
              stations={displayedStations}
              activeStationId={currentStation?.id}
              isPlaying={isPlaying}
              currentTrackTitle={trackTitle}
              onPlayStation={handlePlayStation}
              favorites={favorites}
              onToggleFavorite={handleToggleFavorite}
              loading={activeTab === "stations" && stationsLoading}
              searchQuery={searchQuery}
              onSearchSubmit={handleSearchSubmit}
              searchError={activeTab === "stations" ? searchError : ""}
              selectedTag={selectedTag}
              activeTab={activeTab}
              favoriteCount={savedStations.length}
              />
            </>
          )}
        </div>
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
