// Performance monitoring utility for tracking API and image loading performance
import { ref, computed } from 'vue'

interface PerformanceMetric {
  id: string
  type: 'api' | 'image' | 'navigation'
  startTime: number
  endTime?: number
  duration?: number
  url?: string
  status?: 'success' | 'error' | 'cancelled'
  size?: number
  metadata?: Record<string, any>
}

class PerformanceMonitor {
  private metrics = ref<PerformanceMetric[]>([])
  private activeMetrics = new Map<string, PerformanceMetric>()
  private maxMetrics = 100 // Keep only last 100 metrics

  // Start tracking a performance metric
  startMetric(id: string, type: PerformanceMetric['type'], url?: string, metadata?: Record<string, any>): void {
    const metric: PerformanceMetric = {
      id,
      type,
      startTime: performance.now(),
      url,
      metadata
    }

    this.activeMetrics.set(id, metric)
  }

  // End tracking a performance metric
  endMetric(id: string, status: PerformanceMetric['status'] = 'success', size?: number): void {
    const metric = this.activeMetrics.get(id)
    if (!metric) return

    const endTime = performance.now()
    const completedMetric: PerformanceMetric = {
      ...metric,
      endTime,
      duration: endTime - metric.startTime,
      status,
      size
    }

    // Add to metrics array
    this.metrics.value.push(completedMetric)

    // Keep only the last N metrics
    if (this.metrics.value.length > this.maxMetrics) {
      this.metrics.value = this.metrics.value.slice(-this.maxMetrics)
    }

    // Remove from active metrics
    this.activeMetrics.delete(id)
  }

  // Cancel a metric (for cancelled requests)
  cancelMetric(id: string): void {
    this.endMetric(id, 'cancelled')
  }

  // Get performance statistics
  getStats() {
    const metrics = this.metrics.value
    const apiMetrics = metrics.filter(m => m.type === 'api' && m.duration)
    const imageMetrics = metrics.filter(m => m.type === 'image' && m.duration)

    return {
      total: metrics.length,
      api: {
        count: apiMetrics.length,
        avgDuration: apiMetrics.length > 0 ? apiMetrics.reduce((sum, m) => sum + (m.duration || 0), 0) / apiMetrics.length : 0,
        slowest: Math.max(...apiMetrics.map(m => m.duration || 0)),
        fastest: Math.min(...apiMetrics.map(m => m.duration || 0)),
        successRate: apiMetrics.filter(m => m.status === 'success').length / apiMetrics.length * 100
      },
      images: {
        count: imageMetrics.length,
        avgDuration: imageMetrics.length > 0 ? imageMetrics.reduce((sum, m) => sum + (m.duration || 0), 0) / imageMetrics.length : 0,
        slowest: Math.max(...imageMetrics.map(m => m.duration || 0)),
        fastest: Math.min(...imageMetrics.map(m => m.duration || 0)),
        successRate: imageMetrics.filter(m => m.status === 'success').length / imageMetrics.length * 100
      }
    }
  }

  // Get recent slow operations
  getSlowOperations(threshold: number = 1000) {
    return this.metrics.value
      .filter(m => m.duration && m.duration > threshold)
      .sort((a, b) => (b.duration || 0) - (a.duration || 0))
      .slice(0, 10)
  }

  // Clear all metrics
  clear(): void {
    this.metrics.value = []
    this.activeMetrics.clear()
  }

  // Get all metrics (for debugging)
  getAllMetrics(): PerformanceMetric[] {
    return [...this.metrics.value]
  }
}

// Create singleton instance
const performanceMonitor = new PerformanceMonitor()

// Vue composable for performance monitoring
export const usePerformanceMonitor = () => {
  // Track API request performance
  const trackApiRequest = (url: string, requestKey?: string) => {
    const id = requestKey || `api-${Date.now()}-${Math.random()}`
    performanceMonitor.startMetric(id, 'api', url)
    
    return {
      id,
      success: () => performanceMonitor.endMetric(id, 'success'),
      error: () => performanceMonitor.endMetric(id, 'error'),
      cancel: () => performanceMonitor.cancelMetric(id)
    }
  }

  // Track image loading performance
  const trackImageLoad = (url: string) => {
    const id = `image-${Date.now()}-${Math.random()}`
    performanceMonitor.startMetric(id, 'image', url)
    
    return {
      id,
      success: (size?: number) => performanceMonitor.endMetric(id, 'success', size),
      error: () => performanceMonitor.endMetric(id, 'error'),
      cancel: () => performanceMonitor.cancelMetric(id)
    }
  }

  // Track navigation performance
  const trackNavigation = (route: string) => {
    const id = `nav-${Date.now()}-${Math.random()}`
    performanceMonitor.startMetric(id, 'navigation', route)
    
    return {
      id,
      complete: () => performanceMonitor.endMetric(id, 'success')
    }
  }

  // Get performance statistics
  const stats = computed(() => performanceMonitor.getStats())
  
  // Get slow operations
  const slowOperations = computed(() => performanceMonitor.getSlowOperations())

  return {
    trackApiRequest,
    trackImageLoad,
    trackNavigation,
    stats,
    slowOperations,
    clear: performanceMonitor.clear.bind(performanceMonitor),
    getAllMetrics: performanceMonitor.getAllMetrics.bind(performanceMonitor)
  }
}

// Export trackImagePerformance function for backward compatibility
export const trackImagePerformance = (url: string, options: { cached?: boolean } = {}) => {
  const { trackImageLoad } = usePerformanceMonitor()
  const tracker = trackImageLoad(url)
  
  return {
    onLoad: (size?: number) => tracker.success(size),
    onError: () => tracker.error(),
    onCancel: () => tracker.cancel()
  }
}

// Helper function to wrap axios requests with performance tracking
export const withPerformanceTracking = async <T>(
  requestFn: () => Promise<T>,
  url: string,
  requestKey?: string
): Promise<T> => {
  const { trackApiRequest } = usePerformanceMonitor()
  const tracker = trackApiRequest(url, requestKey)
  
  try {
    const result = await requestFn()
    tracker.success()
    return result
  } catch (error) {
    const err = error as { name?: string; message?: string };
    if (err.name === 'AbortError' || err.name === 'CanceledError' || err.message === 'REQUEST_CANCELLED') {
      tracker.cancel()
    } else {
      tracker.error()
    }
    throw error
  }
}

// Development-only performance logger
export const logPerformanceStats = () => {
  if (import.meta.env.DEV) {
    const { stats, slowOperations } = usePerformanceMonitor()
    console.group('🚀 Performance Stats')
    console.table(stats.value)
    
    if (slowOperations.value.length > 0) {
      console.group('🐌 Slow Operations (>1s)')
      console.table(slowOperations.value)
      console.groupEnd()
    }
    console.groupEnd()
  }
} 