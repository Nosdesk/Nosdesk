<!-- PDFViewer.vue - Scrollable multi-page PDF viewer -->
<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { usePdfViewer } from '@/composables/usePdfViewer'

interface Props {
  src: string
  filename: string
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'ready'): void
  (e: 'error', error: any): void
}>()

const router = useRouter()
const route = useRoute()

// DOM refs
const scrollContainer = ref<HTMLElement | null>(null)
const pageRefs = ref<Map<number, HTMLElement>>(new Map())

// Current page tracking (based on scroll position)
const currentPage = ref(1)

// Fullscreen view detection
const isFullscreenView = computed(() => route.path === '/pdf-viewer')

// Initialize PDF viewer composable
const pdf = usePdfViewer({
  onReady: () => emit('ready'),
  onError: (error) => emit('error', error),
})

// IntersectionObserver for lazy rendering and cleanup
let observer: IntersectionObserver | null = null

const setupObserver = () => {
  if (!scrollContainer.value) return

  observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        const pageNum = parseInt(entry.target.getAttribute('data-page') || '0')
        if (pageNum <= 0) return

        if (entry.isIntersecting) {
          // Page is visible - render it
          renderPageIfNeeded(pageNum)
        } else {
          // Page is not visible - clear canvas to free memory (for large PDFs)
          // Only clear if page is far from viewport (more than 500px away)
          const rect = entry.boundingClientRect
          const containerRect = scrollContainer.value?.getBoundingClientRect()
          if (containerRect) {
            const distance = Math.min(
              Math.abs(rect.bottom - containerRect.top),
              Math.abs(rect.top - containerRect.bottom)
            )
            if (distance > 1000) {
              clearPageCanvas(pageNum)
            }
          }
        }
      })
    },
    {
      root: scrollContainer.value,
      rootMargin: '200px 0px', // Preload pages 200px before visible
      threshold: 0,
    }
  )
}

const renderPageIfNeeded = async (pageNum: number) => {
  const pageEl = pageRefs.value.get(pageNum)
  if (!pageEl) return

  const canvas = pageEl.querySelector('canvas') as HTMLCanvasElement
  if (!canvas) return

  await pdf.renderPage(pageNum, canvas)
}

// Clear canvas to free memory
const clearPageCanvas = (pageNum: number) => {
  if (!pdf.renderedPages.value.has(pageNum)) return

  const pageEl = pageRefs.value.get(pageNum)
  if (!pageEl) return

  const canvas = pageEl.querySelector('canvas') as HTMLCanvasElement
  if (!canvas) return

  const ctx = canvas.getContext('2d')
  if (ctx) {
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    canvas.width = 0
    canvas.height = 0
  }
  pdf.renderedPages.value.delete(pageNum)
}

// Track current page based on scroll position
const updateCurrentPage = () => {
  if (!scrollContainer.value) return

  const container = scrollContainer.value
  const scrollTop = container.scrollTop
  const containerHeight = container.clientHeight

  // Find the page that's most visible
  let mostVisiblePage = 1
  let maxVisibility = 0

  pageRefs.value.forEach((el, pageNum) => {
    const rect = el.getBoundingClientRect()
    const containerRect = container.getBoundingClientRect()

    const visibleTop = Math.max(rect.top, containerRect.top)
    const visibleBottom = Math.min(rect.bottom, containerRect.bottom)
    const visibleHeight = Math.max(0, visibleBottom - visibleTop)

    if (visibleHeight > maxVisibility) {
      maxVisibility = visibleHeight
      mostVisiblePage = pageNum
    }
  })

  currentPage.value = mostVisiblePage
}

// Throttled scroll handler
let scrollTimeout: number | null = null
const handleScroll = () => {
  if (scrollTimeout) return
  scrollTimeout = window.setTimeout(() => {
    updateCurrentPage()
    scrollTimeout = null
  }, 100)
}

// Register page element ref
const setPageRef = (pageNum: number, el: HTMLElement | null) => {
  if (el) {
    pageRefs.value.set(pageNum, el)
    observer?.observe(el)
  } else {
    const existing = pageRefs.value.get(pageNum)
    if (existing) {
      observer?.unobserve(existing)
    }
    pageRefs.value.delete(pageNum)
  }
}

// Re-render visible pages when scale changes
const handleScaleChange = async () => {
  await nextTick()

  // Re-render all pages that have canvases (they were previously rendered)
  // Note: renderedPages is cleared by the composable, so we check DOM instead
  pageRefs.value.forEach((el, pageNum) => {
    const canvas = el.querySelector('canvas')
    if (canvas) {
      // Check if canvas has been rendered (has dimensions)
      if (canvas.width > 0) {
        pdf.renderPage(pageNum, canvas as HTMLCanvasElement)
      }
    }
  })
}

watch(() => pdf.scale.value, handleScaleChange)

// Open fullscreen view
const openFullscreen = () => {
  const queryParams = {
    src: encodeURIComponent(props.src),
    filename: encodeURIComponent(props.filename),
    page: currentPage.value,
  }
  const queryString = Object.entries(queryParams)
    .map(([key, value]) => `${key}=${value}`)
    .join('&')
  router.push(`/pdf-viewer?${queryString}`)
}

// Fit PDF to container width
const fitToContainerWidth = () => {
  if (scrollContainer.value) {
    pdf.fitToWidth(scrollContainer.value.clientWidth)
  }
}

// Load document on mount or src change
watch(
  () => props.src,
  async (newSrc) => {
    if (newSrc) {
      await pdf.loadDocument(newSrc)
      await nextTick()
      setupObserver()
      // Auto-fit to width on initial load
      fitToContainerWidth()
    }
  },
  { immediate: true }
)

// Handle window resize - re-fit to width
let resizeTimeout: number | null = null
const handleResize = () => {
  if (resizeTimeout) return
  resizeTimeout = window.setTimeout(() => {
    fitToContainerWidth()
    resizeTimeout = null
  }, 150)
}

onMounted(() => {
  if (scrollContainer.value) {
    scrollContainer.value.addEventListener('scroll', handleScroll, { passive: true })
  }
  window.addEventListener('resize', handleResize, { passive: true })
})

onBeforeUnmount(() => {
  if (scrollContainer.value) {
    scrollContainer.value.removeEventListener('scroll', handleScroll)
  }
  window.removeEventListener('resize', handleResize)
  if (observer) {
    observer.disconnect()
    observer = null
  }
  if (scrollTimeout) {
    clearTimeout(scrollTimeout)
  }
  if (resizeTimeout) {
    clearTimeout(resizeTimeout)
  }
})
</script>

<template>
  <div
    class="pdf-viewer flex flex-col h-full w-full overflow-hidden"
    role="document"
    aria-label="PDF viewer"
  >
    <!-- Loading indicator -->
    <div
      v-if="pdf.isLoading.value"
      class="absolute inset-0 bg-surface/80 flex items-center justify-center z-20"
    >
      <div class="flex flex-col items-center gap-3">
        <svg
          class="animate-spin h-8 w-8 text-accent"
          xmlns="http://www.w3.org/2000/svg"
          fill="none"
          viewBox="0 0 24 24"
        >
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
          <path
            class="opacity-75"
            fill="currentColor"
            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
          />
        </svg>
        <span class="text-sm text-secondary">Loading PDF...</span>
      </div>
    </div>

    <!-- Scrollable PDF Container -->
    <div
      ref="scrollContainer"
      class="flex-1 min-h-0 overflow-auto bg-surface-alt"
    >
      <div class="pdf-pages flex flex-col items-center gap-4 p-4">
        <!-- Page placeholders with lazy-loaded canvases -->
        <div
          v-for="pageNum in pdf.totalPages.value"
          :key="pageNum"
          :ref="(el) => setPageRef(pageNum, el as HTMLElement)"
          :data-page="pageNum"
          class="pdf-page-wrapper flex justify-center flex-shrink-0"
          :style="{
            width: pdf.getScaledMaxWidth() + 'px',
          }"
        >
          <div
            class="pdf-page bg-white shadow-lg rounded overflow-hidden flex items-center justify-center"
            :style="{
              width: pdf.getPageDimensions(pageNum)?.width + 'px',
              height: pdf.getPageDimensions(pageNum)?.height + 'px',
            }"
          >
            <canvas class="block" />
          </div>
        </div>
      </div>
    </div>

    <!-- Controls Bar -->
    <div class="flex-shrink-0 bg-surface border-t border-default px-4 py-2">
      <div class="flex items-center justify-between gap-4 max-w-xl mx-auto">
        <!-- Page indicator -->
        <div class="flex items-center gap-2 text-sm text-secondary">
          <span class="font-medium text-primary">{{ currentPage }}</span>
          <span>/</span>
          <span>{{ pdf.totalPages.value }}</span>
        </div>

        <!-- Zoom controls -->
        <div class="flex items-center gap-2">
          <button
            @click="pdf.zoomOut"
            :disabled="pdf.scale.value <= 0.5"
            class="p-1.5 rounded text-secondary hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed"
            title="Zoom Out"
            aria-label="Zoom out"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 12H4" />
            </svg>
          </button>

          <span class="text-sm text-secondary min-w-[3rem] text-center">
            {{ Math.round(pdf.scale.value * 100) }}%
          </span>

          <button
            @click="pdf.zoomIn"
            :disabled="pdf.scale.value >= 3.0"
            class="p-1.5 rounded text-secondary hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed"
            title="Zoom In"
            aria-label="Zoom in"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
            </svg>
          </button>

          <button
            @click="fitToContainerWidth"
            class="p-1.5 rounded text-secondary hover:bg-surface-hover"
            title="Fit to Width"
            aria-label="Fit to width"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
            </svg>
          </button>
        </div>

        <!-- Fullscreen / Download -->
        <div class="flex items-center gap-2">
          <button
            v-if="!isFullscreenView"
            @click="openFullscreen"
            class="p-1.5 rounded text-secondary hover:bg-surface-hover"
            title="Fullscreen"
            aria-label="Open fullscreen"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5v-4m0 4h-4m4 0l-5-5"
              />
            </svg>
          </button>

          <a
            :href="src"
            target="_blank"
            :download="filename"
            class="p-1.5 rounded text-secondary hover:bg-surface-hover"
            title="Download PDF"
            aria-label="Download PDF"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
              />
            </svg>
          </a>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.pdf-pages {
  min-height: 100%;
}

.pdf-page {
  /* Ensure pages don't get squished */
  flex-shrink: 0;
}

.pdf-page canvas {
  display: block;
}
</style>
