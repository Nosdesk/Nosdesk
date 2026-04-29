import { computed, onUnmounted, ref, watch, type ComputedRef, type Ref } from 'vue'
import { useDataStore } from '@/stores/dataStore'
import { useAuthStore } from '@/stores/auth'
import { useRecentUsersStore, type RecentScope } from '@/stores/recentUsers'
import { useDebouncedRef } from '@/composables/useDebouncedRef'

/** A single row's worth of data the picker UI needs. Trimmed from the
 *  full `User` type so the dropdown doesn't carry payload it can't
 *  render (signatures, dashboard layout, etc.). */
export interface PickerUser {
  uuid: string
  name: string
  email: string
  role?: string
  avatar_thumb?: string | null
  avatar_url?: string | null
}

/** Picker scope. Drives the eligible-set load strategy:
 *   - `assignee`: small finite set (admin + technician), loaded once on
 *     open and filtered client-side. Picker also enables the "You" row
 *     and the "Claim" outside-button affordance.
 *   - `requester`: large open set (anyone). Loaded as a paginated first
 *     page on open, server-filtered on type. */
export type UserPickerType = RecentScope

interface Options {
  /** Reactive picker scope. Type can change at runtime if needed; the
   *  composable re-loads accordingly. */
  type: UserPickerType
  /** Reactive currently-selected uuid. Read-only — the consumer is
   *  responsible for emitting updates. */
  selectedUuid: Ref<string>
  /** Optional pre-fetched user object for the current selection, used to
   *  resolve a name without an extra round trip when the picker mounts
   *  on a ticket whose requester/assignee object isn't already cached. */
  selectedUserSeed?: Ref<{ uuid: string; name: string; email?: string; avatar_thumb?: string | null; avatar_url?: string | null } | null | undefined>
}

interface Result {
  /** Live-bound input string. Component v-models the input box to this. */
  query: Ref<string>
  /** True while the eligible-set load is in flight. */
  isLoading: Ref<boolean>
  /** True when the user has typed a non-empty query. The Recent / You
   *  sections hide while this is true; only the filtered results
   *  section renders. */
  isFiltering: ComputedRef<boolean>
  /** The currently-selected user resolved to a full row (name, avatar,
   *  email). Pinned to the top of the dropdown. Null when no selection
   *  or the row hasn't resolved yet. */
  selected: ComputedRef<PickerUser | null>
  /** The current signed-in user as a picker row, only when they're
   *  eligible for the picker scope (e.g. tech / admin for assignee).
   *  Null otherwise — render hides the "You" row in that case. */
  currentUserRow: ComputedRef<PickerUser | null>
  /** Top recents for this scope, current selection excluded so the row
   *  doesn't appear in both "Selected" and "Recent". Hidden while
   *  filtering. */
  recent: ComputedRef<PickerUser[]>
  /** The "All" / filtered-results list. With an empty query: the full
   *  eligible set minus the selection / "You" / recents (so each user
   *  shows up exactly once). With a query: the loaded set filtered
   *  client-side for assignee, or the most recent server response for
   *  requester. */
  results: ComputedRef<PickerUser[]>
  /** Display name for the currently-selected uuid, used to fill the
   *  input value. Falls back to the seed prop, then the data store
   *  cache, then the uuid itself. */
  selectedDisplayName: ComputedRef<string>
  /** True if the current signed-in account is eligible to assign this
   *  picker. Drives the "You" row + "Claim" outside button
   *  visibility. Always false for the requester picker. */
  canSelfAssign: ComputedRef<boolean>
  /** Run the eligible-set load — call when the dropdown opens. Idempotent
   *  for assignee (cached) and refreshing for requester (re-paginates
   *  the first page each open since the user roster can churn). */
  loadEligible: () => Promise<void>
  /** Notify the recents store that this user just got picked. The
   *  picker component calls this after emitting the update so the
   *  caller never sees the recent before the actual change has
   *  propagated. */
  remember: (user: PickerUser) => void
}

/** Eligible-set strategy. The assignee scope hits a comma-separated
 *  multi-role filter (admin,technician) — backend supports this in one
 *  request. The requester scope leaves role unset to load everyone. */
const ROLE_FILTER: Record<UserPickerType, string | undefined> = {
  assignee: 'admin,technician',
  requester: undefined,
}

/** Roles the assignee picker considers eligible. Backend already
 *  filters via the `roles` param, but we also enforce client-side so a
 *  stale role on a cached user, an API regression, or a recents entry
 *  predating tighter validation can't surface a non-staff option. */
const ASSIGNEE_ELIGIBLE_ROLES = new Set(['admin', 'technician'])

function isEligibleForType(type: UserPickerType, role: string | undefined): boolean {
  if (type === 'requester') return true
  return !!role && ASSIGNEE_ELIGIBLE_ROLES.has(role)
}

const PAGE_SIZE = 50
const DEBOUNCE_MS = 200

export function useUserPicker(opts: Options): Result {
  const dataStore = useDataStore()
  const authStore = useAuthStore()
  const recentStore = useRecentUsersStore()

  const query = ref('')
  // Debounced query drives the server-side requester search; the input
  // ref is what the user sees. Same pattern as the global search modal.
  const debouncedQuery = useDebouncedRef(query, DEBOUNCE_MS)

  const isLoading = ref(false)
  const isFiltering = computed(() => query.value.trim().length > 0)

  // Eligible set as last loaded. Two roles depending on scope:
  //   * assignee: the full admin + technician roster, loaded once.
  //   * requester: the latest server response — the entire response IS
  //     already filtered server-side on `debouncedQuery`, so we treat it
  //     as the visible result set rather than re-filtering client-side.
  const eligible = ref<PickerUser[]>([])
  const eligibleLoadedFor = ref<UserPickerType | null>(null)

  // ---- Selection-resolution helpers ----

  // The currently-selected row. Resolves from (in priority order):
  //   1. The seed prop, when its uuid matches the current selection.
  //   2. The eligible set, when the selection is in there.
  //   3. The data store's cached user lookup.
  // The component renders nothing for "Selected" until the row exists.
  const selected = computed<PickerUser | null>(() => {
    const uuid = opts.selectedUuid.value
    if (!uuid) return null

    const seed = opts.selectedUserSeed?.value
    if (seed && seed.uuid === uuid) {
      return {
        uuid: seed.uuid,
        name: seed.name,
        email: seed.email ?? '',
        avatar_thumb: seed.avatar_thumb,
        avatar_url: seed.avatar_url,
      }
    }

    const fromEligible = eligible.value.find((u) => u.uuid === uuid)
    if (fromEligible) return fromEligible

    const cached = dataStore.getCachedUserByUuid(uuid)
    if (cached) {
      return {
        uuid: cached.uuid,
        name: cached.name,
        email: cached.email ?? '',
        role: cached.role,
        avatar_thumb: cached.avatar_thumb,
        avatar_url: cached.avatar_url,
      }
    }

    return null
  })

  const selectedDisplayName = computed(() => {
    if (selected.value) return selected.value.name
    return opts.selectedUuid.value || ''
  })

  // The current signed-in user as a picker row, when they're eligible
  // for this scope. Returns null for requester pickers (the "You" row
  // doesn't apply there) or when the user is signed out.
  const currentUserRow = computed<PickerUser | null>(() => {
    if (opts.type !== 'assignee') return null
    const me = authStore.user
    if (!me) return null
    if (!authStore.isTechnician) return null
    return {
      uuid: me.uuid,
      name: me.name,
      email: me.email ?? '',
      role: me.role,
      avatar_thumb: me.avatar_thumb,
      avatar_url: me.avatar_url,
    }
  })

  const canSelfAssign = computed(
    () => opts.type === 'assignee' && currentUserRow.value !== null,
  )

  // ---- Section data ----

  // Recents: top 5 from the LRU, current selection excluded so the row
  // doesn't double up with the "Selected" pin. The "You" row is also
  // pruned so the current account isn't shown in both sections.
  const recent = computed<PickerUser[]>(() => {
    if (isFiltering.value) return []
    const exclude = new Set<string>()
    if (opts.selectedUuid.value) exclude.add(opts.selectedUuid.value)
    if (currentUserRow.value) exclude.add(currentUserRow.value.uuid)

    const recents = recentStore.topFor(opts.type, opts.selectedUuid.value || null)
    return recents
      .filter((r) => !exclude.has(r.uuid))
      .map<PickerUser | null>((r) => {
        // Resolve the recent uuid against either the eligible set or
        // the cached user store so we have an avatar / email to render.
        const fromEligible = eligible.value.find((u) => u.uuid === r.uuid)
        if (fromEligible) return fromEligible
        const cached = dataStore.getCachedUserByUuid(r.uuid)
        if (cached) {
          // Drop ineligible cached users — a recents entry from before
          // tighter role enforcement could otherwise surface a regular
          // user as an assignee option.
          if (!isEligibleForType(opts.type, cached.role)) return null
          return {
            uuid: cached.uuid,
            name: cached.name,
            email: cached.email ?? '',
            role: cached.role,
            avatar_thumb: cached.avatar_thumb,
            avatar_url: cached.avatar_url,
          }
        }
        // Unknown role: for assignee, refuse to render the row rather
        // than risk surfacing a non-staff user. For requester (open
        // set) it's safe to render the stub.
        if (opts.type === 'assignee') return null
        return { uuid: r.uuid, name: r.name, email: '' }
      })
      .filter((r): r is PickerUser => r !== null)
  })

  // Main results: with an empty query, the eligible set minus rows
  // already shown above (selected, you, recents). With a query, the
  // current eligible set filtered by name / email substring.
  const results = computed<PickerUser[]>(() => {
    const exclude = new Set<string>()
    if (selected.value) exclude.add(selected.value.uuid)
    if (currentUserRow.value) exclude.add(currentUserRow.value.uuid)
    if (!isFiltering.value) {
      for (const r of recent.value) exclude.add(r.uuid)
    }

    const trimmed = query.value.trim().toLowerCase()
    const base = eligible.value.filter((u) => !exclude.has(u.uuid))

    if (!trimmed) return base
    // Client-side filter for the small assignee set; the requester
    // server already filtered for us, but applying the same predicate
    // here is harmless and keeps the UI snappy if the response races.
    return base.filter(
      (u) =>
        u.name.toLowerCase().includes(trimmed) ||
        (u.email ?? '').toLowerCase().includes(trimmed),
    )
  })

  // ---- Loading ----

  async function fetchEligible(searchTerm: string | undefined): Promise<PickerUser[]> {
    const response = await dataStore.getPaginatedUsers({
      page: 1,
      pageSize: PAGE_SIZE,
      search: searchTerm ?? '',
      sortField: 'name',
      sortDirection: 'asc',
      role: ROLE_FILTER[opts.type],
    })
    return response.data
      .filter((u) => isEligibleForType(opts.type, u.role))
      .map<PickerUser>((u) => ({
        uuid: u.uuid,
        name: u.name,
        email: u.email,
        role: u.role,
        avatar_thumb: u.avatar_thumb,
        avatar_url: u.avatar_url,
      }))
  }

  async function loadEligible(): Promise<void> {
    // Assignee scope: cache the eligible set across opens. The roster
    // turns over rarely and the dropdown should feel instant.
    if (opts.type === 'assignee' && eligibleLoadedFor.value === 'assignee') {
      return
    }

    isLoading.value = true
    try {
      eligible.value = await fetchEligible(undefined)
      eligibleLoadedFor.value = opts.type
    } catch (err) {
      console.error('User picker load failed', err)
      eligible.value = []
    } finally {
      isLoading.value = false
    }
  }

  // For requester scope, re-fetch on every debounced query change so
  // the server gets to apply the filter. Assignee scope filters client
  // side and skips this watcher.
  const stopRequesterWatch = watch(debouncedQuery, async (next) => {
    if (opts.type !== 'requester') return
    isLoading.value = true
    try {
      eligible.value = await fetchEligible(next.trim() || undefined)
    } catch (err) {
      console.error('User picker requester search failed', err)
      eligible.value = []
    } finally {
      isLoading.value = false
    }
  })

  onUnmounted(stopRequesterWatch)

  function remember(user: PickerUser) {
    recentStore.remember(opts.type, { uuid: user.uuid, name: user.name })
  }

  return {
    query,
    isLoading,
    isFiltering,
    selected,
    currentUserRow,
    recent,
    results,
    selectedDisplayName,
    canSelfAssign,
    loadEligible,
    remember,
  }
}
