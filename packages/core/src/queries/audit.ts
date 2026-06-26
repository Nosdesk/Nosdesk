import { listKeys } from './listKeys'

/**
 * Query-key family for the unified audit feed. Cursor-paginated via
 * Pinia Colada `useInfiniteQuery`, so only the `infinite` variant is
 * used; the cache key encodes the active filter set.
 */
export const auditKeys = listKeys('audit')
