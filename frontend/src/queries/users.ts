/**
 * Users list query-key family. Owns the cache key the
 * `UsersListView`'s `useInfiniteQuery` (via useListPage)
 * subscribes to.
 *
 * Individual user lookups (`byUuid`) are NOT here — they're owned
 * by the sync engine's user pool (`backend/sync-models/user.json`),
 * read via `useUsersDirectory` / `useReference('user', uuid)`. The
 * Pinia Colada cache only owns the paginated/searched LIST shape,
 * which is independent of the per-row sync.
 */
import { listKeys } from './listKeys'

export const usersKeys = listKeys('users')
