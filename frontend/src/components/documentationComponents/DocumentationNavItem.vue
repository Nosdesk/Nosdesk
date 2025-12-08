<script setup lang="ts">
import { RouterLink, useRoute } from 'vue-router'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import type { Page } from '@/services/documentationService'
import { computed, ref } from 'vue'

// Define props
const props = defineProps({
  page: {
    type: Object as () => Page,
    required: true
  },
  isChild: {
    type: Boolean,
    default: false
  },
  level: {
    type: Number,
    default: 0
  },
  isDragging: {
    type: Boolean,
    default: false
  },
  isDropTarget: {
    type: Boolean,
    default: false
  },
  isDropAbove: {
    type: Boolean,
    default: false
  },
  isDropBelow: {
    type: Boolean,
    default: false
  }
})

// Define emits
const emit = defineEmits<{
  (e: 'toggleExpand', id: string | number): void;
  (e: 'pageClick', id: string | number): void;
  (e: 'dragStart', id: string | number, event: DragEvent): void;
  (e: 'dragEnd', event: DragEvent): void;
  (e: 'dragOver', id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below'): void;
  (e: 'drop', id: string | number, event: DragEvent, position: 'above' | 'inside' | 'below'): void;
}>()

const route = useRoute()
const docNavStore = useDocumentationNavStore()
const dragPosition = ref<'above' | 'inside' | 'below' | null>(null)

// Computed classes for the page item based on level
const pageItemClasses = computed(() => {
  const classes: Record<string, boolean> = {
    'ml-0': props.level === 0,
    'ml-2': props.level === 1,
    'ml-4': props.level === 2,
    'ml-6': props.level === 3,
    'ml-8': props.level === 4,
    'ml-10': props.level >= 5,
    'opacity-50': props.isDragging,
    'border-t-2 border-blue-500': dragPosition.value === 'above' || props.isDropAbove,
    'border-b-2 border-blue-500': dragPosition.value === 'below' || props.isDropBelow,
    'bg-blue-900/30': dragPosition.value === 'inside' || props.isDropTarget,
  }
  
  // Add active page indicator
  const isActive = route.path === `/documentation/${props.page.id}` || 
                  (typeof props.page.slug === 'string' && route.path === `/documentation/${props.page.slug}`)
  
  classes['text-secondary'] = !isActive
  classes['text-primary bg-surface'] = isActive
  
  return classes
})

// Check if an icon is an SVG
const isIconSvg = (icon: string | undefined): boolean => {
  return Boolean(icon && icon.startsWith('<svg'))
}

// Handle toggle expansion
const handleToggleExpand = (id: string | number, event: Event) => {
  event.stopPropagation()
  emit('toggleExpand', id)
}

// Handle page click
const handlePageClick = (id: string | number) => {
  emit('pageClick', id)
}

// Handle drag events
const handleDragStart = (event: DragEvent) => {
  if (!event.dataTransfer) return
  // Set the drag data
  event.dataTransfer.setData('text/plain', String(props.page.id))
  event.dataTransfer.effectAllowed = 'move'
  
  // Emit drag start event
  emit('dragStart', props.page.id, event)
}

const handleDragEnd = (event: DragEvent) => {
  // Clear drag position
  dragPosition.value = null
  
  // Emit drag end event
  emit('dragEnd', event)
}

const handleDragOver = (event: DragEvent) => {
  event.preventDefault()
  
  if (!event.dataTransfer) return
  event.dataTransfer.dropEffect = 'move'
  
  // Determine drop position based on mouse position
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
  const mouseY = event.clientY
  const threshold = rect.height / 3
  
  if (mouseY < rect.top + threshold) {
    // Top third - drop above
    dragPosition.value = 'above'
  } else if (mouseY > rect.bottom - threshold) {
    // Bottom third - drop below
    dragPosition.value = 'below'
  } else {
    // Middle - drop inside (nest)
    dragPosition.value = 'inside'
  }
  
  // Emit drag over event
  emit('dragOver', props.page.id, event, dragPosition.value as 'above' | 'inside' | 'below')
}

const handleDragLeave = () => {
  // Clear drag position
  dragPosition.value = null
}

const handleDrop = (event: DragEvent) => {
  event.preventDefault()
  
  if (dragPosition.value) {
    // Emit drop event with position
    emit('drop', props.page.id, event, dragPosition.value)
  }
  
  // Clear drag position
  dragPosition.value = null
}

// Check if this page has children
const hasChildren = computed(() => {
  return props.page.children && props.page.children.length > 0;
});
</script>

<template>
  <li class="flex flex-col gap-0.5 relative nav-item">
    <!-- Main Page Item -->
    <div
      class="flex gap-1 text-xs font-medium px-2 py-1 hover:text-primary hover:bg-surface-hover rounded transition-colors items-center relative"
      :class="pageItemClasses"
      @click.stop="handlePageClick(page.id)"
      draggable="true"
      @dragstart="handleDragStart"
      @dragend="handleDragEnd"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      
      <!-- Expand/Collapse Arrow (only if has children) -->
      <span
        v-if="hasChildren"
        class="text-tertiary transition-transform duration-200 cursor-pointer flex-shrink-0 flex items-center justify-center hover:text-primary"
        :class="{ 'rotate-90': docNavStore.expandedPages[page.id] }"
        @click.stop="handleToggleExpand(page.id, $event)"
        aria-label="Toggle children"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-2.5 w-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
        </svg>
      </span>
      
      <!-- Page Icon and Title -->
      <div class="flex items-center flex-grow max-w-full gap-1" :class="{ 'ml-2.5': !hasChildren }">
        <!-- Page Icon -->
        <span class="mr-1 flex-shrink-0 w-3.5 h-3.5 flex items-center justify-center text-tertiary">
          <span v-if="page.icon && !isIconSvg(page.icon)" class="text-xs">{{ page.icon }}</span>
          <span v-else-if="page.icon && isIconSvg(page.icon)" v-safe-html.svg="page.icon" class="w-3.5 h-3.5"></span>
          <span v-else class="text-xs">📄</span>
        </span>
        
        <!-- Page Title -->
        <span class="truncate max-w-[calc(100%-2.5rem)]">{{ page.title }}</span>
      </div>
      
      <!-- Drag handle indicator that shows on hover -->
      <div class="opacity-0 group-hover:opacity-50 transition-opacity duration-200 ml-auto">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-2.5 w-2.5 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8h16M4 16h16" />
        </svg>
      </div>
    </div>
    
    <!-- Child Pages (only shown when expanded) -->
    <div v-if="docNavStore.expandedPages[page.id] && hasChildren" class="pl-1.5">
      <ul class="flex flex-col gap-0.5 border-l border-default pl-1 relative">
        <!-- Show a subtle expanding line animation when first expanded -->
        <div class="absolute left-0 top-0 bottom-0 w-px bg-border-subtle expand-line"></div>
        
        <DocumentationNavItem
          v-for="child in page.children"
          :key="child.id"
          :page="child"
          :is-child="true"
          :level="(level ?? 0) + 1"
          :is-dragging="$attrs['is-dragging'] === String(child.id)"
          :is-drop-target="$attrs['is-drop-target'] === String(child.id)"
          :is-drop-above="$attrs['is-drop-above'] === String(child.id)"
          :is-drop-below="$attrs['is-drop-below'] === String(child.id)"
          @toggle-expand="(id) => emit('toggleExpand', id)"
          @page-click="(id) => emit('pageClick', id)"
          @drag-start="(id, event) => emit('dragStart', id, event)"
          @drag-end="(event) => emit('dragEnd', event)"
          @drag-over="(id, event, position) => emit('dragOver', id, event, position)"
          @drop="(id, event, position) => emit('drop', id, event, position)"
        />
      </ul>
    </div>
  </li>
</template>

<style scoped>
.nav-item {
  position: relative;
  transition: all 0.2s ease-in-out;
}

/* Drag-and-drop visual indicators */
.border-t-2.border-blue-500 {
  position: relative;
}

.border-t-2.border-blue-500::before {
  content: "";
  position: absolute;
  top: -2px;
  left: 0;
  right: 0;
  height: 2px;
  background-color: #3b82f6;
  z-index: 10;
}

.border-b-2.border-blue-500 {
  position: relative;
}

.border-b-2.border-blue-500::after {
  content: "";
  position: absolute;
  bottom: -2px;
  left: 0;
  right: 0;
  height: 2px;
  background-color: #3b82f6;
  z-index: 10;
}

/* Cursor styles for draggable elements */
[draggable="true"] {
  cursor: grab;
}

[draggable="true"]:active {
  cursor: grabbing;
}

/* Animation for the dragged item */
[draggable="true"].opacity-50 {
  transform: scale(0.98);
  box-shadow: 0 0 0 1px rgba(59, 130, 246, 0.3);
}

/* Animation for the expanding line when children are shown */
.expand-line {
  animation: expandLineAnimation 0.3s ease-out forwards;
  transform-origin: top;
}

@keyframes expandLineAnimation {
  from {
    transform: scaleY(0);
    opacity: 0;
  }
  to {
    transform: scaleY(1);
    opacity: 1;
  }
}
</style> 