import { ref, onMounted, onUnmounted, watch, type Ref } from 'vue'

export interface DropdownPosition {
  top: number
  bottom: number
  left: number
  maxWidth?: number
  openDirection: 'down' | 'up'
}

/**
 * Composable for viewport-aware dropdown positioning.
 * Handles scroll updates, viewport edge detection, and responsive sizing.
 */
export function useDropdownPosition(
  triggerRef: Ref<HTMLElement | null>,
  isOpen: Ref<boolean>,
  options?: {
    preferredWidth?: number
    offset?: number
    minEdgeMargin?: number
  }
) {
  const position = ref<DropdownPosition>({
    top: 0,
    bottom: 0,
    left: 0,
    openDirection: 'down'
  })

  const preferredWidth = options?.preferredWidth ?? 192 // 12rem default
  const offset = options?.offset ?? 4
  const minEdgeMargin = options?.minEdgeMargin ?? 8

  let rafId: number | null = null

  const updatePosition = () => {
    if (!triggerRef.value || !isOpen.value) return

    const trigger = triggerRef.value
    const rect = trigger.getBoundingClientRect()
    const viewportWidth = window.innerWidth
    const viewportHeight = window.innerHeight

    // Calculate available space
    const spaceBelow = viewportHeight - rect.bottom - offset
    const spaceAbove = rect.top - offset
    const spaceRight = viewportWidth - rect.left
    const spaceLeft = rect.right

    // Estimate menu height (will be clamped anyway)
    const estimatedMenuHeight = 250

    // Determine whether dropdown should open upward
    const openUp = spaceBelow < estimatedMenuHeight && spaceAbove > spaceBelow

    // Calculate top position
    let top: number
    if (openUp) {
      // Position above the trigger
      top = rect.top - offset
    } else {
      // Position below the trigger
      top = rect.bottom + offset
    }

    // Calculate left position with viewport constraints
    let left = rect.left
    let maxWidth = preferredWidth

    // Check if dropdown would overflow right edge
    if (left + preferredWidth > viewportWidth - minEdgeMargin) {
      // Try to align to right edge of trigger
      left = Math.max(minEdgeMargin, rect.right - preferredWidth)

      // If still too wide, shrink to fit
      if (left + preferredWidth > viewportWidth - minEdgeMargin) {
        maxWidth = viewportWidth - left - minEdgeMargin
      }
    }

    // Ensure left doesn't go off screen
    if (left < minEdgeMargin) {
      left = minEdgeMargin
      maxWidth = Math.min(preferredWidth, viewportWidth - 2 * minEdgeMargin)
    }

    position.value = {
      top,
      bottom: viewportHeight - top,
      left,
      maxWidth,
      openDirection: openUp ? 'up' : 'down'
    }
  }

  const handleScroll = () => {
    if (!isOpen.value) return

    if (rafId) {
      cancelAnimationFrame(rafId)
    }

    rafId = requestAnimationFrame(() => {
      updatePosition()
      rafId = null
    })
  }

  const handleResize = () => {
    if (isOpen.value) {
      updatePosition()
    }
  }

  // Update position when dropdown opens
  watch(isOpen, (open) => {
    if (open) {
      // Use nextTick via setTimeout to ensure DOM is ready
      setTimeout(updatePosition, 0)
    }
  })

  onMounted(() => {
    window.addEventListener('scroll', handleScroll, true)
    window.addEventListener('resize', handleResize)
  })

  onUnmounted(() => {
    window.removeEventListener('scroll', handleScroll, true)
    window.removeEventListener('resize', handleResize)

    if (rafId) {
      cancelAnimationFrame(rafId)
    }
  })

  return {
    position,
    updatePosition
  }
}
