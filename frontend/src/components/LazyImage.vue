<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue'

const props = defineProps<{
  src?: string | null
  alt?: string
  fallback?: string
  width?: string | number
  height?: string | number
  class?: string
  loading?: 'lazy' | 'eager'
  threshold?: number
  objectFit?: 'cover' | 'contain' | 'fill' | 'none' | 'scale-down'
}>()

const emit = defineEmits<{
  load: []
  error: []
}>()

const imageRef = ref<HTMLImageElement | null>(null)
const isLoaded = ref(false)
const hasError = ref(false)
const isIntersecting = ref(false)
const observer = ref<IntersectionObserver | null>(null)

// Computed image source with fallback
const imageSrc = computed(() => {
  if (hasError.value && props.fallback) {
    return props.fallback
  }
  return props.src || props.fallback || ''
})

// Should load image when intersecting or eager loading
const shouldLoad = computed(() => {
  return props.loading === 'eager' || isIntersecting.value
})

// Handle image load
const handleLoad = () => {
  isLoaded.value = true
  hasError.value = false
  emit('load')
}

// Handle image error
const handleError = () => {
  hasError.value = true
  if (props.fallback && props.src !== props.fallback) {
    // Try fallback if available and not already using it
    return
  }
  emit('error')
}

// Set up intersection observer
onMounted(() => {
  if (props.loading === 'lazy' && imageRef.value) {
    observer.value = new IntersectionObserver(
      (entries) => {
        const entry = entries[0]
        if (entry.isIntersecting) {
          isIntersecting.value = true
          // Disconnect observer once image is in view
          if (observer.value) {
            observer.value.disconnect()
          }
        }
      },
      {
        threshold: props.threshold || 0.1,
        rootMargin: '50px' // Start loading 50px before entering viewport
      }
    )
    
    observer.value.observe(imageRef.value)
  } else {
    // For eager loading, immediately set as intersecting
    isIntersecting.value = true
  }
})

// Cleanup observer
onUnmounted(() => {
  if (observer.value) {
    observer.value.disconnect()
  }
})
</script>

<template>
  <div 
    :class="[
      'relative overflow-hidden',
      props.class
    ]"
    :style="{
      width: typeof width === 'number' ? `${width}px` : width,
      height: typeof height === 'number' ? `${height}px` : height
    }"
  >
    <!-- Loading placeholder -->
    <div
      v-if="!isLoaded && shouldLoad"
      class="absolute inset-0 bg-surface-alt animate-pulse flex items-center justify-center"
    >
      <svg
        class="w-6 h-6 text-tertiary"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"
        />
      </svg>
    </div>

    <!-- Placeholder when not loading yet -->
    <div
      v-else-if="!shouldLoad"
      class="absolute inset-0 bg-surface flex items-center justify-center"
    >
      <svg
        class="w-6 h-6 text-tertiary"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path 
          stroke-linecap="round" 
          stroke-linejoin="round" 
          stroke-width="2" 
          d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"
        />
      </svg>
    </div>

    <!-- Actual image -->
    <img
      v-if="shouldLoad && imageSrc"
      ref="imageRef"
      :src="imageSrc"
      :alt="alt || ''"
      :class="[
        'w-full h-full transition-opacity duration-300',
        `object-${objectFit || 'cover'}`,
        isLoaded ? 'opacity-100' : 'opacity-0'
      ]"
      @load="handleLoad"
      @error="handleError"
    />

    <!-- Error state -->
    <div
      v-if="hasError && !props.fallback"
      class="absolute inset-0 bg-surface-alt flex items-center justify-center"
    >
      <svg
        class="w-6 h-6 text-status-error"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
    </div>
  </div>
</template> 