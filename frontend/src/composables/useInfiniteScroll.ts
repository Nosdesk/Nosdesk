import { ref, onMounted, onBeforeUnmount, watch, type Ref } from 'vue'

interface UseInfiniteScrollOptions {
  /** Ref to the scrollable container element */
  containerRef: Ref<HTMLElement | null>
  /** Whether infinite scroll mode is active */
  enabled: Ref<boolean>
  /** Whether there are more items to load */
  hasMore: Ref<boolean>
  /** Whether currently loading more items */
  isLoading: Ref<boolean>
  /** Callback when more items should be loaded */
  onLoadMore: () => void
  /** Distance from bottom to trigger load (px). Default: 400 */
  threshold?: number
}

/**
 * Composable for infinite scroll behavior.
 * Handles scroll detection and auto-loading when content doesn't fill viewport.
 */
export function useInfiniteScroll(options: UseInfiniteScrollOptions) {
  const {
    containerRef,
    enabled,
    hasMore,
    isLoading,
    onLoadMore,
    threshold = 400
  } = options

  const handleScroll = () => {
    if (!enabled.value || !hasMore.value || isLoading.value) return

    const container = containerRef.value
    if (!container) return

    const { scrollTop, scrollHeight, clientHeight } = container
    const distanceFromBottom = scrollHeight - scrollTop - clientHeight

    if (distanceFromBottom < threshold) {
      onLoadMore()
    }
  }

  // Auto-load if content doesn't fill viewport
  const checkViewportFill = () => {
    if (!enabled.value || !hasMore.value || isLoading.value) return

    const container = containerRef.value
    if (!container) return

    if (container.scrollHeight <= container.clientHeight) {
      onLoadMore()
    }
  }

  // Watch for changes that might require checking viewport fill
  watch(
    [containerRef, enabled, hasMore, isLoading],
    () => {
      // Use requestAnimationFrame to ensure DOM has updated
      requestAnimationFrame(checkViewportFill)
    },
    { flush: 'post' }
  )

  // Set up scroll listener
  watch(
    containerRef,
    (newContainer, oldContainer) => {
      if (oldContainer) {
        oldContainer.removeEventListener('scroll', handleScroll)
      }
      if (newContainer) {
        newContainer.addEventListener('scroll', handleScroll, { passive: true })
      }
    },
    { immediate: true }
  )

  // Cleanup on unmount
  onBeforeUnmount(() => {
    const container = containerRef.value
    if (container) {
      container.removeEventListener('scroll', handleScroll)
    }
  })

  return {
    /** Manually trigger a viewport fill check */
    checkViewportFill
  }
}
