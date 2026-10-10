import { computed, type ComputedRef, type Ref } from 'vue'

/**
 * The message to show for a query that failed to load, or '' when there is
 * nothing to say. A failed refetch (the network dropped) leaves the data from
 * the last good fetch in place, so the message shows only while there is no
 * data to show instead. Pinia Colada refetches a failed query when the device
 * comes back online or the tab is shown again, and a fetch that works clears
 * the error.
 */
export function queryLoadError(
  query: { error: Ref<unknown>; data: Ref<unknown> },
  message: () => string,
): ComputedRef<string> {
  return computed(() => (query.error.value && query.data.value === undefined ? message() : ''))
}
