<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'

export interface DropdownOption {
  value: string
  label: string
  description?: string
  icon?: string
  /**
   * One or more Tailwind background-color classes rendered as small
   * leading dots before the label (in both the trigger and menu).
   * Use a single tone for options that map to one domain value
   * (e.g. Open → status-open), or multiple tones for meta options
   * that span several (e.g. Active → open + in-progress, All → all
   * three). Single tones render as one 8px dot; multiple tones
   * render as a chip-stack of smaller 4px dots in sequence.
   */
  tones?: string[]
}

const props = withDefaults(defineProps<{
  modelValue: string | string[]
  options: DropdownOption[]
  placeholder?: string
  disabled?: boolean
  size?: 'xs' | 'sm' | 'md' | 'lg'
  multiple?: boolean
}>(), {
  placeholder: 'Select an option',
  disabled: false,
  size: 'md',
  multiple: false
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | string[]): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuRef = ref<HTMLElement | null>(null)
const highlightedIndex = ref(-1)

// Normalize modelValue to array for consistent handling
const selectedValues = computed((): string[] => {
  if (props.multiple) {
    return Array.isArray(props.modelValue) ? props.modelValue : []
  }
  return props.modelValue ? [props.modelValue as string] : []
})

// Check if an option is selected
const isSelected = (value: string): boolean => {
  return selectedValues.value.includes(value)
}

// Get selected option (for single select mode)
const selectedOption = computed(() =>
  props.options.find(option => option.value === props.modelValue)
)

// Get display text for trigger button
const displayText = computed(() => {
  if (props.multiple) {
    const selected = selectedValues.value.filter(v => v !== 'all')
    if (selected.length === 0) return props.placeholder
    // If all are selected, show the "all" option label (e.g., "All Statuses")
    const allOption = props.options.find(o => o.value === 'all')
    const nonAllOptions = props.options.filter(o => o.value !== 'all')
    if (selected.length === nonAllOptions.length && allOption) {
      return allOption.label
    }
    if (selected.length === 1) {
      return props.options.find(o => o.value === selected[0])?.label || selected[0]
    }
    return `${selected.length} selected`
  }
  return selectedOption.value?.label || props.placeholder
})

// Check if anything is selected (for styling)
const hasSelection = computed(() => {
  if (props.multiple) {
    return selectedValues.value.filter(v => v !== 'all').length > 0
  }
  return !!selectedOption.value
})

// Size classes
const sizeClasses = computed(() => {
  switch (props.size) {
    case 'xs':
      return {
        button: 'px-1.5 py-0.5 text-sm',
        menu: 'text-sm',
        option: 'px-3 py-1.5'
      }
    case 'sm':
      return {
        button: 'px-3 py-1.5 text-sm',
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

// Update position based on trigger and menu elements. Clamps both
// axes to the viewport so the menu never spills off-screen:
//  - Vertical: flip to open upward when space below is tight.
//  - Horizontal: anchor to trigger's left edge by default; if the menu
//    would overflow the right edge, shift left so its right edge
//    aligns with the viewport (right-align the menu to the trigger).
//    As a last resort for ultra-narrow viewports, clamp to the left
//    margin so the left edge is always visible.
const VIEWPORT_MARGIN_PX = 8

const updatePosition = () => {
  if (!triggerRef.value || !menuRef.value) return

  const triggerRect = triggerRef.value.getBoundingClientRect()
  const menuHeight = menuRef.value.offsetHeight
  const viewportHeight = window.innerHeight
  const viewportWidth = window.innerWidth
  const spaceBelow = viewportHeight - triggerRect.bottom

  // Open upward if not enough space below
  const openUpward = spaceBelow < menuHeight && triggerRect.top > menuHeight

  // Min width of 240px gives enough room for 30–35ch of option text
  // before wrapping kicks in. Short triggers ("Priority", icon-only,
  // etc.) still get a readable menu; wide triggers still match.
  const menuWidth = Math.max(triggerRect.width, 240)

  // Horizontal clamp: prefer the trigger's left edge, fall back to a
  // right-aligned position, then to the viewport left margin.
  let left = triggerRect.left
  if (left + menuWidth > viewportWidth - VIEWPORT_MARGIN_PX) {
    left = Math.max(VIEWPORT_MARGIN_PX, viewportWidth - menuWidth - VIEWPORT_MARGIN_PX)
  }

  menuPosition.value = {
    top: openUpward ? triggerRect.top - menuHeight - 2 : triggerRect.bottom + 2,
    left,
    width: menuWidth,
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

// Get all non-"all" option values
const allOptionValues = computed(() =>
  props.options.filter(o => o.value !== 'all').map(o => o.value)
)

// Check if all options are selected
const allSelected = computed(() => {
  if (!props.multiple) return false
  return allOptionValues.value.every(v => selectedValues.value.includes(v))
})

const selectOption = (option: DropdownOption) => {
  if (props.multiple) {
    // Handle "all" option - toggles all selections
    if (option.value === 'all') {
      if (allSelected.value) {
        // Deselect all
        emit('update:modelValue', [])
      } else {
        // Select all
        emit('update:modelValue', [...allOptionValues.value])
      }
      return
    }

    // Toggle the selected value
    const currentValues = [...selectedValues.value].filter(v => v !== 'all')
    const index = currentValues.indexOf(option.value)

    if (index === -1) {
      currentValues.push(option.value)
    } else {
      currentValues.splice(index, 1)
    }

    emit('update:modelValue', currentValues)
    // Don't close dropdown in multi-select mode
  } else {
    emit('update:modelValue', option.value)
    closeDropdown()
  }
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
        class="truncate flex items-center gap-2 min-w-0"
        :class="hasSelection ? 'text-primary' : 'text-tertiary'"
      >
        <span
          v-if="selectedOption?.tones?.length"
          aria-hidden="true"
          class="flex items-center gap-0.5 flex-shrink-0"
        >
          <span
            v-for="(t, i) in selectedOption.tones"
            :key="i"
            :class="[
              t,
              'rounded-full',
              selectedOption.tones.length === 1 ? 'w-2 h-2' : 'w-1 h-1',
            ]"
          />
        </span>
        <span class="truncate">{{ displayText }}</span>
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
              :aria-selected="isSelected(option.value)"
              @click="selectOption(option)"
              @mouseenter="highlightedIndex = index"
              class="w-full text-left text-primary transition-colors flex items-center gap-3"
              :class="[
                sizeClasses.option,
                (option.value === 'all' ? allSelected : isSelected(option.value))
                  ? 'bg-accent/10 text-accent'
                  : highlightedIndex === index
                    ? 'bg-surface-hover'
                    : 'hover:bg-surface-hover'
              ]"
            >
              <!-- Checkbox for multi-select mode -->
              <template v-if="multiple">
                <div
                  class="w-4 h-4 border rounded flex-shrink-0 flex items-center justify-center transition-colors"
                  :class="(option.value === 'all' ? allSelected : isSelected(option.value))
                    ? 'bg-accent border-accent'
                    : 'border-default'"
                >
                  <svg
                    v-if="option.value === 'all' ? allSelected : isSelected(option.value)"
                    class="w-3 h-3 text-white"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" />
                  </svg>
                </div>
              </template>

              <!--
                Left gutter for single-select. A fixed-size box keeps
                every row's label column aligned regardless of what
                the gutter is showing: check when selected, a dot
                (single tone) or dot-cluster (multi-tone meta option)
                when the option carries tones, empty otherwise.
              -->
              <span v-else class="w-4 h-4 flex items-center justify-center flex-shrink-0">
                <svg
                  v-if="isSelected(option.value)"
                  class="w-4 h-4 text-accent"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                </svg>
                <span
                  v-else-if="option.tones?.length"
                  aria-hidden="true"
                  class="flex items-center gap-0.5"
                >
                  <span
                    v-for="(t, i) in option.tones"
                    :key="i"
                    :class="[
                      t,
                      'rounded-full',
                      option.tones.length === 1 ? 'w-2 h-2' : 'w-1 h-1',
                    ]"
                  />
                </span>
              </span>

              <div class="flex-1 min-w-0">
                <div :class="(option.value === 'all' ? allSelected : isSelected(option.value)) ? 'font-medium' : ''">
                  {{ option.label }}
                </div>
                <div v-if="option.description" class="text-xs text-tertiary mt-0.5 leading-snug">
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
