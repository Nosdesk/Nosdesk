/**
 * Reactive UUID → user resolver.
 *
 * The sync engine doesn't pool users (no `user` SyncAggregate
 * in models.rs), so the tickets table can't `useReference()`
 * its assignee / requester ids. This composable bridges the
 * gap: a `getUser(uuid)` call returns a reactive computed that
 * starts as null and resolves to the User row once the
 * dataStore cache has it.
 *
 * Two pieces of cross-instance state live at module scope:
 *
 * - `computedCache` memoises the per-uuid computed so two
 *   call sites for the same uuid share one effect rather than
 *   spinning up a redundant pair. This matters when the same
 *   user appears across many table rows — without sharing,
 *   N rows would create N parallel computeds for one fact.
 *
 * - `requested` dedupes the lazy fetch. The dataStore already
 *   batches + dedupes its REST calls, but this set keeps us
 *   from even calling `getUserByUuid` more than once per uuid
 *   per session, which keeps the call graph noise-free.
 *
 * Failure is silent on purpose: the cell degrades to '?'
 * initials when the cache stays empty. Surfacing per-row
 * fetch errors in the table would just be visual noise.
 */
import { computed, type ComputedRef } from 'vue'
import { useDataStore } from '@/stores/dataStore'
import type { User } from '@/types/user'

export type UserStatus = 'loading' | 'resolved' | 'missing'

export interface UserHandle {
  user: ComputedRef<User | null>
  status: ComputedRef<UserStatus>
}

const handleCache = new Map<string, UserHandle>()
const requested = new Set<string>()

export function useUsersDirectory() {
  const dataStore = useDataStore()

  /** Reactive handle for a uuid: `user` resolves to the User row
   * when the cache has it (null otherwise), and `status`
   * distinguishes `loading` / `resolved` / `missing`. Consumers
   * should bind on `status` to decide between rendering a
   * skeleton (loading) vs a fallback (missing) vs the user
   * (resolved). Without the status split, a fetch that completes
   * with "user not found" leaves consumers stuck in a skeleton
   * forever because `user` stays null indistinguishably from
   * the in-flight state. */
  function getUserHandle(uuid: string): UserHandle {
    let handle = handleCache.get(uuid)
    if (!handle) {
      // Both computeds read from the dataStore's reactive Map,
      // so they re-evaluate when the fetch lands without any
      // explicit subscription dance.
      handle = {
        user: computed<User | null>(
          () => dataStore.getCachedUserByUuid(uuid) ?? null,
        ),
        status: computed<UserStatus>(() => dataStore.getUserStatus(uuid)),
      }
      handleCache.set(uuid, handle)
    }
    if (!requested.has(uuid)) {
      requested.add(uuid)
      void dataStore.getUserByUuid(uuid).catch(() => {
        // Errors logged inside the store; the handle's `status`
        // composable surfaces the missing state to callers.
      })
    }
    return handle
  }

  /** Back-compat: original API returned just the user computed.
   * New call sites should prefer `getUserHandle` so they can
   * distinguish loading from missing. */
  function getUser(uuid: string): ComputedRef<User | null> {
    return getUserHandle(uuid).user
  }

  return { getUser, getUserHandle }
}
