import { ref, computed, onMounted, onBeforeUnmount, type Ref, type ComputedRef } from 'vue'

// Shared state - single resize listener for all consumers
let listenerCount = 0
let resizeTimeout: ReturnType<typeof setTimeout> | null = null
const windowWidth = ref(typeof window !== 'undefined' ? window.innerWidth : 1024)

const updateWidth = () => {
  windowWidth.value = window.innerWidth
}

const debouncedUpdateWidth = () => {
  if (resizeTimeout) clearTimeout(resizeTimeout)
  resizeTimeout = setTimeout(updateWidth, 150)
}

const addListener = () => {
  listenerCount++
  if (listenerCount === 1 && typeof window !== 'undefined') {
    updateWidth() // Ensure current value
    window.addEventListener('resize', debouncedUpdateWidth)
  }
}

const removeListener = () => {
  listenerCount--
  if (listenerCount === 0 && typeof window !== 'undefined') {
    window.removeEventListener('resize', debouncedUpdateWidth)
    if (resizeTimeout) {
      clearTimeout(resizeTimeout)
      resizeTimeout = null
    }
  }
}

// Common breakpoints
export const BREAKPOINTS = {
  sm: 640,
  md: 768,
  lg: 1024,
  xl: 1280,
} as const

export type BreakpointKey = keyof typeof BREAKPOINTS

interface UseMobileDetectionReturn {
  /** Whether viewport is below the breakpoint */
  isMobile: ComputedRef<boolean>
  /** Current window width */
  windowWidth: Ref<number>
}

/**
 * Shared mobile detection composable.
 * Uses a single resize listener for all consumers.
 *
 * @param breakpoint - Pixel width or breakpoint key (default: 'lg' = 1024px)
 */
export function useMobileDetection(breakpoint: number | BreakpointKey = 'lg'): UseMobileDetectionReturn {
  const breakpointPx = typeof breakpoint === 'number'
    ? breakpoint
    : BREAKPOINTS[breakpoint]

  const isMobile = computed(() => windowWidth.value < breakpointPx)

  onMounted(addListener)
  onBeforeUnmount(removeListener)

  return {
    isMobile,
    windowWidth
  }
}
