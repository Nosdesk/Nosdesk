import { watch, toValue, type MaybeRefOrGetter } from 'vue'

/**
 * Composable for reactive favicon management
 *
 * Following Vue 3 best practices from the official documentation:
 * - Uses MaybeRefOrGetter for flexible input types
 * - Uses watch with toValue() for proper reactivity tracking
 *
 * @see https://vuejs.org/guide/reusability/composables.html
 */

interface UseFaviconOptions {
  /** Base URL to prepend to relative paths */
  baseUrl?: string
  /** Default favicon paths to restore when url is null */
  defaultIcons?: Array<{
    rel: string
    type?: string
    sizes?: string
    href: string
  }>
}

const DEFAULT_ICONS = [
  { rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg' },
  { rel: 'icon', type: 'image/png', sizes: '32x32', href: '/favicon-32.png' },
  { rel: 'alternate icon', href: '/favicon.ico' }
]

/**
 * Get MIME type from file extension
 */
function getMimeType(url: string): string | undefined {
  if (url.endsWith('.svg')) return 'image/svg+xml'
  if (url.endsWith('.ico')) return 'image/x-icon'
  if (url.endsWith('.png')) return 'image/png'
  if (url.endsWith('.jpg') || url.endsWith('.jpeg')) return 'image/jpeg'
  if (url.endsWith('.gif')) return 'image/gif'
  if (url.endsWith('.webp')) return 'image/webp'
  return undefined
}

/**
 * Apply favicon to the document head
 */
function applyFavicon(url: string, baseUrl?: string): void {
  // Remove existing favicon links
  const existingIcons = document.querySelectorAll('link[rel*="icon"]')
  existingIcons.forEach(icon => icon.remove())

  // Build full URL
  const fullUrl = baseUrl ? `${baseUrl}${url}` : url
  const mimeType = getMimeType(fullUrl)

  // Create primary favicon link
  const link = document.createElement('link')
  link.rel = 'icon'
  if (mimeType) link.type = mimeType
  link.href = fullUrl
  document.head.appendChild(link)

  // Add apple-touch-icon for iOS
  const appleLink = document.createElement('link')
  appleLink.rel = 'apple-touch-icon'
  appleLink.href = fullUrl
  document.head.appendChild(appleLink)
}

/**
 * Restore default favicons
 */
function restoreDefaultFavicons(defaultIcons: typeof DEFAULT_ICONS): void {
  // Remove existing favicon links
  const existingIcons = document.querySelectorAll('link[rel*="icon"]')
  existingIcons.forEach(icon => icon.remove())

  // Add default icons
  defaultIcons.forEach(iconData => {
    const link = document.createElement('link')
    link.rel = iconData.rel
    if (iconData.type) link.type = iconData.type
    if (iconData.sizes) link.setAttribute('sizes', iconData.sizes)
    link.href = iconData.href
    document.head.appendChild(link)
  })
}

/**
 * Reactive favicon composable
 *
 * @param source - Reactive source for favicon URL (ref, getter, or plain value)
 * @param options - Configuration options
 *
 * @example
 * ```ts
 * // With a ref
 * const faviconUrl = ref('/custom-favicon.png')
 * useFavicon(faviconUrl)
 *
 * // With a computed/getter
 * useFavicon(() => brandingStore.faviconUrl)
 *
 * // With a store computed
 * const brandingStore = useBrandingStore()
 * useFavicon(computed(() => brandingStore.faviconUrl))
 * ```
 */
export function useFavicon(
  source: MaybeRefOrGetter<string | null | undefined>,
  options: UseFaviconOptions = {}
): void {
  const { baseUrl, defaultIcons = DEFAULT_ICONS } = options

  // Watch the source for changes and update favicon
  // Using watch instead of watchEffect for explicit dependency tracking
  // and to handle the immediate case properly
  watch(
    () => toValue(source),
    (newUrl) => {
      if (newUrl) {
        applyFavicon(newUrl, baseUrl)
      } else {
        restoreDefaultFavicons(defaultIcons)
      }
    },
    { immediate: true }
  )
}

export default useFavicon
