<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useHorizontalScroll } from '@/composables/useHorizontalScroll'

const props = withDefaults(defineProps<{
  /** Additional classes for the scroll container */
  containerClass?: string
  /** Width of the fade gradient in pixels */
  fadeWidth?: number
  /** Show scroll hint dots below the container */
  showDots?: boolean
  /** Number of dots to show for scroll progress */
  dotCount?: number
  /** Enable drag-to-scroll on desktop */
  draggable?: boolean
  /** Background color for fade gradient (theme variable name without --color- prefix) */
  fadeBackground?: 'bg-surface' | 'bg-surface-alt' | 'bg-app'
}>(), {
  fadeWidth: 24,
  showDots: true,
  dotCount: 3,
  draggable: true,
  fadeBackground: 'bg-surface'
})

const scrollContainerRef = ref<HTMLElement | null>(null)
const { canScrollLeft, canScrollRight, isOverflowing, activeDotIndex } = useHorizontalScroll(scrollContainerRef, props.dotCount)

// Generate array of dot indices for v-for
const dotIndices = computed(() => Array.from({ length: props.dotCount }, (_, i) => i))

// Drag-to-scroll state
const isDragging = ref(false)
const hasDragged = ref(false) // Track if actual dragging occurred
const startX = ref(0)
const scrollLeftStart = ref(0)

const handleMouseDown = (e: MouseEvent) => {
  if (!props.draggable || !scrollContainerRef.value) return
  isDragging.value = true
  hasDragged.value = false
  startX.value = e.clientX
  scrollLeftStart.value = scrollContainerRef.value.scrollLeft
  scrollContainerRef.value.style.cursor = 'grabbing'

  // Add global listeners to handle mouse up outside the container
  document.addEventListener('mouseup', handleGlobalMouseUp)
  document.addEventListener('mousemove', handleGlobalMouseMove)
}

const handleGlobalMouseUp = () => {
  if (isDragging.value) {
    isDragging.value = false
    if (scrollContainerRef.value) {
      scrollContainerRef.value.style.cursor = 'grab'
    }
  }
  // Clean up global listeners
  document.removeEventListener('mouseup', handleGlobalMouseUp)
  document.removeEventListener('mousemove', handleGlobalMouseMove)

  // Reset hasDragged after a short delay to allow click events to check it
  setTimeout(() => {
    hasDragged.value = false
  }, 0)
}

const handleGlobalMouseMove = (e: MouseEvent) => {
  if (!isDragging.value || !scrollContainerRef.value) return
  e.preventDefault()

  const walk = startX.value - e.clientX
  // Only mark as dragged if we've moved more than a few pixels
  if (Math.abs(walk) > 3) {
    hasDragged.value = true
  }
  scrollContainerRef.value.scrollLeft = scrollLeftStart.value + walk
}

const handleWheel = (e: WheelEvent) => {
  if (!scrollContainerRef.value || !isOverflowing.value) return
  e.preventDefault()
  const delta = e.deltaY !== 0 ? e.deltaY : e.deltaX
  scrollContainerRef.value.scrollLeft += delta
}

// Handle click on container - prevent if we were dragging
const handleClick = (e: MouseEvent) => {
  if (hasDragged.value) {
    e.preventDefault()
    e.stopPropagation()
  }
}

// Click on dot to scroll to position
const scrollToDot = (dotIndex: number) => {
  if (!scrollContainerRef.value) return
  const { scrollWidth, clientWidth } = scrollContainerRef.value
  const maxScroll = scrollWidth - clientWidth
  const targetScroll = (dotIndex / (props.dotCount - 1)) * maxScroll
  scrollContainerRef.value.scrollTo({ left: targetScroll, behavior: 'smooth' })
}

// Cleanup on unmount
onUnmounted(() => {
  document.removeEventListener('mouseup', handleGlobalMouseUp)
  document.removeEventListener('mousemove', handleGlobalMouseMove)
})

// Expose ref and drag state for parent components
defineExpose({ scrollContainerRef, isDragging, hasDragged })
</script>

<template>
  <div class="horizontal-scroll-wrapper">
    <div class="horizontal-scroll-inner">
      <!-- Left fade indicator -->
      <div
        class="scroll-fade scroll-fade-left"
        :class="{ 'opacity-100': canScrollLeft, 'opacity-0': !canScrollLeft }"
        :style="{
          width: `${fadeWidth}px`,
          background: `linear-gradient(to right, var(--color-${fadeBackground}), transparent)`
        }"
      />

      <!-- Scroll container -->
      <div
        ref="scrollContainerRef"
        class="horizontal-scroll-container"
        :class="[containerClass, { 'cursor-grab select-none': draggable }]"
        @mousedown="handleMouseDown"
        @click.capture="handleClick"
        @wheel="handleWheel"
      >
        <slot />
      </div>

      <!-- Right fade indicator -->
      <div
        class="scroll-fade scroll-fade-right"
        :class="{ 'opacity-100': canScrollRight, 'opacity-0': !canScrollRight }"
        :style="{
          width: `${fadeWidth}px`,
          background: `linear-gradient(to left, var(--color-${fadeBackground}), transparent)`
        }"
      />
    </div>

    <!-- Scroll hint dots (only visible when overflowing and enabled) -->
    <div
      v-if="showDots && isOverflowing"
      class="scroll-hint-dots"
    >
      <button
        v-for="i in dotIndices"
        :key="i"
        type="button"
        class="scroll-dot"
        :class="{ 'opacity-100': i === activeDotIndex, 'opacity-30': i !== activeDotIndex }"
        @click="scrollToDot(i)"
        :aria-label="`Scroll to section ${i + 1}`"
      />
    </div>
  </div>
</template>

<style scoped>
.horizontal-scroll-wrapper {
  display: flex;
  flex-direction: column;
}

.horizontal-scroll-inner {
  position: relative;
}

.horizontal-scroll-container {
  display: flex;
  overflow-x: auto;
  scrollbar-width: none; /* Firefox */
  -ms-overflow-style: none; /* IE/Edge */
}

.horizontal-scroll-container::-webkit-scrollbar {
  display: none; /* Chrome/Safari/Opera */
}

/* Buttons inside the container should show pointer cursor */
.horizontal-scroll-container :deep(button),
.horizontal-scroll-container :deep(a) {
  cursor: pointer;
}

.scroll-fade {
  position: absolute;
  top: 0;
  bottom: 0;
  pointer-events: none;
  z-index: 10;
  transition: opacity 0.2s ease;
}

.scroll-fade-left {
  left: 0;
}

.scroll-fade-right {
  right: 0;
}

.scroll-hint-dots {
  display: flex;
  justify-content: center;
  gap: 6px;
  padding-top: 8px;
}

.scroll-dot {
  width: 6px;
  height: 6px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background-color: var(--color-text-tertiary);
  transition: opacity 0.2s ease, transform 0.15s ease;
  cursor: pointer;
}

.scroll-dot:hover {
  transform: scale(1.3);
  opacity: 0.8 !important;
}

.scroll-dot:focus {
  outline: none;
}

.scroll-dot:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}
</style>
