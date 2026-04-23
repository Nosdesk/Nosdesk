/**
 * Minimal async resource plumbing. Owns the loading/error/data triad
 * every list-style widget was repeating inline.
 *
 * Deliberately small — no SSE, no retries, no cache. Widgets that
 * need live-update layer `useTicketStats` (or similar) on top.
 */
import { onMounted, ref, type Ref } from 'vue'

export interface UseAsyncResourceResult<T> {
  data: Ref<T>
  loading: Ref<boolean>
  error: Ref<string | null>
  /** Re-run the fetcher. Does not toggle `loading` — use it for
   *  manual refresh where the caller wants to keep current content
   *  on screen while new data arrives. */
  reload: () => Promise<void>
}

export function useAsyncResource<T>(
  fetcher: () => Promise<T>,
  initial: T,
  errorLabel = 'Failed to load',
): UseAsyncResourceResult<T> {
  const data = ref(initial) as Ref<T>
  const loading = ref(true)
  const error = ref<string | null>(null)

  async function reload() {
    try {
      data.value = await fetcher()
      error.value = null
    } catch (e) {
      console.error('useAsyncResource fetch failed:', e)
      error.value = errorLabel
    } finally {
      loading.value = false
    }
  }

  onMounted(reload)
  return { data, loading, error, reload }
}
