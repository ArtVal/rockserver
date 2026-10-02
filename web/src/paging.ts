import type { SearchResponse, StationItem } from "./api";

/**
 * Maximum offset accepted by the public search endpoint; the server rejects any
 * larger value, so paging stops here instead of sending doomed requests.
 */
export const MAX_SEARCH_OFFSET = 10_000;

/** Result of folding one server search page into an accumulated issuance. */
export interface MergedPage {
  /** Accumulated stations with duplicate IDs removed. */
  stations: StationItem[];
  /** Stations from the incoming page that were new (used for icon prefetch). */
  addedCount: number;
  /** Offset for the next request, counted in server page sizes, not unique rows. */
  serverOffset: number;
  /** Whether another page request can return more data. */
  hasMore: boolean;
  /** Total matches reported by the server, when known. */
  total?: number;
  /** True when the server offset boundary stopped paging while more data may exist. */
  boundary: boolean;
}

/**
 * Merges one server search page into the current issuance list.
 *
 * Invariants: duplicate station IDs never appear twice, yet `serverOffset`
 * advances by the full server page length so subsequent offsets stay aligned
 * with server-side paging. An empty page always ends paging regardless of the
 * reported `has_more`, preventing endless requests. Crossing
 * {@link MAX_SEARCH_OFFSET} with more data reported sets `boundary` and stops
 * paging instead of silently lifting the server limit.
 */
export function mergeServerPage(
  current: StationItem[],
  response: SearchResponse,
  previousOffset: number
): MergedPage {
  const page = response.stations ?? [];
  const seen = new Set(current.map((station) => station.id));
  const added = page.filter((station) => !seen.has(station.id));
  const serverOffset = previousOffset + page.length;
  const total = typeof response.total === "number" ? response.total : undefined;
  let hasMore = response.has_more ?? (total !== undefined && serverOffset < total);
  if (page.length === 0) hasMore = false;
  const boundary = hasMore && serverOffset > MAX_SEARCH_OFFSET;
  if (boundary) hasMore = false;
  return {
    stations: [...current, ...added],
    addedCount: added.length,
    serverOffset,
    hasMore,
    total,
    boundary,
  };
}
