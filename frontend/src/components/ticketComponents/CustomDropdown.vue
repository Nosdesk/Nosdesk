<!-- components/CustomDropdown.vue -->
<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import { useThemeStore } from '@/stores/theme'

const themeStore = useThemeStore()

const props = defineProps<{
  value: string
  options: { value: string; label: string }[]
  type: 'status' | 'priority'
}>()

const emit = defineEmits<{
  (e: 'update:value', value: string): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuRef = ref<HTMLElement | null>(null)

// Reactive position tracking for scroll updates
const menuPosition = ref({ top: 0, left: 0, width: 0 })

const selectedOption = computed(() =>
  props.options.find(option => option.value === props.value)
)

// Update position based on trigger element using fixed positioning
const updatePosition = () => {
  if (!triggerRef.value) return

  const rect = triggerRef.value.getBoundingClientRect()
  const viewportHeight = window.innerHeight
  const spaceBelow = viewportHeight - rect.bottom
  const menuHeight = Math.min(props.options.length * 40 + 8, 200)

  // Open upward if not enough space below
  const openUpward = spaceBelow < menuHeight && rect.top > menuHeight

  menuPosition.value = {
    // Align with top of trigger, offset by menu height if opening upward
    top: openUpward ? rect.top - menuHeight : rect.top,
    left: rect.left,
    width: Math.max(rect.width, 150)
  }
}

const openDropdown = async () => {
  isOpen.value = true
  // Wait for next tick so the menu is rendered, then update position
  await nextTick()
  updatePosition()
}

const closeDropdown = () => {
  isOpen.value = false
}

const toggleDropdown = () => {
  if (isOpen.value) {
    closeDropdown()
  } else {
    openDropdown()
  }
}

const selectOption = (option: { value: string; label: string }) => {
  emit('update:value', option.value)
  closeDropdown()
}

// Close dropdown when clicking outside - use mousedown for better UX
const handleClickOutside = (event: MouseEvent) => {
  if (!isOpen.value) return

  const target = event.target as Node

  // Check if click is on trigger
  if (triggerRef.value?.contains(target)) return

  // Check if click is on menu (need to query by class since ref might not be set yet)
  const menu = document.querySelector('.custom-dropdown-menu')
  if (menu?.contains(target)) return

  closeDropdown()
}

// Update position on scroll using requestAnimationFrame for smooth performance
let rafId: number | null = null
const handleScroll = () => {
  if (!isOpen.value) return

  if (rafId) {
    cancelAnimationFrame(rafId)
  }

  rafId = requestAnimationFrame(() => {
    updatePosition()
    rafId = null
  })
}

// Handle resize
const handleResize = () => {
  if (isOpen.value) {
    updatePosition()
  }
}

// Theme-aware status indicator colors
const getStatusColor = (value: string) => {
  if (props.type === 'status') {
    switch (value) {
      case 'open':
        return 'bg-status-open'
      case 'in-progress':
        return 'bg-status-in-progress'
      case 'closed':
        return 'bg-status-closed'
      default:
        return 'bg-tertiary'
    }
  } else if (props.type === 'priority') {
    switch (value) {
      case 'low':
        return 'bg-priority-low'
      case 'medium':
        return 'bg-priority-medium'
      case 'high':
        return 'bg-priority-high'
      default:
        return 'bg-tertiary'
    }
  }
  return 'bg-tertiary'
}

onMounted(() => {
  // Use mousedown instead of click for faster response
  document.addEventListener('mousedown', handleClickOutside)
  // Capture phase to catch scroll events from any scrollable container
  window.addEventListener('scroll', handleScroll, true)
  window.addEventListener('resize', handleResize)
})

onUnmounted(() => {
  document.removeEventListener('mousedown', handleClickOutside)
  window.removeEventListener('scroll', handleScroll, true)
  window.removeEventListener('resize', handleResize)

  if (rafId) {
    cancelAnimationFrame(rafId)
  }
})
</script>

<template>
  <div class="relative" ref="triggerRef">
    <!-- Dropdown trigger -->
    <button
      type="button"
      @click="toggleDropdown"
      class="w-full px-3 py-2 bg-transparent text-primary text-left flex items-center justify-between hover:bg-surface-hover transition-colors rounded-lg"
    >
      <div class="flex items-center gap-2">
        <StatusIndicator v-if="type === 'status'" :status="value as 'open' | 'in-progress' | 'closed'" size="sm" />
        <PriorityIndicator v-else-if="type === 'priority'" :priority="value as 'low' | 'medium' | 'high'" size="sm" />
        <div v-else class="w-2 h-2 rounded-full" :class="getStatusColor(value)"></div>
        <span class="text-sm">{{ selectedOption?.label || 'Select an option' }}</span>
      </div>
      <svg
        class="w-4 h-4 text-tertiary transition-transform duration-200"
        :class="{ 'rotate-180': isOpen }"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
      </svg>
    </button>

    <!-- Dropdown menu - Teleported to body for proper stacking -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-100 ease-out"
        enter-from-class="opacity-0 scale-95"
        enter-to-class="opacity-100 scale-100"
        leave-active-class="transition duration-75 ease-in"
        leave-from-class="opacity-100 scale-100"
        leave-to-class="opacity-0 scale-95"
      >
        <div
          v-if="isOpen"
          ref="menuRef"
          class="custom-dropdown-menu fixed bg-surface border border-default rounded-lg shadow-lg overflow-hidden"
          :style="{
            top: `${menuPosition.top}px`,
            left: `${menuPosition.left}px`,
            width: `${menuPosition.width}px`,
            zIndex: 50
          }"
        >
          <div class="py-1">
            <button
              v-for="option in options"
              :key="option.value"
              @click="selectOption(option)"
              class="w-full px-3 py-2 text-left text-primary hover:bg-surface-hover transition-colors flex items-center gap-2"
              :class="{ 'bg-accent/10': option.value === value }"
            >
              <StatusIndicator v-if="type === 'status'" :status="option.value as 'open' | 'in-progress' | 'closed'" size="sm" />
              <PriorityIndicator v-else-if="type === 'priority'" :priority="option.value as 'low' | 'medium' | 'high'" size="sm" />
              <div v-else class="w-2 h-2 rounded-full" :class="getStatusColor(option.value)"></div>
              <span class="text-sm" :class="{ 'font-medium': option.value === value }">{{ option.label }}</span>
            </button>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>
