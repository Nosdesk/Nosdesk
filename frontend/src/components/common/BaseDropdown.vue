<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'

export interface DropdownOption {
  value: string
  label: string
  description?: string
  icon?: string
}

const props = withDefaults(defineProps<{
  modelValue: string
  options: DropdownOption[]
  placeholder?: string
  disabled?: boolean
  size?: 'sm' | 'md' | 'lg'
}>(), {
  placeholder: 'Select an option',
  disabled: false,
  size: 'md'
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuRef = ref<HTMLElement | null>(null)
const highlightedIndex = ref(-1)

const selectedOption = computed(() =>
  props.options.find(option => option.value === props.modelValue)
)

// Size classes
const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm':
      return {
        button: 'px-3 py-2 text-sm',
        menu: 'text-sm',
        option: 'px-3 py-2'
      }
    case 'lg':
      return {
        button: 'px-4 py-3.5 text-base',
        menu: 'text-base',
        option: 'px-4 py-3'
      }
    default:
      return {
        button: 'px-4 py-3 text-sm',
        menu: 'text-sm',
        option: 'px-4 py-2.5'
      }
  }
})

// Reactive position tracking for scroll updates
const menuPosition = ref({ top: 0, left: 0, width: 0 })

// Update position based on trigger and menu elements
const updatePosition = () => {
  if (!triggerRef.value || !menuRef.value) return

  const triggerRect = triggerRef.value.getBoundingClientRect()
  const menuHeight = menuRef.value.offsetHeight
  const viewportHeight = window.innerHeight
  const spaceBelow = viewportHeight - triggerRect.bottom

  // Open upward if not enough space below
  const openUpward = spaceBelow < menuHeight && triggerRect.top > menuHeight

  menuPosition.value = {
    top: openUpward ? triggerRect.top - menuHeight - 2 : triggerRect.bottom + 2,
    left: triggerRect.left,
    width: Math.max(triggerRect.width, 180)
  }
}

// Watch for menu ref to be set, then position
watch(menuRef, (menu) => {
  if (menu) {
    updatePosition()
  }
})

const openDropdown = () => {
  if (props.disabled) return
  isOpen.value = true
  highlightedIndex.value = props.options.findIndex(o => o.value === props.modelValue)
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

const selectOption = (option: DropdownOption) => {
  emit('update:modelValue', option.value)
  closeDropdown()
}

// Keyboard navigation
const handleKeydown = (event: KeyboardEvent) => {
  if (!isOpen.value) {
    if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown') {
      event.preventDefault()
      openDropdown()
    }
    return
  }

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      highlightedIndex.value = Math.min(highlightedIndex.value + 1, props.options.length - 1)
      break
    case 'ArrowUp':
      event.preventDefault()
      highlightedIndex.value = Math.max(highlightedIndex.value - 1, 0)
      break
    case 'Enter':
    case ' ':
      event.preventDefault()
      if (highlightedIndex.value >= 0) {
        selectOption(props.options[highlightedIndex.value])
      }
      break
    case 'Escape':
      event.preventDefault()
      closeDropdown()
      break
  }
}

// Close dropdown when clicking outside - use mousedown for better UX
const handleClickOutside = (event: MouseEvent) => {
  if (!isOpen.value) return

  const target = event.target as Node

  // Check if click is on trigger
  if (triggerRef.value?.contains(target)) return

  // Check if click is on menu
  const menu = document.querySelector('.base-dropdown-menu')
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

// Scroll highlighted item into view
watch(highlightedIndex, (index) => {
  if (index >= 0 && menuRef.value) {
    const items = menuRef.value.querySelectorAll('[role="option"]')
    items[index]?.scrollIntoView({ block: 'nearest' })
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

  if (rafId) {
    cancelAnimationFrame(rafId)
  }
})
</script>

<template>
  <div class="relative" ref="triggerRef">
    <!-- Dropdown trigger button -->
    <button
      type="button"
      @click="toggleDropdown"
      @keydown="handleKeydown"
      :disabled="disabled"
      :aria-expanded="isOpen"
      :aria-haspopup="true"
      class="w-full bg-surface-alt border border-default rounded-lg text-left flex items-center justify-between transition-all duration-200"
      :class="[
        sizeClasses.button,
        disabled
          ? 'opacity-50 cursor-not-allowed'
          : 'hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent cursor-pointer',
        isOpen && !disabled ? 'border-accent ring-1 ring-accent' : ''
      ]"
    >
      <span
        class="truncate"
        :class="selectedOption ? 'text-primary' : 'text-tertiary'"
      >
        {{ selectedOption?.label || placeholder }}
      </span>
      <svg
        class="w-4 h-4 text-tertiary flex-shrink-0 ml-2 transition-transform duration-200"
        :class="{ 'rotate-180': isOpen }"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
      </svg>
    </button>

    <!-- Dropdown menu -->
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
          role="listbox"
          class="base-dropdown-menu fixed bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
          :class="sizeClasses.menu"
          :style="{
            top: `${menuPosition.top}px`,
            left: `${menuPosition.left}px`,
            width: `${menuPosition.width}px`,
            zIndex: 50,
            maxHeight: '264px'
          }"
        >
          <div class="py-1 overflow-y-auto max-h-64">
            <button
              v-for="(option, index) in options"
              :key="option.value"
              role="option"
              :aria-selected="option.value === modelValue"
              @click="selectOption(option)"
              @mouseenter="highlightedIndex = index"
              class="w-full text-left text-primary transition-colors flex items-center gap-3"
              :class="[
                sizeClasses.option,
                option.value === modelValue
                  ? 'bg-accent/10 text-accent'
                  : highlightedIndex === index
                    ? 'bg-surface-hover'
                    : 'hover:bg-surface-hover'
              ]"
            >
              <!-- Check mark for selected item -->
              <svg
                v-if="option.value === modelValue"
                class="w-4 h-4 text-accent flex-shrink-0"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
              </svg>
              <div v-else class="w-4 flex-shrink-0" />

              <div class="flex-1 min-w-0">
                <div class="truncate" :class="option.value === modelValue ? 'font-medium' : ''">
                  {{ option.label }}
                </div>
                <div v-if="option.description" class="text-xs text-tertiary truncate mt-0.5">
                  {{ option.description }}
                </div>
              </div>
            </button>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>
