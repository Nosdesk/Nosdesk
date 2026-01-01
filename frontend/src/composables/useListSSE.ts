import { ref, onActivated, onDeactivated, onMounted, onUnmounted } from 'vue'
import { useSSE, type SSEEventType } from '@/services/sseService'
import { useAuthStore } from '@/stores/auth'

// SSE event data wrapper type (matches sseService)
interface SSEEventData {
  data?: Record<string, unknown>
  [key: string]: unknown
}

interface UseListSSEOptions<T> {
  // List manager methods
  hasItem: (id: string | number) => boolean
  updateItemField: <K extends keyof T>(id: string | number, field: K, value: T[K]) => boolean
  removeItem: (id: string | number) => boolean
  prependItem: (item: T) => void

  // Event configuration
  eventTypes: {
    updated?: string
    created?: string
    deleted?: string
  }

  // ID extractor from event data
  getEventItemId: (data: unknown) => string | number | undefined

  // Item key in created events (e.g., 'ticket', 'user')
  itemKey?: string

  // Custom update handler for complex field mappings
  onItemUpdated?: (data: unknown) => void
}

/**
 * SSE integration for list views with real-time updates.
 */
export function useListSSE<T>(options: UseListSSEOptions<T>) {
  const { addEventListener, removeEventListener, connect, isConnected } = useSSE()
  const authStore = useAuthStore()
  const isActive = ref(true)

  const handleItemUpdated = (data: unknown) => {
    if (!isActive.value) return

    const itemId = options.getEventItemId(data)
    if (!itemId || !options.hasItem(itemId)) return

    if (options.onItemUpdated) {
      options.onItemUpdated(data)
    } else {
      // Default: direct field update
      const eventObj = data as SSEEventData
      const eventData = eventObj.data || eventObj
      if (eventData.field && eventData.value !== undefined) {
        options.updateItemField(itemId, eventData.field as keyof T, eventData.value as T[keyof T])
      }
    }
  }

  const handleItemCreated = (data: unknown) => {
    if (!isActive.value) return

    const eventObj = data as SSEEventData
    const eventData = eventObj.data || eventObj
    const itemKey = options.itemKey || 'item'
    if (eventData[itemKey]) {
      options.prependItem(eventData[itemKey] as T)
    }
  }

  const handleItemDeleted = (data: unknown) => {
    if (!isActive.value) return

    const itemId = options.getEventItemId(data)
    if (itemId && options.hasItem(itemId)) {
      options.removeItem(itemId)
    }
  }

  const handlers = {
    updated: handleItemUpdated,
    created: handleItemCreated,
    deleted: handleItemDeleted
  }

  const setupListeners = () => {
    for (const [key, eventType] of Object.entries(options.eventTypes)) {
      if (eventType) {
        addEventListener(eventType as SSEEventType, handlers[key as keyof typeof handlers])
      }
    }
  }

  const cleanupListeners = () => {
    for (const [key, eventType] of Object.entries(options.eventTypes)) {
      if (eventType) {
        removeEventListener(eventType as SSEEventType, handlers[key as keyof typeof handlers])
      }
    }
  }

  onActivated(() => { isActive.value = true })
  onDeactivated(() => { isActive.value = false })

  onMounted(() => {
    setupListeners()
    if (authStore.isAuthenticated && !isConnected.value) {
      connect()
    }
  })

  onUnmounted(cleanupListeners)

  return { isConnected }
}
