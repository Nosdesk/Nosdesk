import { ref, onMounted, onUnmounted, nextTick } from 'vue'
import { trackImagePerformance } from '@/utils/performanceMonitor'

// Global image cache to prevent re-downloading the same images across components
const globalImageCache = new Map<string, { 
  url: string; 
  loaded: boolean; 
  error: boolean; 
  timestamp: number;
  blob?: Blob;
}>()

// Active image requests for cancellation
const activeRequests = new Map<string, AbortController>()

// Request priority queue
interface PriorityRequest {
  url: string
  priority: number
  timestamp: number
  abortController: AbortController
}

const requestQueue: PriorityRequest[] = []
const MAX_CONCURRENT_REQUESTS = 6 // Increased for better performance
let activeRequestCount = 0

// Cache cleanup interval (5 minutes)
const CACHE_CLEANUP_INTERVAL = 5 * 60 * 1000
// Cache expiry time (30 minutes - standard practice)
const CACHE_EXPIRY_TIME = 30 * 60 * 1000

// Cleanup expired cache entries periodically (singleton, runs for app lifetime)
const cleanupInterval = window.setInterval(() => {
  const now = Date.now()
  for (const [url, entry] of globalImageCache.entries()) {
    if (now - entry.timestamp > CACHE_EXPIRY_TIME) {
      globalImageCache.delete(url)
    }
  }
}, CACHE_CLEANUP_INTERVAL)

// Clean up interval during HMR to prevent duplicates
if (import.meta.hot) {
  import.meta.hot.dispose(() => clearInterval(cleanupInterval))
}

// Cancel all pending requests with lower priority
const cancelLowerPriorityRequests = (currentPriority: number) => {
  for (const [url, controller] of activeRequests.entries()) {
    const queuedRequest = requestQueue.find(req => req.url === url)
    if (queuedRequest && queuedRequest.priority > currentPriority) {
      controller.abort()
      activeRequests.delete(url)
      activeRequestCount = Math.max(0, activeRequestCount - 1)
    }
  }
  
  // Remove cancelled requests from queue
  for (let i = requestQueue.length - 1; i >= 0; i--) {
    if (requestQueue[i].priority > currentPriority) {
      requestQueue.splice(i, 1)
    }
  }
}

// Process the request queue
const processRequestQueue = () => {
  while (requestQueue.length > 0 && activeRequestCount < MAX_CONCURRENT_REQUESTS) {
    // Sort by priority (lower number = higher priority) and timestamp
    requestQueue.sort((a, b) => {
      if (a.priority !== b.priority) return a.priority - b.priority
      return a.timestamp - b.timestamp
    })
    
    const request = requestQueue.shift()
    if (!request) break
    
    activeRequestCount++
    // The actual request processing happens in preloadImage
  }
}

export interface UseImageOptimizationOptions {
  lazy?: boolean
  rootMargin?: string
  threshold?: number
  enableCache?: boolean
  preloadOnHover?: boolean
  enablePerformanceTracking?: boolean
  priority?: 'critical' | 'high' | 'medium' | 'low'
}

export function useImageOptimization(options: UseImageOptimizationOptions = {}) {
  const {
    lazy = true,
    rootMargin = '50px',
    threshold = 0.1,
    enableCache = true,
    preloadOnHover = false,
    enablePerformanceTracking = true,
    priority = 'medium'
  } = options

  // Convert priority to numeric value
  const priorityMap = { critical: 0, high: 1, medium: 2, low: 3 }
  const numericPriority = priorityMap[priority]

  // Reactive state
  const imageLoading = ref(false)
  const imageError = ref(false)
  const imageLoaded = ref(false)
  const shouldLoadImage = ref(!lazy)
  const elementRef = ref<HTMLElement>()

  // Intersection Observer for lazy loading
  let observer: IntersectionObserver | null = null
  let currentAbortController: AbortController | null = null

  // Setup intersection observer for lazy loading
  const setupLazyLoading = () => {
    if (!lazy || !elementRef.value) return

    observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting && !shouldLoadImage.value) {
            shouldLoadImage.value = true
            observer?.disconnect()
          }
        })
      },
      {
        rootMargin,
        threshold
      }
    )

    observer.observe(elementRef.value)
  }

  // Preload image with caching, performance tracking, and cancellation
  const preloadImage = async (url: string): Promise<void> => {
    if (!url) return Promise.reject(new Error('No URL provided'))

    // Cancel any existing request for this component
    if (currentAbortController) {
      currentAbortController.abort()
      activeRequests.delete(url)
      activeRequestCount = Math.max(0, activeRequestCount - 1)
    }

    return new Promise((resolve, reject) => {
      // Check cache first if enabled
      const cached = enableCache ? globalImageCache.get(url) : null
      const isCached = cached && Date.now() - cached.timestamp < CACHE_EXPIRY_TIME

      // Setup performance tracking
      const performanceTracker = enablePerformanceTracking 
        ? trackImagePerformance(url, { cached: !!(isCached && cached?.loaded) })
        : null

      if (isCached) {
        if (cached.loaded) {
          imageLoaded.value = true
          imageError.value = false
          imageLoading.value = false
          performanceTracker?.onLoad()
          resolve()
          return
        } else if (cached.error) {
          imageError.value = true
          imageLoaded.value = false
          imageLoading.value = false
          performanceTracker?.onError()
          reject(new Error('Cached image failed to load'))
          return
        }
      }

      // Cancel lower priority requests if this is high priority
      if (numericPriority <= 1) { // critical or high priority
        cancelLowerPriorityRequests(numericPriority)
      }

      // Create abort controller for this request
      const abortController = new AbortController()
      currentAbortController = abortController
      activeRequests.set(url, abortController)

      // Add to priority queue
      requestQueue.push({
        url,
        priority: numericPriority,
        timestamp: Date.now(),
        abortController
      })

      // Process queue
      processRequestQueue()

      // Create new image element for preloading
      const img = new Image()
      
      // Set loading state
      imageLoading.value = true
      imageError.value = false
      imageLoaded.value = false

      const cleanup = () => {
        activeRequests.delete(url)
        activeRequestCount = Math.max(0, activeRequestCount - 1)
        processRequestQueue() // Process next items in queue
      }
      
      img.onload = () => {
        if (abortController.signal.aborted) return
        
        if (enableCache) {
          globalImageCache.set(url, { 
            url, 
            loaded: true, 
            error: false, 
            timestamp: Date.now() 
          })
        }
        imageLoaded.value = true
        imageError.value = false
        imageLoading.value = false
        performanceTracker?.onLoad()
        cleanup()
        resolve()
      }
      
      img.onerror = () => {
        if (abortController.signal.aborted) return
        
        if (enableCache) {
          globalImageCache.set(url, { 
            url, 
            loaded: false, 
            error: true, 
            timestamp: Date.now() 
          })
        }
        imageError.value = true
        imageLoaded.value = false
        imageLoading.value = false
        performanceTracker?.onError()
        cleanup()
        reject(new Error('Failed to load image'))
      }

      // Handle abort
      abortController.signal.addEventListener('abort', () => {
        imageLoading.value = false
        cleanup()
        reject(new Error('Request cancelled'))
      })
      
      // Set cache entry as loading if enabled
      if (enableCache) {
        globalImageCache.set(url, { 
          url, 
          loaded: false, 
          error: false, 
          timestamp: Date.now() 
        })
      }
      
      // Add crossorigin attribute for better caching
      img.crossOrigin = 'anonymous'
      img.src = url
    })
  }

  // Load image when conditions are met
  const loadImage = async (url: string) => {
    if (!url || !shouldLoadImage.value) return

    try {
      await preloadImage(url)
    } catch (err) {
      if (err instanceof Error && err.message !== 'Request cancelled') {
        console.warn('Failed to preload image:', err)
      }
    }
  }

  // Setup hover preloading
  const setupHoverPreloading = (url: string) => {
    if (!preloadOnHover || !elementRef.value) return

    const handleMouseEnter = () => {
      if (!imageLoaded.value && !imageLoading.value && !imageError.value) {
        loadImage(url)
      }
    }

    elementRef.value.addEventListener('mouseenter', handleMouseEnter, { once: true })
    
    return () => {
      elementRef.value?.removeEventListener('mouseenter', handleMouseEnter)
    }
  }

  // Initialize lazy loading
  const initializeLazyLoading = async () => {
    await nextTick()
    if (lazy && elementRef.value) {
      setupLazyLoading()
    }
  }

  // Reset state
  const resetState = () => {
    imageLoading.value = false
    imageError.value = false
    imageLoaded.value = false
    shouldLoadImage.value = !lazy
  }

  // Cleanup
  const cleanup = () => {
    if (observer) {
      observer.disconnect()
      observer = null
    }
    if (currentAbortController) {
      currentAbortController.abort()
      currentAbortController = null
    }
  }

  // Lifecycle hooks
  onMounted(() => {
    initializeLazyLoading()
  })

  onUnmounted(() => {
    cleanup()
  })

  // Utility functions
  const getCacheStats = () => {
    return {
      size: globalImageCache.size,
      entries: Array.from(globalImageCache.entries()).map(([url, entry]) => ({
        url,
        loaded: entry.loaded,
        error: entry.error,
        age: Date.now() - entry.timestamp
      }))
    }
  }

  const clearCache = () => {
    globalImageCache.clear()
  }

  const preloadImages = async (urls: string[]) => {
    const promises = urls.map(url => preloadImage(url).catch(() => {}))
    await Promise.allSettled(promises)
  }

  // Prefetch images that are likely to be needed soon
  const prefetchImages = async (urls: string[], prefetchPriority: 'high' | 'low' = 'low') => {
    // Use requestIdleCallback for low priority prefetching
    if (prefetchPriority === 'low' && 'requestIdleCallback' in window) {
      return new Promise<void>((resolve) => {
        window.requestIdleCallback(() => {
          preloadImages(urls).then(() => resolve())
        })
      })
    } else {
      return preloadImages(urls)
    }
  }

  return {
    // Reactive state
    imageLoading,
    imageError,
    imageLoaded,
    shouldLoadImage,
    elementRef,
    
    // Methods
    loadImage,
    preloadImage,
    resetState,
    setupHoverPreloading,
    initializeLazyLoading,
    
    // Utilities
    getCacheStats,
    clearCache,
    preloadImages,
    prefetchImages
  }
} 