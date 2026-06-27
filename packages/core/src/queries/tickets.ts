/**
 * Tickets list query layer. Owns the cache-key family the view
 * (`useInfiniteQuery` in `TicketsListView.vue`) and the data
 * loader (`ticketsListLoader.ts`) both subscribe to. They MUST
 * agree on the key, otherwise the loader's cache prime is silently
 * orphaned and the view fires a duplicate request on mount.
 *
 * To keep them honest:
 *  - `ticketsKeys.list(...)` is the only place the key array is
 *    constructed.
 *  - `ticketsListParamsFromQuery()` is the only place the
 *    URL query is converted into the params shape that
 *    `serializeListCacheKey` accepts.
 */
// Structural equivalent of vue-router's `LocationQuery` (parsed URL query
// params), inlined so @nosdesk/core carries no router dependency. The frontend
// passes vue-router's own `LocationQuery`, which is assignable to this shape.
type LocationQueryValue = string | null
type LocationQuery = Record<string, LocationQueryValue | LocationQueryValue[]>

import { listKeys } from './listKeys'
import type { ListRequestParams } from './listSerialization'

export const ticketsKeys = listKeys('tickets')

/** URL query keys the tickets list view URL-syncs. Keep in sync
 *  with the filter pickers in `TicketsListView.vue`. */
export const TICKETS_FILTER_PARAM_KEYS = [
  'status',
  'priority',
  'category',
  'assignee',
  'requester',
  'createdOn',
  'createdAfter',
  'createdBefore',
  'modifiedOn',
  'modifiedAfter',
  'modifiedBefore',
  'closedOn',
  'closedAfter',
  'closedBefore',
] as const

export interface TicketsListParamsFromQueryOptions {
  /** Resolved page size (the loader translates the UI's `0`
   *  ("All / infinite") sentinel to the real chunk size before
   *  calling). */
  pageSize: number
}

/**
 * Convert a `LocationQuery` (URL params) into the
 * `ListRequestParams` shape that `serializeListCacheKey()` accepts.
 * Used by the data loader, which doesn't have a `useListControls`
 * instance to read from.
 *
 * Defaults must match `useListControls`'s defaults so the loader
 * primes the same key the view's first request will produce.
 */
export function ticketsListParamsFromQuery(
  query: LocationQuery,
  options: TicketsListParamsFromQueryOptions,
): ListRequestParams {
  const str = (key: string): string | undefined => {
    const v = query[key]
    return typeof v === 'string' ? v : undefined
  }

  const params: ListRequestParams = {
    page: 1,
    pageSize: options.pageSize,
    sortField: str('sortField') ?? 'id',
    sortDirection: (str('sortDirection') as 'asc' | 'desc') ?? 'desc',
    search: str('search') ?? '',
  }

  for (const key of TICKETS_FILTER_PARAM_KEYS) {
    const value = str(key)
    if (value && value !== 'all') {
      params[key] = value.toLowerCase()
    }
  }

  return params
}
