import { ref, computed, onMounted, onUnmounted, watch, type Ref } from 'vue'

/**
 * Composable for horizontal scroll containers with edge fade indicators
 * Shows visual cues when content overflows on either side
 */
export function useHorizontalScroll(containerRef: Ref<HTMLElement | null>, dotCount = 5) {
  const canScrollLeft = ref(false)
  const canScrollRight = ref(false)
  const isOverflowing = ref(false)
  const scrollProgress = ref(0) // 0 to 1

  let resizeObserver: ResizeObserver | null = null
  let currentContainer: HTMLElement | null = null

  const updateScrollState = () => {
    const container = containerRef.value
    if (!container) return

    const { scrollLeft, scrollWidth, clientWidth } = container
    const maxScroll = scrollWidth - clientWidth

    // Check if content overflows
    isOverflowing.value = scrollWidth > clientWidth

    // Can scroll left if not at the start
    canScrollLeft.value = scrollLeft > 1

    // Can scroll right if not at the end (with small tolerance for rounding)
    canScrollRight.value = scrollLeft < maxScroll - 1

    // Calculate scroll progress (0 to 1)
    scrollProgress.value = maxScroll > 0 ? scrollLeft / maxScroll : 0
  }

  // Compute which dot should be active based on scroll progress
  const activeDotIndex = computed(() => {
    if (!isOverflowing.value) return 0
    // Map progress (0-1) to dot index (0 to dotCount-1)
    return Math.round(scrollProgress.value * (dotCount - 1))
  })

  const scrollTo = (direction: 'left' | 'right', amount = 150) => {
    const container = containerRef.value
    if (!container) return

    const scrollAmount = direction === 'left' ? -amount : amount
    container.scrollBy({ left: scrollAmount, behavior: 'smooth' })
  }

  const setupContainer = (container: HTMLElement | null) => {
    // Cleanup previous container
    if (currentContainer) {
      currentContainer.removeEventListener('scroll', updateScrollState)
      resizeObserver?.unobserve(currentContainer)
    }

    currentContainer = container
    if (!container) return

    // Initial check
    updateScrollState()

    // Listen to scroll events
    container.addEventListener('scroll', updateScrollState, { passive: true })

    // Watch for size changes
    if (!resizeObserver) {
      resizeObserver = new ResizeObserver(updateScrollState)
    }
    resizeObserver.observe(container)
  }

  // Watch for ref changes (handles dynamic content)
  watch(containerRef, (newContainer) => {
    setupContainer(newContainer)
  }, { immediate: true })

  onMounted(() => {
    // Setup if ref is already available
    if (containerRef.value && !currentContainer) {
      setupContainer(containerRef.value)
    }
  })

  onUnmounted(() => {
    if (currentContainer) {
      currentContainer.removeEventListener('scroll', updateScrollState)
    }
    resizeObserver?.disconnect()
  })

  return {
    canScrollLeft,
    canScrollRight,
    isOverflowing,
    scrollProgress,
    activeDotIndex,
    updateScrollState,
    scrollTo
  }
}
