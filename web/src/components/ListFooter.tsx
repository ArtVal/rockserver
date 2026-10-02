import { useEffect, useRef } from "preact/hooks";

/** Paging contract shared by catalog pages (network) and personal lists (local slices). */
export interface ListPaging {
  /** Stations currently rendered in the list. */
  shownCount: number;
  /** Full size of the underlying list: server total or synced records count. */
  total: number | undefined;
  hasMore: boolean;
  loadingMore: boolean;
  /** Error of the next-page request; never hides already shown stations. */
  error: string;
  /** True when the server search offset boundary stopped paging. */
  boundary: boolean;
  /** Noun for the end-of-list line: "станции" for catalog, "записи" for personal lists. */
  itemLabel: string;
  onLoadMore: () => void;
}

/**
 * Sentinel-driven infinite list footer. When the sentinel approaches the
 * viewport it preloads the next portion; the footer always exposes the manual
 * "Загрузить ещё" action plus loading, retry, end-of-list, and server-boundary
 * states. Re-observing after each applied portion keeps a still-visible
 * sentinel loading the next one.
 */
export function ListFooter({ paging }: { paging: ListPaging }) {
  const sentinelRef = useRef<HTMLDivElement>(null);
  // Keeps the observer effect free of callback identity while calls stay current.
  const loadMoreRef = useRef(paging.onLoadMore);
  loadMoreRef.current = paging.onLoadMore;
  const idle = paging.hasMore && !paging.loadingMore && !paging.error;

  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!idle || !sentinel || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMoreRef.current();
      },
      { rootMargin: "600px 0px" }
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [idle, paging.shownCount, paging.hasMore]);

  return (
    <div class="list-footer-wrap">
      <div ref={sentinelRef} class="load-more-sentinel" aria-hidden="true" />
      <div class="list-footer" role="status" aria-live="polite">
        {paging.loadingMore && (
          <p class="list-footer-loading">
            <span class="list-spinner" aria-hidden="true" />
            Загрузка станций…
          </p>
        )}
        {!paging.loadingMore && paging.error && (
          <div class="list-footer-error">
            <span>{paging.error}</span>
            <button type="button" class="load-more-btn" onClick={() => loadMoreRef.current()}>
              Повторить
            </button>
          </div>
        )}
        {idle && (
          <div class="list-footer-more">
            {paging.total !== undefined && (
              <span class="list-footer-counter">
                Показано {paging.shownCount} из {paging.total}
              </span>
            )}
            <button type="button" class="load-more-btn" onClick={() => loadMoreRef.current()}>
              Загрузить ещё
            </button>
          </div>
        )}
        {!paging.hasMore && !paging.error && (
          <p class={paging.boundary ? "list-footer-boundary" : "list-footer-end"}>
            {paging.boundary
              ? "Достигнут предел постраничного поиска (10 000). Уточните запрос, чтобы увидеть другие станции."
              : `Показаны все ${paging.itemLabel} (${paging.shownCount})`}
          </p>
        )}
      </div>
    </div>
  );
}
