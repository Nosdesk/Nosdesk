/**
 * Cursor Scanlines Composable
 *
 * Creates a full-screen scanline overlay effect that follows the cursor position.
 * The effect renders horizontal and vertical lines spanning the viewport at the
 * cursor's X and Y coordinates, creating a targeting/CRT aesthetic.
 *
 * Only active when the specified theme is applied (default: red-horizon).
 */
import { ref, onMounted, onUnmounted, watch } from 'vue'

interface ScanlineOptions {
  theme?: string
  horizontalColor?: string
  verticalColor?: string
  lineWidth?: number
  opacity?: number
}

export function useCursorScanlines(options: ScanlineOptions = {}) {
  const {
    theme = 'red-horizon',
    horizontalColor = 'rgba(255, 136, 68, 0.15)',
    verticalColor = 'rgba(255, 136, 68, 0.15)',
    lineWidth = 1,
    opacity = 1
  } = options

  const isActive = ref(false)
  const cursorX = ref(0)
  const cursorY = ref(0)

  let horizontalLine: HTMLDivElement | null = null
  let verticalLine: HTMLDivElement | null = null
  let rafId: number | null = null
  let fadeTimeout: number | null = null
  let lastX = 0
  let lastY = 0

  // Check if device supports touch
  const isTouchDevice = () => 'ontouchstart' in window || navigator.maxTouchPoints > 0

  const createOverlayElements = () => {
    const isTouch = isTouchDevice()

    // Horizontal line (spans full width at cursor Y)
    horizontalLine = document.createElement('div')
    horizontalLine.className = 'cursor-scanline cursor-scanline-horizontal'
    horizontalLine.style.cssText = `
      position: fixed;
      left: 0;
      right: 0;
      height: ${lineWidth}px;
      background: ${horizontalColor};
      pointer-events: none;
      z-index: 99999;
      opacity: ${isTouch ? 0 : opacity};
      transform: translateY(-50%);
      transition: ${isTouch ? 'opacity 0.3s ease, top 0.15s ease-out' : 'opacity 0.15s ease'};
    `

    // Vertical line (spans full height at cursor X)
    verticalLine = document.createElement('div')
    verticalLine.className = 'cursor-scanline cursor-scanline-vertical'
    verticalLine.style.cssText = `
      position: fixed;
      top: 0;
      bottom: 0;
      width: ${lineWidth}px;
      background: ${verticalColor};
      pointer-events: none;
      z-index: 99999;
      opacity: ${isTouch ? 0 : opacity};
      transform: translateX(-50%);
      transition: ${isTouch ? 'opacity 0.3s ease, left 0.15s ease-out' : 'opacity 0.15s ease'};
    `

    document.body.appendChild(horizontalLine)
    document.body.appendChild(verticalLine)
  }

  const removeOverlayElements = () => {
    if (horizontalLine) {
      horizontalLine.remove()
      horizontalLine = null
    }
    if (verticalLine) {
      verticalLine.remove()
      verticalLine = null
    }
  }

  const updatePosition = () => {
    if (!horizontalLine || !verticalLine) return

    // Use transform for better performance (GPU accelerated)
    horizontalLine.style.top = `${lastY}px`
    verticalLine.style.left = `${lastX}px`
  }

  const handleMouseMove = (event: MouseEvent) => {
    lastX = event.clientX
    lastY = event.clientY
    cursorX.value = lastX
    cursorY.value = lastY

    // Use requestAnimationFrame for smooth updates
    if (rafId === null) {
      rafId = requestAnimationFrame(() => {
        updatePosition()
        rafId = null
      })
    }
  }

  const handleMouseLeave = () => {
    // Fade out when cursor leaves window
    if (horizontalLine) horizontalLine.style.opacity = '0'
    if (verticalLine) verticalLine.style.opacity = '0'
  }

  const handleMouseEnter = () => {
    // Fade in when cursor enters window
    if (horizontalLine) horizontalLine.style.opacity = String(opacity)
    if (verticalLine) verticalLine.style.opacity = String(opacity)
  }

  const handleTouchStart = (event: TouchEvent) => {
    if (event.touches.length === 0) return

    const touch = event.touches[0]
    lastX = touch.clientX
    lastY = touch.clientY
    cursorX.value = lastX
    cursorY.value = lastY

    // Clear any pending fade timeout
    if (fadeTimeout !== null) {
      clearTimeout(fadeTimeout)
      fadeTimeout = null
    }

    // Update position and show
    if (rafId === null) {
      rafId = requestAnimationFrame(() => {
        updatePosition()
        rafId = null
      })
    }

    // Fade in
    if (horizontalLine) horizontalLine.style.opacity = String(opacity)
    if (verticalLine) verticalLine.style.opacity = String(opacity)
  }

  const handleTouchMove = (event: TouchEvent) => {
    if (event.touches.length === 0) return

    const touch = event.touches[0]
    lastX = touch.clientX
    lastY = touch.clientY
    cursorX.value = lastX
    cursorY.value = lastY

    // Update position
    if (rafId === null) {
      rafId = requestAnimationFrame(() => {
        updatePosition()
        rafId = null
      })
    }
  }

  const handleTouchEnd = () => {
    // Fade out after a short delay
    fadeTimeout = window.setTimeout(() => {
      if (horizontalLine) horizontalLine.style.opacity = '0'
      if (verticalLine) verticalLine.style.opacity = '0'
      fadeTimeout = null
    }, 600) // Show for 600ms then fade out
  }

  const checkTheme = () => {
    const currentTheme = document.documentElement.getAttribute('data-theme')
    const shouldBeActive = currentTheme === theme

    if (shouldBeActive && !isActive.value) {
      activate()
    } else if (!shouldBeActive && isActive.value) {
      deactivate()
    }
  }

  const activate = () => {
    if (isActive.value) return

    isActive.value = true
    createOverlayElements()

    // Mouse events for desktop
    document.addEventListener('mousemove', handleMouseMove, { passive: true })
    document.addEventListener('mouseleave', handleMouseLeave)
    document.addEventListener('mouseenter', handleMouseEnter)

    // Touch events for mobile - show on tap, follow drag, fade out after
    document.addEventListener('touchstart', handleTouchStart, { passive: true })
    document.addEventListener('touchmove', handleTouchMove, { passive: true })
    document.addEventListener('touchend', handleTouchEnd, { passive: true })
  }

  const deactivate = () => {
    if (!isActive.value) return

    isActive.value = false
    removeOverlayElements()

    document.removeEventListener('mousemove', handleMouseMove)
    document.removeEventListener('mouseleave', handleMouseLeave)
    document.removeEventListener('mouseenter', handleMouseEnter)
    document.removeEventListener('touchstart', handleTouchStart)
    document.removeEventListener('touchmove', handleTouchMove)
    document.removeEventListener('touchend', handleTouchEnd)

    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }

    if (fadeTimeout !== null) {
      clearTimeout(fadeTimeout)
      fadeTimeout = null
    }
  }

  // Watch for theme changes via MutationObserver
  let themeObserver: MutationObserver | null = null

  onMounted(() => {
    // Initial check
    checkTheme()

    // Watch for theme attribute changes on html element
    themeObserver = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        if (mutation.type === 'attributes' && mutation.attributeName === 'data-theme') {
          checkTheme()
        }
      }
    })

    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme']
    })
  })

  onUnmounted(() => {
    deactivate()

    if (themeObserver) {
      themeObserver.disconnect()
      themeObserver = null
    }
  })

  return {
    isActive,
    cursorX,
    cursorY,
    activate,
    deactivate
  }
}

export default useCursorScanlines
