/**
 * Snowfall Effect Composable
 *
 * Creates ambient falling snow animation across the viewport.
 * Only active when the Christmas theme is applied.
 * Uses canvas for performant rendering of many snowflakes.
 */
import { ref, onMounted, onUnmounted } from 'vue'

interface Snowflake {
  x: number
  y: number
  radius: number
  speed: number
  wind: number
  opacity: number
}

interface SnowfallOptions {
  theme?: string
  snowflakeCount?: number
  minRadius?: number
  maxRadius?: number
  minSpeed?: number
  maxSpeed?: number
  wind?: number
  color?: string
}

export function useSnowfall(options: SnowfallOptions = {}) {
  const {
    theme = 'christmas',
    snowflakeCount = 100,
    minRadius = 1,
    maxRadius = 4,
    minSpeed = 0.5,
    maxSpeed = 2,
    wind = 0.3,
    color = 'rgba(255, 255, 255, 0.8)',
  } = options

  const isActive = ref(false)

  let canvas: HTMLCanvasElement | null = null
  let ctx: CanvasRenderingContext2D | null = null
  let snowflakes: Snowflake[] = []
  let animationId: number | null = null
  let resizeObserver: ResizeObserver | null = null

  const createSnowflake = (startAtTop = false): Snowflake => {
    const radius = minRadius + Math.random() * (maxRadius - minRadius)
    return {
      x: Math.random() * (canvas?.width || window.innerWidth),
      y: startAtTop ? -radius * 2 : Math.random() * (canvas?.height || window.innerHeight),
      radius,
      speed: minSpeed + Math.random() * (maxSpeed - minSpeed),
      wind: (Math.random() - 0.5) * wind * 2,
      opacity: 0.4 + Math.random() * 0.6,
    }
  }

  const initSnowflakes = () => {
    snowflakes = []
    for (let i = 0; i < snowflakeCount; i++) {
      snowflakes.push(createSnowflake(false))
    }
  }

  const createCanvas = () => {
    canvas = document.createElement('canvas')
    canvas.className = 'snowfall-canvas'
    canvas.style.cssText = `
      position: fixed;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      pointer-events: none;
      z-index: 99998;
    `
    canvas.width = window.innerWidth
    canvas.height = window.innerHeight

    ctx = canvas.getContext('2d')
    document.body.appendChild(canvas)

    // Watch for window resize
    resizeObserver = new ResizeObserver(() => {
      if (canvas) {
        canvas.width = window.innerWidth
        canvas.height = window.innerHeight
      }
    })
    resizeObserver.observe(document.body)
  }

  const removeCanvas = () => {
    if (canvas) {
      canvas.remove()
      canvas = null
      ctx = null
    }
    if (resizeObserver) {
      resizeObserver.disconnect()
      resizeObserver = null
    }
  }

  const drawSnowflake = (flake: Snowflake) => {
    if (!ctx) return

    ctx.beginPath()
    ctx.arc(flake.x, flake.y, flake.radius, 0, Math.PI * 2)
    ctx.fillStyle = color.replace(/[\d.]+\)$/, `${flake.opacity})`)
    ctx.fill()
  }

  const updateSnowflake = (flake: Snowflake) => {
    if (!canvas) return

    flake.y += flake.speed
    flake.x += flake.wind

    // Add slight horizontal oscillation for more natural movement
    flake.x += Math.sin(flake.y * 0.01) * 0.3

    // Reset snowflake when it goes off screen
    if (flake.y > canvas.height + flake.radius * 2) {
      flake.y = -flake.radius * 2
      flake.x = Math.random() * canvas.width
    }

    // Wrap horizontally
    if (flake.x > canvas.width + flake.radius) {
      flake.x = -flake.radius
    } else if (flake.x < -flake.radius) {
      flake.x = canvas.width + flake.radius
    }
  }

  const animate = () => {
    if (!ctx || !canvas) return

    ctx.clearRect(0, 0, canvas.width, canvas.height)

    for (const flake of snowflakes) {
      updateSnowflake(flake)
      drawSnowflake(flake)
    }

    animationId = requestAnimationFrame(animate)
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
    createCanvas()
    initSnowflakes()
    animate()
  }

  const deactivate = () => {
    if (!isActive.value) return

    isActive.value = false

    if (animationId !== null) {
      cancelAnimationFrame(animationId)
      animationId = null
    }

    removeCanvas()
    snowflakes = []
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
      attributeFilter: ['data-theme'],
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
    activate,
    deactivate,
  }
}

export default useSnowfall
