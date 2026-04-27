/**
 * Users list query layer. Owns the cache-key family the view
 * (`useInfiniteQuery` in `UsersListView.vue`) subscribes to.
 *
 * No data loader yet, but the same key-builder convention is in
 * place so the moment one is added it can call `usersKeys.list(...)`
 * directly without rolling its own array.
 */
import { listKeys } from './listKeys'

export const usersKeys = listKeys('users')
