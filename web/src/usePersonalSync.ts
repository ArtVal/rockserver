import { useEffect, useRef, useState } from "preact/hooks";
import { api, type BrowserSyncRequest, type StationItem } from "./api";

/** Generates a canonical UUID v4 string for sync records. */
export function makeUuid(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

/**
 * Manages favorites and history state synchronized with the server via LWW sync.
 * Falls back to local storage when unauthenticated or offline.
 */
export function usePersonalSync(csrf: string) {
  const [favorites, setFavorites] = useState<string[]>(() => {
    try {
      const stored = localStorage.getItem("rockserver_player_favorites");
      return stored ? (JSON.parse(stored) as string[]) : [];
    } catch {
      return [];
    }
  });

  const [favoriteStations, setFavoriteStations] = useState<StationItem[]>(() => {
    try {
      const stored = localStorage.getItem("rockserver_player_favorite_stations");
      return stored ? (JSON.parse(stored) as StationItem[]) : [];
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

  const favoriteRecordIds = useRef<Map<string, string>>(new Map());
  const serverRevision = useRef<number>(0);

  // Synchronize with server on authentication
  useEffect(() => {
    if (!csrf) return;
    let active = true;

    const pullAndMerge = async () => {
      try {
        const alreadyMigrated =
          localStorage.getItem("rockserver_initial_favorites_migrated") === "true";
        let initialUpserts:
          | Array<{ record_id: string; station_id: string; added_at: string; updated_at: string }>
          | undefined = undefined;

        if (!alreadyMigrated && favorites.length > 0) {
          const now = new Date().toISOString();
          initialUpserts = favorites.map((stationId) => {
            const recordId = makeUuid();
            favoriteRecordIds.current.set(stationId, recordId);
            return {
              record_id: recordId,
              station_id: stationId,
              added_at: now,
              updated_at: now,
            };
          });
          try {
            localStorage.setItem("rockserver_initial_favorites_migrated", "true");
          } catch {
            // ignore storage failure
          }
        }

        const req: BrowserSyncRequest = {
          since_revision: 0,
          ...(initialUpserts && initialUpserts.length > 0
            ? { favourites: { upserts: initialUpserts } }
            : {}),
        };

        const res = await api.browserSync(req, csrf);
        if (!active) return;

        serverRevision.current = res.server_revision;

        // Map live favourite records
        const liveFavRecordMap = new Map<string, string>();
        for (const rec of res.favourites.records) {
          if (!rec.deleted_at && rec.station_id) {
            liveFavRecordMap.set(rec.station_id, rec.record_id);
            favoriteRecordIds.current.set(rec.station_id, rec.record_id);
          }
        }

        const serverFavIds = Array.from(liveFavRecordMap.keys());
        setFavorites(serverFavIds);
        try {
          localStorage.setItem("rockserver_player_favorites", JSON.stringify(serverFavIds));
        } catch {
          // ignore
        }

        if (res.stations && res.stations.length > 0) {
          setFavoriteStations(res.stations);
          try {
            localStorage.setItem(
              "rockserver_player_favorite_stations",
              JSON.stringify(res.stations)
            );
          } catch {
            // ignore
          }
        } else if (serverFavIds.length === 0) {
          setFavoriteStations([]);
          try {
            localStorage.setItem("rockserver_player_favorite_stations", JSON.stringify([]));
          } catch {
            // ignore
          }
        }

        // Process server playback history
        if (res.history?.records && res.history.records.length > 0) {
          const stationsMap = new Map<string, StationItem>();
          if (res.stations) {
            for (const s of res.stations) {
              stationsMap.set(s.id, s);
            }
          }

          const historyList: StationItem[] = [];
          const seenHistoryIds = new Set<string>();
          const sortedHistory = [...res.history.records]
            .filter((r) => !r.deleted_at && r.station_id)
            .sort((a, b) => b.last_played_at.localeCompare(a.last_played_at));

          for (const rec of sortedHistory) {
            const stId = rec.station_id!;
            if (seenHistoryIds.has(stId)) continue;
            seenHistoryIds.add(stId);

            const fromStations = stationsMap.get(stId);
            if (fromStations) {
              historyList.push(fromStations);
            } else if (rec.metadata && typeof rec.metadata === "object") {
              const meta = rec.metadata as Record<string, unknown>;
              historyList.push({
                id: stId,
                name: (meta.name as string) || "Радиостанция",
                tags: Array.isArray(meta.tags) ? (meta.tags as string[]) : [],
                stream_url: typeof meta.stream_url === "string" ? meta.stream_url : undefined,
                favicon_url: typeof meta.favicon_url === "string" ? meta.favicon_url : undefined,
                homepage_url: typeof meta.homepage_url === "string" ? meta.homepage_url : undefined,
                codec: typeof meta.codec === "string" ? meta.codec : undefined,
                bitrate_kbps: typeof meta.bitrate_kbps === "number" ? meta.bitrate_kbps : undefined,
              });
            } else {
              const localMatch = history.find((h) => h.id === stId);
              if (localMatch) {
                historyList.push(localMatch);
              } else {
                historyList.push({ id: stId, name: stId, tags: [] });
              }
            }
          }

          if (historyList.length > 0) {
            setHistory(historyList);
            try {
              localStorage.setItem("rockserver_player_history", JSON.stringify(historyList));
            } catch {
              // ignore
            }
          }
        }
      } catch {
        // Silently preserve local state if network sync is unavailable
      }
    };

    void pullAndMerge();

    return () => {
      active = false;
    };
  }, [csrf]);

  const toggleFavorite = (stationId: string, stationItem?: StationItem) => {
    const isFav = favorites.includes(stationId);
    const now = new Date().toISOString();

    if (isFav) {
      setFavorites((prev) => {
        const next = prev.filter((id) => id !== stationId);
        try {
          localStorage.setItem("rockserver_player_favorites", JSON.stringify(next));
        } catch {
          // ignore
        }
        return next;
      });

      setFavoriteStations((prev) => {
        const next = prev.filter((s) => s.id !== stationId);
        try {
          localStorage.setItem(
            "rockserver_player_favorite_stations",
            JSON.stringify(next)
          );
        } catch {
          // ignore
        }
        return next;
      });

      const recordId = favoriteRecordIds.current.get(stationId);
      if (recordId) {
        favoriteRecordIds.current.delete(stationId);
      }
      if (csrf) {
        const targetRecordId = recordId || makeUuid();
        void api
          .browserSync(
            {
              favourites: {
                deletes: [{ record_id: targetRecordId, updated_at: now }],
              },
            },
            csrf
          )
          .catch(() => {});
      }
    } else {
      const recordId = makeUuid();
      favoriteRecordIds.current.set(stationId, recordId);

      setFavorites((prev) => {
        const next = [...prev, stationId];
        try {
          localStorage.setItem("rockserver_player_favorites", JSON.stringify(next));
        } catch {
          // ignore
        }
        return next;
      });

      if (stationItem) {
        setFavoriteStations((prev) => {
          const next = [stationItem, ...prev.filter((s) => s.id !== stationId)];
          try {
            localStorage.setItem(
              "rockserver_player_favorite_stations",
              JSON.stringify(next)
            );
          } catch {
            // ignore
          }
          return next;
        });
      }

      if (csrf) {
        void api
          .browserSync(
            {
              favourites: {
                upserts: [
                  {
                    record_id: recordId,
                    station_id: stationId,
                    added_at: now,
                    updated_at: now,
                  },
                ],
              },
            },
            csrf
          )
          .then((res) => {
            if (res.stations && res.stations.length > 0) {
              setFavoriteStations((prev) => {
                const map = new Map<string, StationItem>();
                for (const s of prev) map.set(s.id, s);
                for (const s of res.stations) map.set(s.id, s);
                const next = Array.from(map.values());
                try {
                  localStorage.setItem(
                    "rockserver_player_favorite_stations",
                    JSON.stringify(next)
                  );
                } catch {
                  // ignore
                }
                return next;
              });
            }
          })
          .catch(() => {});
      }
    }
  };

  const recordPlay = (station: StationItem) => {
    setHistory((prev) => {
      const next = [station, ...prev.filter((s) => s.id !== station.id)].slice(0, 50);
      try {
        localStorage.setItem("rockserver_player_history", JSON.stringify(next));
      } catch {
        // ignore
      }
      return next;
    });

    if (csrf) {
      const now = new Date().toISOString();
      const recordId = makeUuid();
      void api
        .browserSync(
          {
            history: {
              upserts: [
                {
                  record_id: recordId,
                  station_id: station.id,
                  started_at: now,
                  last_played_at: now,
                  updated_at: now,
                  metadata: {
                    name: station.name,
                    stream_url: station.stream_url,
                    tags: station.tags,
                    favicon_url: station.favicon_url,
                    homepage_url: station.homepage_url,
                    codec: station.codec,
                    bitrate_kbps: station.bitrate_kbps,
                  },
                },
              ],
            },
          },
          csrf
        )
        .catch(() => {});
    }
  };

  return {
    favorites,
    favoriteStations,
    history,
    toggleFavorite,
    recordPlay,
  };
}
