import { ref, watch, type Ref } from 'vue'

interface DebouncedRefOptions<T> {
  /**
   * Predicate evaluated on each source change. Return true to
   * sync the debounced ref immediately for that update, skipping
   * the delay; return false (the default) to debounce as usual.
   *
   *   leading: (prev, next) => !prev && !!next   // fire on first activation
   *   leading: (prev, next) => !prev || !next    // fire on either edge of empty
   */
  leading?: (prev: T, next: T) => boolean
}

/**
 * Returns a ref whose value lags behind `source` by `delay` ms.
 * The source ref still updates synchronously (use it for v-model
 * so the input feels instant); the returned ref is what to watch
 * when you want the post-debounce value.
 *
 *   const query = ref('')
 *   const debouncedQuery = useDebouncedRef(query, 150)
 *   watch(debouncedQuery, q => { runSearch(q) })
 *
 * Mirrors VueUse's `refDebounced` API in the bits we use; staying
 * in-house keeps the dep surface small.
 */
export function useDebouncedRef<T>(
  source: Ref<T>,
  delay = 200,
  options: DebouncedRefOptions<T> = {},
): Ref<T> {
  const debounced = ref(source.value) as Ref<T>
  let timer: ReturnType<typeof setTimeout> | null = null
  watch(source, (next, prev) => {
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
    if (options.leading?.(prev, next)) {
      debounced.value = next
      return
    }
    timer = setTimeout(() => {
      debounced.value = next
    }, delay)
  })
  return debounced
}
