<!-- components/CustomDropdown.vue -->
<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'

const props = defineProps<{
  value: string
  options: { value: string; label: string }[]
  type: 'status' | 'priority'
}>()

const emit = defineEmits<{
  (e: 'update:value', value: string): void
}>()

const { isMobile } = useMobileDetection('md')

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuRef = ref<HTMLElement | null>(null)

// Reactive position tracking for desktop dropdown
const menuPosition = ref({ top: 0, left: 0, width: 0, openUpward: false })

// Scroll position for iOS scroll lock
let scrollPosition = 0

// Lock scroll for iOS - prevents background scrolling when modal is open
const lockScroll = () => {
  scrollPosition = window.pageYOffset
  document.body.style.overflow = 'hidden'
  document.body.style.position = 'fixed'
  document.body.style.top = `-${scrollPosition}px`
  document.body.style.left = '0'
  document.body.style.right = '0'
}

// Unlock scroll for iOS - restores scroll position
const unlockScroll = () => {
  document.body.style.overflow = ''
  document.body.style.position = ''
  document.body.style.top = ''
  document.body.style.left = ''
  document.body.style.right = ''
  window.scrollTo(0, scrollPosition)
}

const selectedOption = computed(() =>
  props.options.find(option => option.value === props.value)
)

// Update position for desktop dropdown
const updatePosition = () => {
  if (!triggerRef.value || isMobile.value) return

  const rect = triggerRef.value.getBoundingClientRect()
  const viewportHeight = window.innerHeight
  const viewportWidth = window.innerWidth
  const spaceBelow = viewportHeight - rect.bottom
  const menuHeight = Math.min(props.options.length * 48 + 8, 240)

  // Open upward if not enough space below
  const openUpward = spaceBelow < menuHeight && rect.top > menuHeight

  // Calculate width - minimum 160px for readability
  const dropdownWidth = Math.max(rect.width, 160)

  // Ensure dropdown doesn't overflow viewport horizontally
  let left = rect.left
  if (left + dropdownWidth > viewportWidth - 16) {
    left = viewportWidth - dropdownWidth - 16
  }
  if (left < 16) left = 16

  menuPosition.value = {
    top: openUpward ? rect.top - menuHeight : rect.bottom + 4,
    left,
    width: dropdownWidth,
    openUpward,
  }
}

const openDropdown = async () => {
  isOpen.value = true

  // Lock body scroll on mobile using iOS-safe method
  if (isMobile.value) {
    lockScroll()
  }

  await nextTick()
  updatePosition()
}

const closeDropdown = () => {
  isOpen.value = false

  // Unlock body scroll using iOS-safe method
  if (isMobile.value) {
    unlockScroll()
  }
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

// Close dropdown when clicking outside
const handleClickOutside = (event: MouseEvent) => {
  if (!isOpen.value) return

  const target = event.target as Node
  if (triggerRef.value?.contains(target)) return
  if (menuRef.value?.contains(target)) return

  // Check teleported menu
  const menu = document.querySelector('.custom-dropdown-menu')
  if (menu?.contains(target)) return

  closeDropdown()
}

// Update position on scroll (desktop only)
let rafId: number | null = null
const handleScroll = () => {
  if (!isOpen.value || isMobile.value) return

  if (rafId) cancelAnimationFrame(rafId)
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

// Computed dropdown styles for desktop
const dropdownStyles = computed(() => {
  if (isMobile.value) return {}

  return {
    position: 'fixed' as const,
    top: `${menuPosition.value.top}px`,
    left: `${menuPosition.value.left}px`,
    width: `${menuPosition.value.width}px`,
    zIndex: 9999,
  }
})

onMounted(() => {
  document.addEventListener('mousedown', handleClickOutside)
  window.addEventListener('scroll', handleScroll, true)
  window.addEventListener('resize', handleResize)
})

onUnmounted(() => {
  document.removeEventListener('mousedown', handleClickOutside)
  window.removeEventListener('scroll', handleScroll, true)
  window.removeEventListener('resize', handleResize)
  if (rafId) cancelAnimationFrame(rafId)
  // Ensure scroll is unlocked
  if (isMobile.value && isOpen.value) {
    unlockScroll()
  }
})
</script>

<template>
  <div class="relative" ref="triggerRef">
    <!-- Dropdown trigger -->
    <button
      type="button"
      @click="toggleDropdown"
      class="w-full px-3 py-2.5 sm:py-2 min-h-[44px] sm:min-h-[40px] bg-transparent text-primary text-left flex items-center justify-between hover:bg-surface-hover active:bg-surface-alt transition-colors rounded-lg"
    >
      <div class="flex items-center gap-2.5 sm:gap-2">
        <StatusIndicator v-if="type === 'status'" :status="value as 'open' | 'in-progress' | 'closed'" size="sm" />
        <PriorityIndicator v-else-if="type === 'priority'" :priority="value as 'low' | 'medium' | 'high'" size="sm" />
        <span class="text-sm font-medium">{{ selectedOption?.label || 'Select...' }}</span>
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

    <!-- Desktop Dropdown -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-150 ease-out"
        enter-from-class="opacity-0 scale-95"
        enter-to-class="opacity-100 scale-100"
        leave-active-class="transition duration-100 ease-in"
        leave-from-class="opacity-100 scale-100"
        leave-to-class="opacity-0 scale-95"
      >
        <div
          v-if="isOpen && !isMobile"
          ref="menuRef"
          class="custom-dropdown-menu bg-surface border border-default rounded-xl shadow-2xl overflow-hidden"
          :style="dropdownStyles"
        >
          <div class="py-1">
            <button
              v-for="option in options"
              :key="option.value"
              @click="selectOption(option)"
              class="w-full px-3 py-2.5 text-left text-primary hover:bg-surface-hover transition-colors flex items-center gap-2.5"
              :class="{ 'bg-accent/10': option.value === value }"
            >
              <StatusIndicator v-if="type === 'status'" :status="option.value as 'open' | 'in-progress' | 'closed'" size="sm" />
              <PriorityIndicator v-else-if="type === 'priority'" :priority="option.value as 'low' | 'medium' | 'high'" size="sm" />
              <span class="text-sm" :class="{ 'font-medium': option.value === value }">{{ option.label }}</span>
              <svg
                v-if="option.value === value"
                class="w-4 h-4 text-accent ml-auto flex-shrink-0"
                fill="currentColor"
                viewBox="0 0 20 20"
              >
                <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Mobile Bottom Sheet -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-300 ease-out"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition duration-200 ease-in"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div
          v-if="isOpen && isMobile"
          class="fixed inset-0 bg-black/50 z-[9998]"
          style="touch-action: none;"
          @click="closeDropdown"
          @touchmove.prevent
        />
      </Transition>

      <Transition
        enter-active-class="transition duration-300 ease-out"
        enter-from-class="translate-y-full"
        enter-to-class="translate-y-0"
        leave-active-class="transition duration-200 ease-in"
        leave-from-class="translate-y-0"
        leave-to-class="translate-y-full"
      >
        <div
          v-if="isOpen && isMobile"
          ref="menuRef"
          class="custom-dropdown-menu fixed inset-x-0 bottom-0 bg-surface rounded-t-2xl shadow-2xl z-[9999] max-h-[85dvh]"
        >
          <!-- Handle bar -->
          <div class="flex justify-center py-3">
            <div class="w-10 h-1 bg-border-default rounded-full" />
          </div>

          <!-- Header -->
          <div class="px-4 pb-3 border-b border-default">
            <h3 class="text-base font-semibold text-primary">
              Select {{ type === 'status' ? 'Status' : 'Priority' }}
            </h3>
          </div>

          <!-- Options -->
          <div class="py-2">
            <button
              v-for="option in options"
              :key="option.value"
              @click="selectOption(option)"
              class="w-full px-4 py-4 text-left flex items-center gap-4 active:bg-surface-alt transition-colors border-b border-subtle last:border-b-0"
              :class="{ 'bg-accent/5': option.value === value }"
            >
              <StatusIndicator v-if="type === 'status'" :status="option.value as 'open' | 'in-progress' | 'closed'" size="md" />
              <PriorityIndicator v-else-if="type === 'priority'" :priority="option.value as 'low' | 'medium' | 'high'" size="md" />
              <span class="text-base flex-1" :class="{ 'font-medium': option.value === value }">{{ option.label }}</span>
              <svg
                v-if="option.value === value"
                class="w-5 h-5 text-accent flex-shrink-0"
                fill="currentColor"
                viewBox="0 0 20 20"
              >
                <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>

          <!-- Cancel button -->
          <div class="p-4 border-t border-default bg-surface safe-area-inset-bottom">
            <button
              @click="closeDropdown"
              class="w-full py-3.5 px-4 text-base font-medium text-primary bg-surface-alt rounded-xl active:opacity-80 transition-opacity"
            >
              Cancel
            </button>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
/* Safe area for iOS devices */
.safe-area-inset-bottom {
  padding-bottom: max(1rem, env(safe-area-inset-bottom));
}
</style>
