<!-- TicketDragPreview.vue - Shared floating drag preview for tickets -->
<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue'

const props = defineProps<{
  ticket: {
    id: number
    title: string
    priority?: 'low' | 'medium' | 'high'
    assignee?: string | null
  }
  position: { x: number; y: number }
}>()

const CURSOR_OFFSET = 12 // Distance from cursor
const VIEWPORT_MARGIN = 8

// Refs for actual element measurement and positioning
const previewRef = ref<HTMLElement | null>(null)
const transformX = ref(0)
const transformY = ref(0)

// Update position using RAF for smooth cursor following
let rafId: number | null = null

const updatePosition = () => {
  if (!previewRef.value) return

  const { x, y } = props.position
  const rect = previewRef.value.getBoundingClientRect()
  const width = rect.width || 288
  const height = rect.height || 100

  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight

  // Check available space
  const spaceRight = viewportWidth - x
  const spaceBottom = viewportHeight - y

  // Determine placement (prefer bottom-right)
  const placeRight = spaceRight >= width + CURSOR_OFFSET
  const placeBottom = spaceBottom >= height + CURSOR_OFFSET

  let left: number
  let top: number

  if (placeRight) {
    left = x + CURSOR_OFFSET
  } else {
    left = x - width - CURSOR_OFFSET
  }

  if (placeBottom) {
    top = y + CURSOR_OFFSET
  } else {
    top = y - height - CURSOR_OFFSET
  }

  // Clamp to viewport
  left = Math.max(VIEWPORT_MARGIN, Math.min(left, viewportWidth - width - VIEWPORT_MARGIN))
  top = Math.max(VIEWPORT_MARGIN, Math.min(top, viewportHeight - height - VIEWPORT_MARGIN))

  transformX.value = left
  transformY.value = top
}

// Watch position changes and update via RAF
watch(() => props.position, () => {
  if (rafId) cancelAnimationFrame(rafId)
  rafId = requestAnimationFrame(updatePosition)
}, { immediate: true })

onMounted(() => {
  // Initial position calculation after element is mounted
  requestAnimationFrame(updatePosition)
})

onUnmounted(() => {
  if (rafId) cancelAnimationFrame(rafId)
})

const getPriorityBorderClass = (priority?: string) => {
  switch (priority) {
    case 'high': return 'border-l-priority-high'
    case 'medium': return 'border-l-priority-medium'
    case 'low': return 'border-l-priority-low'
    default: return 'border-l-subtle'
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      ref="previewRef"
      class="fixed top-0 left-0 pointer-events-none z-[9999] w-64 md:w-72 will-change-transform"
      :style="{
        transform: `translate3d(${transformX}px, ${transformY}px, 0)`
      }"
    >
      <div
        class="bg-surface rounded-lg border-l-4 border border-accent shadow-lg p-3"
        :class="getPriorityBorderClass(ticket.priority)"
      >
        <span class="text-xs text-tertiary font-mono">#{{ ticket.id }}</span>
        <h4 class="text-sm font-medium text-primary mt-1 line-clamp-2">
          {{ ticket.title }}
        </h4>
        <div v-if="ticket.assignee || ticket.priority" class="flex items-center justify-between mt-3">
          <span class="text-xs text-tertiary">
            {{ ticket.assignee || 'Unassigned' }}
          </span>
          <span v-if="ticket.priority" class="text-xs text-tertiary capitalize">
            {{ ticket.priority }}
          </span>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
