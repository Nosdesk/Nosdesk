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

const computedCache = new Map<string, ComputedRef<User | null>>()
const requested = new Set<string>()

export function useUsersDirectory() {
  const dataStore = useDataStore()

  function getUser(uuid: string): ComputedRef<User | null> {
    let entry = computedCache.get(uuid)
    if (!entry) {
      // Reading from the dataStore's reactive Map inside this
      // computed wires the dep automatically — when the fetch
      // finishes and the row lands in the cache, this computed
      // re-evaluates and any consuming render updates in place.
      entry = computed<User | null>(
        () => dataStore.getCachedUserByUuid(uuid) ?? null,
      )
      computedCache.set(uuid, entry)
    }
    if (!requested.has(uuid)) {
      requested.add(uuid)
      void dataStore.getUserByUuid(uuid).catch(() => {
        // Errors logged inside the store; nothing useful to do
        // here besides letting the cell render its '?' state.
      })
    }
    return entry
  }

  return { getUser }
}
