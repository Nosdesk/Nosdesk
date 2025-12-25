/**
 * CRT Screen Effect Composable
 *
 * Creates a full-screen CRT monitor effect with:
 * - Static horizontal scanlines
 * - Slowly moving refresh/scan band
 * - Subtle screen flicker
 * - Vignette/screen curvature effect
 *
 * Only active when the specified theme is applied (default: red-horizon).
 */
import { ref, onMounted, onUnmounted } from 'vue'

interface CrtEffectOptions {
  theme?: string
  scanlineOpacity?: number
  scanlineSize?: number
  refreshBandSpeed?: number // seconds for one full pass
  refreshBandOpacity?: number
  flickerIntensity?: number
  vignetteIntensity?: number
}

export function useCrtEffect(options: CrtEffectOptions = {}) {
  const {
    theme = 'red-horizon',
    scanlineOpacity = 0.03,
    scanlineSize = 2, // pixels
    refreshBandSpeed = 12, // seconds (slower)
    refreshBandOpacity = 0.015, // much more subtle
    flickerIntensity = 0.005,
    vignetteIntensity = 0.5
  } = options

  const isActive = ref(false)
  let overlay: HTMLDivElement | null = null
  let styleElement: HTMLStyleElement | null = null

  const createCrtOverlay = () => {
    // Create the main overlay container
    overlay = document.createElement('div')
    overlay.className = 'crt-screen-effect'
    overlay.setAttribute('aria-hidden', 'true')

    // Create style element for animations
    styleElement = document.createElement('style')
    styleElement.textContent = `
      .crt-screen-effect {
        position: fixed;
        inset: 0;
        pointer-events: none;
        z-index: 99998;
        overflow: hidden;
      }

      /* Static scanlines */
      .crt-scanlines {
        position: absolute;
        inset: 0;
        background: repeating-linear-gradient(
          0deg,
          transparent,
          transparent ${scanlineSize - 1}px,
          rgba(0, 0, 0, ${scanlineOpacity}) ${scanlineSize - 1}px,
          rgba(0, 0, 0, ${scanlineOpacity}) ${scanlineSize}px
        );
        pointer-events: none;
      }

      /* Moving refresh band - very subtle */
      .crt-refresh-band {
        position: absolute;
        left: 0;
        right: 0;
        height: 60px;
        background: linear-gradient(
          180deg,
          transparent 0%,
          rgba(255, 255, 255, ${refreshBandOpacity}) 40%,
          rgba(255, 255, 255, ${refreshBandOpacity * 1.2}) 50%,
          rgba(255, 255, 255, ${refreshBandOpacity}) 60%,
          transparent 100%
        );
        animation: crt-refresh ${refreshBandSpeed}s linear infinite;
        pointer-events: none;
      }

      @keyframes crt-refresh {
        0% {
          top: -60px;
        }
        100% {
          top: 100%;
        }
      }

      /* Subtle RGB shift / chromatic aberration on edges */
      .crt-rgb-shift {
        position: absolute;
        inset: 0;
        background:
          linear-gradient(90deg, rgba(255, 0, 0, 0.01) 0%, transparent 3%, transparent 97%, rgba(0, 255, 255, 0.01) 100%);
        pointer-events: none;
      }

      /* Vignette effect - stronger CRT screen edge darkening */
      .crt-vignette {
        position: absolute;
        inset: 0;
        background: radial-gradient(
          ellipse 80% 80% at center,
          transparent 0%,
          transparent 40%,
          rgba(0, 0, 0, ${vignetteIntensity * 0.15}) 60%,
          rgba(0, 0, 0, ${vignetteIntensity * 0.4}) 80%,
          rgba(0, 0, 0, ${vignetteIntensity * 0.7}) 95%,
          rgba(0, 0, 0, ${vignetteIntensity}) 100%
        );
        pointer-events: none;
      }

      /* CRT screen curvature simulation via box-shadow */
      .crt-curvature {
        position: absolute;
        inset: 0;
        border-radius: 12px;
        box-shadow:
          inset 0 0 80px 40px rgba(0, 0, 0, 0.15),
          inset 0 0 20px 10px rgba(0, 0, 0, 0.1);
        pointer-events: none;
      }

      /* Subtle flicker effect */
      .crt-flicker {
        position: absolute;
        inset: 0;
        background: transparent;
        animation: crt-flicker 0.15s infinite;
        pointer-events: none;
      }

      @keyframes crt-flicker {
        0%, 100% {
          opacity: 1;
        }
        50% {
          opacity: ${1 - flickerIntensity};
        }
      }

      /* Screen glow */
      .crt-glow {
        position: absolute;
        inset: 0;
        box-shadow: inset 0 0 100px rgba(255, 100, 50, 0.03);
        pointer-events: none;
      }

      /* Subtle horizontal noise lines that occasionally appear */
      .crt-noise {
        position: absolute;
        inset: 0;
        background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='100' height='1'%3E%3Crect fill='rgba(255,255,255,0.03)' width='100' height='1'/%3E%3C/svg%3E");
        background-repeat: repeat;
        opacity: 0;
        animation: crt-noise 15s steps(1) infinite;
        pointer-events: none;
      }

      @keyframes crt-noise {
        0%, 100% { opacity: 0; transform: translateY(0); }
        5% { opacity: 0.3; transform: translateY(25vh); }
        5.3% { opacity: 0; }
        35% { opacity: 0; }
        35.2% { opacity: 0.2; transform: translateY(60vh); }
        35.4% { opacity: 0; }
      }
    `

    document.head.appendChild(styleElement)

    // Build the overlay structure
    overlay.innerHTML = `
      <div class="crt-scanlines"></div>
      <div class="crt-refresh-band"></div>
      <div class="crt-rgb-shift"></div>
      <div class="crt-curvature"></div>
      <div class="crt-vignette"></div>
      <div class="crt-flicker"></div>
      <div class="crt-glow"></div>
      <div class="crt-noise"></div>
    `

    document.body.appendChild(overlay)
  }

  const removeCrtOverlay = () => {
    if (overlay) {
      overlay.remove()
      overlay = null
    }
    if (styleElement) {
      styleElement.remove()
      styleElement = null
    }
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
    createCrtOverlay()
  }

  const deactivate = () => {
    if (!isActive.value) return
    isActive.value = false
    removeCrtOverlay()
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
    activate,
    deactivate
  }
}

export default useCrtEffect
