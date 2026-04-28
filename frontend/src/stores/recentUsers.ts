import { ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useAuthStore } from '@/stores/auth'

/** A user the current account has picked recently. Stored locally so each
 *  signed-in user's recent list is theirs alone. */
export interface RecentUser {
  uuid: string
  name: string
  /** Wall-clock ms of the most recent pick. Most-recent-first ordering. */
  lastUsed: number
}

/** Picker context. Recents are scoped per-context so the assignee picker
 *  doesn't flood with people you've assigned as requesters and vice
 *  versa. */
export type RecentScope = 'assignee' | 'requester'

const MAX_PER_SCOPE = 20
const VISIBLE_PER_SCOPE = 5

const STORAGE_KEY_PREFIX = 'nosdesk:recent-users'

interface PersistedShape {
  assignee: RecentUser[]
  requester: RecentUser[]
}

const EMPTY: PersistedShape = { assignee: [], requester: [] }

/** Build the localStorage key for a given account uuid. Per-account so
 *  switching accounts on the same browser shows the new account's history,
 *  not the previous account's. Anonymous (signed-out) sessions use a
 *  fallback bucket that's wiped on next login. */
function storageKey(accountUuid: string | null): string {
  return accountUuid
    ? `${STORAGE_KEY_PREFIX}:${accountUuid}`
    : `${STORAGE_KEY_PREFIX}:anon`
}

function loadFromStorage(accountUuid: string | null): PersistedShape {
  if (typeof localStorage === 'undefined') return { ...EMPTY }
  try {
    const raw = localStorage.getItem(storageKey(accountUuid))
    if (!raw) return { ...EMPTY }
    const parsed = JSON.parse(raw) as Partial<PersistedShape>
    return {
      assignee: Array.isArray(parsed.assignee) ? parsed.assignee : [],
      requester: Array.isArray(parsed.requester) ? parsed.requester : [],
    }
  } catch {
    return { ...EMPTY }
  }
}

/** Per-account recent-user history. Two scoped lists (assignee, requester),
 *  capped at 20 entries each, persisted to localStorage. The picker's
 *  "Recent" section reads `topFor(scope, currentSelectionUuid)` — already
 *  pruned to the top 5 with the current selection excluded so it doesn't
 *  duplicate the "Selected" pin. */
export const useRecentUsersStore = defineStore('recentUsers', () => {
  const auth = useAuthStore()
  const accountKey = () => auth.user?.uuid ?? null

  // The list itself. Loaded for the current account on store init and
  // reloaded whenever the signed-in account changes.
  const recents = ref<PersistedShape>(loadFromStorage(accountKey()))

  // Re-bucket whenever the account changes (login / logout / switch).
  watch(
    () => accountKey(),
    (uuid) => {
      recents.value = loadFromStorage(uuid)
    },
  )

  // Persist on every mutation. Cheap — total payload is at most
  // 40 entries × ~80 bytes each, well under the localStorage budget.
  watch(
    recents,
    (next) => {
      if (typeof localStorage === 'undefined') return
      try {
        localStorage.setItem(storageKey(accountKey()), JSON.stringify(next))
      } catch {
        // Quota exceeded or storage disabled. Best-effort; the in-memory
        // list still drives the UI for the current session.
      }
    },
    { deep: true },
  )

  /** Record that the current account just picked `user` for the given
   *  scope. Dedupes by uuid (existing entry is moved to the top with a
   *  fresh timestamp), and caps the list at MAX_PER_SCOPE. */
  function remember(scope: RecentScope, user: { uuid: string; name: string }) {
    const list = recents.value[scope]
    const filtered = list.filter((r) => r.uuid !== user.uuid)
    filtered.unshift({ uuid: user.uuid, name: user.name, lastUsed: Date.now() })
    recents.value[scope] = filtered.slice(0, MAX_PER_SCOPE)
  }

  /** The top N recents for the picker's "Recent" section, with
   *  `excludeUuid` filtered out (typically the currently-selected user
   *  so the row doesn't appear in both "Selected" and "Recent"). */
  function topFor(scope: RecentScope, excludeUuid?: string | null): RecentUser[] {
    const list = recents.value[scope]
    const pruned = excludeUuid ? list.filter((r) => r.uuid !== excludeUuid) : list
    return pruned.slice(0, VISIBLE_PER_SCOPE)
  }

  /** Clear all recents for the current account. Exposed for tests / a
   *  potential future "clear history" admin action. */
  function clear(scope?: RecentScope) {
    if (scope) {
      recents.value[scope] = []
    } else {
      recents.value = { ...EMPTY }
    }
  }

  return {
    remember,
    topFor,
    clear,
  }
})
