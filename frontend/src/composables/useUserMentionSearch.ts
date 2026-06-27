/**
 * Reactive user-mention search for editor + mention-input components.
 *
 * Owns the full search lifecycle: watches a query ref, debounces,
 * fetches via `userService.getPaginatedUsers` with a fresh
 * `AbortController` per call, cancels the in-flight request on a
 * newer keystroke, and tears down on component unmount.
 *
 * Cancellation is internal: every fresh keystroke aborts the prior
 * call and the resulting `AbortError` is swallowed silently. Callers
 * just read the returned refs and render. No try/catch, no string
 * sentinels, no console.error on a debouncer race.
 *
 * Compared to the previous per-component implementation (three
 * near-identical 20-line blocks across `MentionInput`, `SimpleEditor`,
 * and `CollaborativeEditor`), this:
 *
 *  - replaces the legacy `requestManager` keyed-by-string cancellation
 *    pattern with the spec's `AbortSignal`, so cancellation is a real
 *    `AbortError` instead of the stringly-typed `REQUEST_CANCELLED`
 *    sentinel each caller had to remember to ignore;
 *  - guarantees teardown on `onScopeDispose`, so a fast-typing user
 *    unmounting the dropdown never leaves a settling fetch racing
 *    against torn-down refs;
 *  - centralises the debounce window so all mention surfaces feel the
 *    same.
 */

import { onScopeDispose, ref, watch, type Ref } from 'vue'
import userService from '@/services/userService'
import type { User } from '@nosdesk/core/types/user'

export interface UseUserMentionSearchOptions {
  /** Max results to fetch per query. Defaults to 10. Dropdown
   *  surfaces typically render 6 to 10. */
  limit?: number
  /** Debounce window in milliseconds. Defaults to 200. */
  debounceMs?: number
  /** Reactive gate: when set and false, no search runs. Useful when
   *  the consuming surface (e.g. a mention dropdown) is hidden and
   *  the typed query shouldn't trigger network. */
  enabled?: Ref<boolean>
}

export interface UseUserMentionSearchReturn {
  /** Current results. Empty array while loading or after an error. */
  users: Ref<User[]>
  /** True while a fetch is in flight (post-debounce). */
  isLoading: Ref<boolean>
  /** Non-null when the last fetch failed for a reason other than
   *  cancellation. Cancellation is not surfaced. */
  error: Ref<Error | null>
}

export function useUserMentionSearch(
  query: Ref<string>,
  options: UseUserMentionSearchOptions = {},
): UseUserMentionSearchReturn {
  const { limit = 10, debounceMs = 200, enabled } = options

  const users = ref<User[]>([])
  const isLoading = ref(false)
  const error = ref<Error | null>(null)

  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let controller: AbortController | null = null

  function cancelPending() {
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    controller?.abort()
    controller = null
  }

  async function run(q: string) {
    controller?.abort()
    const ac = new AbortController()
    controller = ac

    isLoading.value = true
    error.value = null

    try {
      const result = await userService.getPaginatedUsers(
        { page: 1, pageSize: limit, search: q || undefined },
        'user-mention-search',
        { signal: ac.signal },
      )
      if (ac.signal.aborted) return
      users.value = result.data
    } catch (err) {
      // AbortError is the spec signal for "I cancelled this on
      // purpose" (e.g. user typed again before this resolved).
      // Never surfaced to the caller; not an error.
      if (ac.signal.aborted || (err as Error)?.name === 'AbortError') return
      error.value = err instanceof Error ? err : new Error(String(err))
      users.value = []
    } finally {
      if (!ac.signal.aborted) isLoading.value = false
    }
  }

  watch(
    query,
    (q) => {
      if (enabled && !enabled.value) {
        cancelPending()
        return
      }
      if (debounceTimer) clearTimeout(debounceTimer)
      debounceTimer = setTimeout(() => void run(q), debounceMs)
    },
    { immediate: true },
  )

  if (enabled) {
    watch(enabled, (on) => {
      if (!on) cancelPending()
    })
  }

  onScopeDispose(cancelPending)

  return { users, isLoading, error }
}
