import { useEffect, useState } from "preact/hooks";
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
  const [selectedTag, setSelectedTag] = useState("");
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

  // Search or fetch stations when query or selected tag changes
  useEffect(() => {
    let active = true;
    const loadStations = async () => {
      setStationsLoading(true);
      try {
        const query = searchQuery.trim() || selectedTag || "rock";
        const res = await api.searchStations(query, 24);
        if (active) setStations(res.stations ?? []);
      } catch {
        if (active) setStations([]);
      } finally {
        if (active) setStationsLoading(false);
      }
    };
    void loadStations();
    return () => {
      active = false;
    };
  }, [searchQuery, selectedTag]);

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

  const handleNextStation = () => {
    const list = activeTab === "favorites" ? stations.filter((s) => favorites.includes(s.id)) : stations;
    if (!list.length) return;
    const currentIndex = currentStation ? list.findIndex((s) => s.id === currentStation.id) : -1;
    const nextIndex = (currentIndex + 1) % list.length;
    handlePlayStation(list[nextIndex]);
  };

  const handlePrevStation = () => {
    const list = activeTab === "favorites" ? stations.filter((s) => favorites.includes(s.id)) : stations;
    if (!list.length) return;
    const currentIndex = currentStation ? list.findIndex((s) => s.id === currentStation.id) : 0;
    const prevIndex = (currentIndex - 1 + list.length) % list.length;
    handlePlayStation(list[prevIndex]);
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

  const displayedStations =
    activeTab === "favorites"
      ? stations.filter((s) => favorites.includes(s.id))
      : stations;

  return (
    <div class="cabinet-layout">
      {/* Top Application Header */}
      <Header
        accountName={accountName}
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        onLogout={onLogout}
        logoutBusy={logoutBusy}
      />

      <main class="cabinet-main-grid">
        {/* Left Column: Navigation & Presets */}
        <SidebarNav
          activeTab={activeTab}
          onTabChange={setActiveTab}
          selectedTag={selectedTag}
          onTagSelect={setSelectedTag}
          favoritesCount={favorites.length}
        />

        {/* Center Column: Tuner & Station Grid */}
        <div class="cabinet-center-col">
          {accountMessage && (
            <p class="account-alert" role="status">
              {accountMessage}
            </p>
          )}

          <p class="badge" role="status">
            ✓ Выполнен вход в браузере
          </p>

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
            selectedTag={selectedTag}
          />
        </div>

        {/* Right Column: Hardware & Smart Home HUD */}
        <aside class="cabinet-right-col">
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
