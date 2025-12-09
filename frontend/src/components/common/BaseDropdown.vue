<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'

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
const dropdownRef = ref<HTMLElement | null>(null)
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

// Dropdown position computed with viewport awareness
const dropdownPosition = computed(() => {
  if (!dropdownRef.value || !isOpen.value) {
    return { top: '0px', left: '0px', width: '0px' }
  }

  const rect = dropdownRef.value.getBoundingClientRect()
  const viewportHeight = window.innerHeight
  const spaceBelow = viewportHeight - rect.bottom
  const menuHeight = Math.min(props.options.length * 44 + 8, 264) // Estimate menu height

  // Open upward if not enough space below
  const openUpward = spaceBelow < menuHeight && rect.top > menuHeight

  return {
    top: openUpward
      ? `${rect.top + window.scrollY - menuHeight - 4}px`
      : `${rect.bottom + window.scrollY + 4}px`,
    left: `${rect.left + window.scrollX}px`,
    width: `${Math.max(rect.width, 180)}px`
  }
})

const toggleDropdown = () => {
  if (props.disabled) return
  isOpen.value = !isOpen.value
  if (isOpen.value) {
    // Set highlight to current selection
    highlightedIndex.value = props.options.findIndex(o => o.value === props.modelValue)
  }
}

const selectOption = (option: DropdownOption) => {
  emit('update:modelValue', option.value)
  isOpen.value = false
}

// Keyboard navigation
const handleKeydown = (event: KeyboardEvent) => {
  if (!isOpen.value) {
    if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown') {
      event.preventDefault()
      isOpen.value = true
      highlightedIndex.value = props.options.findIndex(o => o.value === props.modelValue)
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
      isOpen.value = false
      break
  }
}

// Close dropdown when clicking outside
const handleClickOutside = (event: MouseEvent) => {
  const target = event.target as Node
  if (dropdownRef.value && !dropdownRef.value.contains(target) &&
      menuRef.value && !menuRef.value.contains(target)) {
    isOpen.value = false
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
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <div class="relative" ref="dropdownRef">
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
          : 'hover:border-strong focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 cursor-pointer',
        isOpen && !disabled ? 'border-blue-500 ring-1 ring-blue-500' : ''
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
          v-if="isOpen && dropdownRef"
          ref="menuRef"
          role="listbox"
          class="fixed bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
          :class="sizeClasses.menu"
          :style="{
            ...dropdownPosition,
            zIndex: 9999,
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
                  ? 'bg-blue-500/10 text-blue-500'
                  : highlightedIndex === index
                    ? 'bg-surface-hover'
                    : 'hover:bg-surface-hover'
              ]"
            >
              <!-- Check mark for selected item -->
              <svg
                v-if="option.value === modelValue"
                class="w-4 h-4 text-blue-500 flex-shrink-0"
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
