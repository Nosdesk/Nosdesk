<script setup lang="ts">
import { RouterLink, useRoute } from 'vue-router'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import { computed, ref } from 'vue'

const props = defineProps<{
  page: Page;
  isChild?: boolean;
  level?: number;
  isDragging?: boolean;
  isDropTarget?: boolean;
  draggedPageId?: string | number | null;
}>()

const emit = defineEmits<{
  (e: 'toggleExpand', id: string | number): void;
  (e: 'pageClick', id: string | number): void;
  (e: 'dragStart', id: string | number, event: DragEvent): void;
  (e: 'dragEnd', event: DragEvent): void;
  (e: 'dragOver', id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below', level: number): void;
  (e: 'drop', id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below'): void;
}>()

const route = useRoute()
const docNavStore = useDocumentationNavStore()
const dragPosition = ref<'above' | 'inside' | 'below' | null>(null)
const itemRef = ref<HTMLElement | null>(null)

// Calculate indent based on level - base padding of 8px plus 8px per level
const indentStyle = computed(() => ({
  '--indent-level': props.level ?? 0,
  paddingLeft: `${8 + (props.level ?? 0) * 8}px`
}))

// Check if this page is currently active
const isActive = computed(() => {
  return route.path === `/documentation/${props.page.id}` ||
         (typeof props.page.slug === 'string' && route.path === `/documentation/${props.page.slug}`)
})

// Check if this page has children
const hasChildren = computed(() => {
  return props.page.children && props.page.children.length > 0
})

// Check if this page is expanded
const isExpanded = computed(() => {
  return docNavStore.expandedPages[props.page.id]
})

// Check if an icon is an SVG
const isIconSvg = (icon: string | undefined): boolean => {
  return Boolean(icon && icon.startsWith('<svg'))
}

// Get all descendant IDs of a page recursively
const getAllDescendantIds = (page: Page): string[] => {
  const ids: string[] = []
  if (page.children && page.children.length > 0) {
    for (const child of page.children) {
      ids.push(String(child.id))
      ids.push(...getAllDescendantIds(child))
    }
  }
  return ids
}

// Check if this item is a descendant of the dragged page (would create circular reference)
const isDescendantOfDragged = computed(() => {
  if (!props.draggedPageId) return false
  // This check is done by the parent - we just check if the dragged page is an ancestor
  // If draggedPageId matches this page's ID, it's being dragged
  return String(props.draggedPageId) === String(props.page.id)
})

// Handle toggle expansion
const handleToggleExpand = (event: Event) => {
  event.stopPropagation()
  emit('toggleExpand', props.page.id)
}

// Handle page click
const handlePageClick = () => {
  emit('pageClick', props.page.id)
}

// Drag event handlers
const handleDragStart = (event: DragEvent) => {
  if (!event.dataTransfer) return
  event.dataTransfer.setData('text/plain', String(props.page.id))
  event.dataTransfer.effectAllowed = 'move'
  emit('dragStart', props.page.id, event)
}

const handleDragEnd = (event: DragEvent) => {
  dragPosition.value = null
  emit('dragEnd', event)
}

const handleDragOver = (event: DragEvent) => {
  event.preventDefault()
  if (!event.dataTransfer) return
  event.dataTransfer.dropEffect = 'move'

  // Calculate drop position based on mouse Y within the element
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
  const mouseY = event.clientY
  const relativeY = mouseY - rect.top
  const height = rect.height

  // Divide into thirds for above/inside/below
  if (relativeY < height * 0.25) {
    dragPosition.value = 'above'
  } else if (relativeY > height * 0.75) {
    dragPosition.value = 'below'
  } else {
    dragPosition.value = 'inside'
  }

  emit('dragOver', props.page.id, event, dragPosition.value, props.level ?? 0)
}

const handleDragLeave = (event: DragEvent) => {
  // Only clear if we're actually leaving this element (not entering a child)
  const relatedTarget = event.relatedTarget as HTMLElement
  if (!itemRef.value?.contains(relatedTarget)) {
    dragPosition.value = null
  }
}

const handleDrop = (event: DragEvent) => {
  event.preventDefault()
  if (dragPosition.value) {
    emit('drop', props.page.id, event, dragPosition.value)
  }
  dragPosition.value = null
}
</script>

<template>
  <li class="relative select-none">
    <!-- Main Item -->
    <div
      ref="itemRef"
      class="group relative flex items-center gap-1 py-1 pr-2 rounded text-xs cursor-pointer transition-all duration-150"
      :class="[
        isActive
          ? 'bg-surface text-primary font-medium'
          : 'text-secondary hover:text-primary hover:bg-surface-hover',
        isDragging && 'opacity-40 scale-[0.98]',
        (dragPosition === 'inside' || isDropTarget) && 'bg-blue-500/10 ring-1 ring-blue-500/30',
      ]"
      :style="indentStyle"
      draggable="true"
      @click.stop="handlePageClick"
      @dragstart="handleDragStart"
      @dragend="handleDragEnd"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <!-- Expand/Collapse Toggle (only shown for pages with children) -->
      <button
        v-if="hasChildren"
        class="flex-shrink-0 w-3 h-3 flex items-center justify-center text-tertiary hover:text-primary rounded transition-colors"
        @click.stop="handleToggleExpand"
        :aria-label="isExpanded ? 'Collapse' : 'Expand'"
      >
        <svg
          class="w-2.5 h-2.5 transition-transform duration-200"
          :class="{ 'rotate-90': isExpanded }"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="2.5"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
        </svg>
      </button>

      <!-- Page Icon -->
      <span class="flex-shrink-0 w-3.5 h-3.5 flex items-center justify-center leading-none">
        <span v-if="page.icon && !isIconSvg(page.icon)" class="text-xs leading-none flex items-center justify-center">{{ page.icon }}</span>
        <span v-else-if="page.icon && isIconSvg(page.icon)" v-safe-html.svg="page.icon" class="w-3.5 h-3.5 flex items-center justify-center"></span>
        <span v-else class="text-xs leading-none text-tertiary flex items-center justify-center">📄</span>
      </span>

      <!-- Page Title -->
      <span class="flex-1 truncate min-w-0">{{ page.title }}</span>

      <!-- Drag Handle (visible on hover) -->
      <span class="flex-shrink-0 opacity-0 group-hover:opacity-60 transition-opacity text-tertiary">
        <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 16 16">
          <circle cx="5" cy="4" r="1.5"/>
          <circle cx="11" cy="4" r="1.5"/>
          <circle cx="5" cy="8" r="1.5"/>
          <circle cx="11" cy="8" r="1.5"/>
          <circle cx="5" cy="12" r="1.5"/>
          <circle cx="11" cy="12" r="1.5"/>
        </svg>
      </span>
    </div>

    <!-- Children Container -->
    <div
      v-if="hasChildren && isExpanded"
      class="relative"
    >
      <!-- Vertical connecting line -->
      <div
        class="absolute top-0 bottom-2 w-px bg-border-default opacity-50"
        :style="{ left: `${8 + ((level ?? 0) + 1) * 8 + 6}px` }"
      ></div>

      <ul class="flex flex-col">
        <DocumentationNavItem
          v-for="child in page.children"
          :key="child.id"
          :page="child"
          :is-child="true"
          :level="(level ?? 0) + 1"
          :dragged-page-id="draggedPageId"
          :is-dragging="$attrs['is-dragging'] === String(child.id)"
          :is-drop-target="$attrs['is-drop-target'] === String(child.id)"
          @toggle-expand="(id) => emit('toggleExpand', id)"
          @page-click="(id) => emit('pageClick', id)"
          @drag-start="(id, event) => emit('dragStart', id, event)"
          @drag-end="(event) => emit('dragEnd', event)"
          @drag-over="(id, event, position, level) => emit('dragOver', id, event, position, level)"
          @drop="(id, event, position) => emit('drop', id, event, position)"
        />
      </ul>
    </div>
  </li>
</template>

<style scoped>
/* Smooth grab cursor for draggable items */
[draggable="true"] {
  cursor: grab;
}

[draggable="true"]:active {
  cursor: grabbing;
}
</style>
