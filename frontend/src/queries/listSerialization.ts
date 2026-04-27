/**
 * Pure serialisation of list-view request params into the cache-key
 * string that `useInfiniteQuery({ key: [..., cacheKey] })` uses.
 *
 * `useListControls.cacheKeyPart` and the Vue Router Data Loaders
 * both call into this so a loader-primed cache entry hits the same
 * key the view's query produces. Drift here meant the loader's
 * prime was silently orphaned and the view refetched on mount.
 *
 * Pure: no Vue reactivity, no DOM, no localStorage. Safe to call
 * from a loader (which runs outside the Vue component tree) and
 * from a composable (which runs inside it).
 */

/** The shape `useListControls.requestParams` produces. Filter keys
 *  are domain-specific (e.g. `status`, `priority`, `role`) and live
 *  alongside the standard keys, so this is intentionally open. */
export interface ListRequestParams {
  page: number
  pageSize: number
  sortField: string
  sortDirection: 'asc' | 'desc'
  search?: string
  [filterKey: string]: string | number | undefined
}

/**
 * Serialise the params Pinia Colada uses as the variable part of
 * the query key. `page` is excluded: infinite queries carry the
 * page in `pageParam`, paginated queries carry it as a separate
 * trailing key segment via `listKeys.list(..., page)`. Including it
 * here too would double-count and break key matching.
 */
export function serializeListCacheKey(params: ListRequestParams): string {
  const { page: _page, ...rest } = params
  // Drop empty search so `?search=` and no search key serialise the
  // same. Keeps cache hits stable when users clear the input.
  if (rest.search === '') delete rest.search
  return JSON.stringify(rest)
}
