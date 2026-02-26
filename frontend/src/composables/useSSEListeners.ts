import { ref, onMounted, onUnmounted, onActivated, onDeactivated } from 'vue'
import { useSSE, type SSEEventType } from '@/services/sseService'

interface UseSSEListenersOptions {
  /** Reload function — used to create a debounced reload for SSE-triggered refreshes */
  reload?: () => void | Promise<void>
  /** Debounce delay in ms (default: 300) */
  debounceMs?: number
}

/**
 * Composable for SSE event listeners with KeepAlive support and optional debounced reloading.
 *
 * Handles:
 * - KeepAlive: pauses handlers when component is deactivated, resumes on activation
 * - Lifecycle: registers listeners on mount, cleans up on unmount (including timers)
 * - Debouncing: optional debounced reload to prevent request storms from bulk SSE events
 *
 * Usage:
 *   const { on, debouncedReload } = useSSEListeners({ reload: loadData })
 *   on('ticket-updated', (data) => { ... })
 *   on('documentation-created', () => debouncedReload())
 */
export function useSSEListeners(options?: UseSSEListenersOptions) {
  const { addEventListener, removeEventListener } = useSSE()
  const isActive = ref(true)
  const registered: [SSEEventType, (data: unknown) => void][] = []
  let reloadTimer: ReturnType<typeof setTimeout> | null = null

  /** Register an SSE event handler. Must be called synchronously during setup. */
  const on = (event: SSEEventType, handler: (data: unknown) => void) => {
    const wrapped = (data: unknown) => {
      if (!isActive.value) return
      handler(data)
    }
    registered.push([event, wrapped])
  }

  /** Debounced reload — coalesces rapid SSE events into a single reload call */
  const debouncedReload = options?.reload
    ? () => {
        if (reloadTimer) clearTimeout(reloadTimer)
        reloadTimer = setTimeout(() => options.reload!(), options.debounceMs ?? 300)
      }
    : () => {}

  onMounted(() => {
    for (const [event, handler] of registered) {
      addEventListener(event, handler)
    }
  })

  onUnmounted(() => {
    if (reloadTimer) clearTimeout(reloadTimer)
    for (const [event, handler] of registered) {
      removeEventListener(event, handler)
    }
  })

  onActivated(() => { isActive.value = true })
  onDeactivated(() => { isActive.value = false })

  return { on, debouncedReload }
}
