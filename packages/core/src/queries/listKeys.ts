/**
 * Generic Pinia Colada query-key factory for paginated/infinite list
 * pages. Per-feature modules (`./tickets.ts`, `./users.ts`,
 * `./devices.ts`) instantiate this with their own root segment so
 * the key shape is identical across features and the *only* thing
 * that varies is the root namespace.
 *
 * Both list views and Vue Router Data Loaders must build the same
 * key for a given (variant, cacheKey, page) tuple, otherwise the
 * loader-primed cache entry is silently orphaned and the view
 * refetches over the prime on mount. Centralising the builder here
 * is the simplest way to keep the two in sync.
 */

export type ListVariant = 'infinite' | 'paginated'

export interface ListKeys<R extends string> {
  /** All entries under this feature, e.g. for invalidation. */
  root: readonly [R]
  /**
   * Hierarchical key for one variant + cache-key-part + optional page.
   * Page is undefined for infinite lists (the page rides in
   * `pageParam`); paginated lists include the current page so each
   * page has its own cache entry.
   */
  list: (variant: ListVariant, cacheKey: string, page?: number) =>
    | readonly [R, 'list', ListVariant, string]
    | readonly [R, 'list', ListVariant, string, number]
}

export function listKeys<R extends string>(root: R): ListKeys<R> {
  const rootKey = [root] as const
  return {
    root: rootKey,
    list: (variant, cacheKey, page) =>
      page === undefined
        ? ([...rootKey, 'list', variant, cacheKey] as const)
        : ([...rootKey, 'list', variant, cacheKey, page] as const),
  }
}
