import { useEffect, useRef, useState } from "preact/hooks";
import { api, type StationItem } from "./api";
import { MAX_SEARCH_OFFSET, mergeServerPage, type MergedPage } from "./paging";

/** Page size for catalog requests; the public search endpoint caps a page at 20. */
const PAGE_SIZE = 20;

type PagesState = {
  stations: StationItem[];
  loading: boolean;
  loadingMore: boolean;
  error: string;
  moreError: string;
  hasMore: boolean;
  total?: number;
  serverOffset: number;
  boundary: boolean;
};

const initialPagesState: PagesState = {
  stations: [],
  loading: false,
  loadingMore: false,
  error: "",
  moreError: "",
  hasMore: false,
  total: undefined,
  serverOffset: 0,
  boundary: false,
};

/** Maps a failed page request to a short Russian message, mirroring the search banner. */
const pageErrorMessage = (error: unknown) => {
  const apiError = error as { code?: string; message?: string; status?: number };
  const rateLimited =
    apiError?.code === "rate_limited" ||
    apiError?.status === 429 ||
    (typeof apiError?.message === "string" && apiError.message.includes("rate limit"));
  return rateLimited
    ? "Слишком частые запросы. Подождите несколько секунд перед следующим переключением."
    : "Не удалось загрузить станции. Попробуйте повторить запрос.";
};

/**
 * Loads a catalog issuance page by page with infinite-scroll semantics.
 *
 * Guarantees: only one page request runs at a time (a ref guard covers the gap
 * before re-render), responses of a superseded query are dropped via a
 * generation counter, the next offset is counted in server page sizes (see
 * {@link mergeServerPage}), already shown stations are never cleared by a
 * next-page failure, and completed issuances are cached per query so returning
 * to them restores the loaded pages without refetching.
 */
export function useStationPages(query: string, attempt: number) {
  const [state, setState] = useState<PagesState>(initialPagesState);
  const cache = useRef<Map<string, MergedPage>>(new Map());
  // Monotonic issuance generation; late responses compare against it and drop themselves.
  const generation = useRef(0);
  // Single in-flight guard that also holds between loadMore() and the state update.
  const busy = useRef(false);
  const stateRef = useRef(state);
  stateRef.current = state;
  const queryRef = useRef(query);
  queryRef.current = query;
  const lastRun = useRef({ query: "", attempt: -1 });

  const loadPage = async (targetQuery: string, offset: number, base: StationItem[], gen: number) => {
    busy.current = true;
    try {
      const response = await api.searchStations(targetQuery, PAGE_SIZE, offset);
      if (gen !== generation.current) return;
      const merged = mergeServerPage(base, response, offset);
      cache.current.set(targetQuery, merged);
      setState((previous) => ({
        ...previous,
        stations: merged.stations,
        loading: false,
        loadingMore: false,
        error: "",
        moreError: "",
        hasMore: merged.hasMore,
        total: merged.total,
        serverOffset: merged.serverOffset,
        boundary: merged.boundary,
      }));
    } catch (error) {
      if (gen !== generation.current) return;
      const message = pageErrorMessage(error);
      setState((previous) =>
        offset === 0
          ? { ...previous, loading: false, error: message }
          : { ...previous, loadingMore: false, moreError: message }
      );
    } finally {
      busy.current = false;
    }
  };

  useEffect(() => {
    // A repeated run with the same query but a new attempt is an explicit retry:
    // drop the cached snapshot and refetch from the first page.
    const retry = lastRun.current.query === query && lastRun.current.attempt !== attempt;
    lastRun.current = { query, attempt };
    generation.current += 1;
    busy.current = false;

    const snapshot = retry ? undefined : cache.current.get(query);
    if (snapshot) {
      setState({
        stations: snapshot.stations,
        loading: false,
        loadingMore: false,
        error: "",
        moreError: "",
        hasMore: snapshot.hasMore,
        total: snapshot.total,
        serverOffset: snapshot.serverOffset,
        boundary: snapshot.boundary,
      });
      return;
    }
    if (retry) cache.current.delete(query);
    setState({ ...initialPagesState, loading: true, hasMore: true });
    void loadPage(query, 0, [], generation.current);
  }, [query, attempt]);

  /** Loads the next page of the current issuance; guarded to one request at a time. */
  const loadMore = () => {
    const current = stateRef.current;
    if (current.loading || current.loadingMore || !current.hasMore || busy.current) return;
    if (current.serverOffset > MAX_SEARCH_OFFSET) return;
    setState((previous) => ({ ...previous, loadingMore: true, moreError: "" }));
    void loadPage(queryRef.current, current.serverOffset, current.stations, generation.current);
  };

  return {
    stations: state.stations,
    loading: state.loading,
    loadingMore: state.loadingMore,
    error: state.error,
    moreError: state.moreError,
    hasMore: state.hasMore,
    total: state.total,
    boundary: state.boundary,
    loadMore,
  };
}
