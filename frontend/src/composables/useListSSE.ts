import { ref, onActivated, onDeactivated, onMounted, onUnmounted } from 'vue'
import { useSSE } from '@/services/sseService'
import { useAuthStore } from '@/stores/auth'

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
  getEventItemId: (data: any) => string | number | undefined

  // Item key in created events (e.g., 'ticket', 'user')
  itemKey?: string

  // Custom update handler for complex field mappings
  onItemUpdated?: (data: any) => void
}

/**
 * SSE integration for list views with real-time updates.
 */
export function useListSSE<T>(options: UseListSSEOptions<T>) {
  const { addEventListener, removeEventListener, connect, isConnected } = useSSE()
  const authStore = useAuthStore()
  const isActive = ref(true)

  const handleItemUpdated = (data: any) => {
    if (!isActive.value) return

    const itemId = options.getEventItemId(data)
    if (!itemId || !options.hasItem(itemId)) return

    if (options.onItemUpdated) {
      options.onItemUpdated(data)
    } else {
      // Default: direct field update
      const eventData = data.data || data
      if (eventData.field && eventData.value !== undefined) {
        options.updateItemField(itemId, eventData.field, eventData.value)
      }
    }
  }

  const handleItemCreated = (data: any) => {
    if (!isActive.value) return

    const eventData = data.data || data
    const itemKey = options.itemKey || 'item'
    if (eventData[itemKey]) {
      options.prependItem(eventData[itemKey])
    }
  }

  const handleItemDeleted = (data: any) => {
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
        addEventListener(eventType as any, handlers[key as keyof typeof handlers])
      }
    }
  }

  const cleanupListeners = () => {
    for (const [key, eventType] of Object.entries(options.eventTypes)) {
      if (eventType) {
        removeEventListener(eventType as any, handlers[key as keyof typeof handlers])
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
