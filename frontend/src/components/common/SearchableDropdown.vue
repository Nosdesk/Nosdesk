<script setup lang="ts">
/**
 * Combobox-style dropdown for long static lists.
 *
 * Mirrors `BaseDropdown`'s prop shape, but renders a search input
 * at the top of the menu and filters `options` against it as the
 * user types. Use this when `options.length > ~15` or when the
 * user can't reasonably pick by scrolling alone — timezone
 * picker, channel picker, big enum selectors, etc.
 *
 * Why a separate component rather than a `searchable: boolean`
 * on `BaseDropdown`: the two have different interaction models
 * (listbox vs combobox in WAI-ARIA terms). Boolean flags that
 * branch interaction patterns make components harder to reason
 * about and accumulate behavioural edge cases over time.
 *
 * Single-select only. If multi-select with search becomes a need,
 * it goes in `MultiSelectCombobox.vue` — same logic, different
 * selection wiring — rather than back into here.
 */
import { computed, nextTick, ref, watch } from 'vue'
import ResponsiveMenu from './ResponsiveMenu.vue'
import Icon from './Icon.vue'
import type { DropdownOption } from './BaseDropdown.vue'

export type { DropdownOption }

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: DropdownOption[]
    placeholder?: string
    searchPlaceholder?: string
    emptyMessage?: string
    disabled?: boolean
    size?: 'xs' | 'sm' | 'md' | 'lg'
  }>(),
  {
    placeholder: 'Select an option',
    searchPlaceholder: 'Search...',
    emptyMessage: 'No matches',
    disabled: false,
    size: 'md',
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuContentRef = ref<HTMLElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const highlightedIndex = ref(-1)
const query = ref('')

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

const MENU_MIN_WIDTH = 280

// ---- Filtered options -----------------------------------------------
//
// Case-insensitive substring match against `label`, `description`,
// and `value` so the user can find a timezone by IANA name
// ("Sydney"), by display string ("Australia"), or by free-text
// metadata ("UTC+10"). Empty query returns the full list.

const filteredOptions = computed<DropdownOption[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter((option) => {
    const haystack = [option.label, option.description ?? '', option.value]
      .join(' ')
      .toLowerCase()
    return haystack.includes(q)
  })
})

// ---- Selection state ------------------------------------------------

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
)

const displayText = computed(
  () => selectedOption.value?.label || props.placeholder,
)

const hasSelection = computed(() => !!selectedOption.value)

const isSelected = (value: string) => value === props.modelValue

// ---- Sizing ---------------------------------------------------------

const sizeClasses = computed(() => {
  switch (props.size) {
    case 'xs':
      return { button: 'px-1.5 py-0.5 text-sm', menu: 'text-sm', option: 'px-3 py-1.5' }
    case 'sm':
      return { button: 'px-3 py-1.5 text-sm', menu: 'text-sm', option: 'px-3 py-2' }
    case 'lg':
      return { button: 'px-4 py-3.5 text-base', menu: 'text-base', option: 'px-4 py-3' }
    default:
      return { button: 'px-4 py-3 text-sm', menu: 'text-sm', option: 'px-4 py-2.5' }
  }
})

// ---- Open/close + selection -----------------------------------------

const openDropdown = async () => {
  if (props.disabled) return
  isOpen.value = true
  query.value = ''
  highlightedIndex.value = props.options.findIndex(
    (o) => o.value === props.modelValue,
  )
  // Hand focus to the search input on next tick so the trigger
  // button's blur doesn't immediately close the popover.
  await nextTick()
  searchInputRef.value?.focus()
}

const closeDropdown = () => {
  isOpen.value = false
  query.value = ''
}

const toggleDropdown = () => {
  if (isOpen.value) closeDropdown()
  else openDropdown()
}

const selectOption = (option: DropdownOption) => {
  emit('update:modelValue', option.value)
  closeDropdown()
}

// Reset highlight to the top of the filtered list whenever the
// query changes — what was at index 3 before may now be at
// index 0 or filtered out entirely.
watch(query, () => {
  highlightedIndex.value = filteredOptions.value.length > 0 ? 0 : -1
})

// ---- Keyboard navigation -------------------------------------------

const handleTriggerKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown') {
    event.preventDefault()
    openDropdown()
  }
}

const handleSearchKeydown = (event: KeyboardEvent) => {
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      highlightedIndex.value = Math.min(
        highlightedIndex.value + 1,
        filteredOptions.value.length - 1,
      )
      break
    case 'ArrowUp':
      event.preventDefault()
      highlightedIndex.value = Math.max(highlightedIndex.value - 1, 0)
      break
    case 'Enter':
      event.preventDefault()
      if (
        highlightedIndex.value >= 0 &&
        highlightedIndex.value < filteredOptions.value.length
      ) {
        selectOption(filteredOptions.value[highlightedIndex.value])
      }
      break
    case 'Escape':
      event.preventDefault()
      closeDropdown()
      break
  }
}

// Keep the highlighted item visible inside the menu's overflow
// container as the user arrow-keys through a long filtered list.
watch(highlightedIndex, async (index) => {
  if (index < 0) return
  await nextTick()
  const items = menuContentRef.value?.querySelectorAll('[role="option"]')
  items?.[index]?.scrollIntoView({ block: 'nearest' })
})
</script>

<template>
  <div class="relative" ref="triggerRef">
    <button
      type="button"
      @click="toggleDropdown"
      @keydown="handleTriggerKeydown"
      :disabled="disabled"
      :aria-expanded="isOpen"
      :aria-haspopup="true"
      class="w-full bg-surface-alt border border-default rounded-lg text-left flex items-center justify-between transition-all duration-200"
      :class="[
        sizeClasses.button,
        disabled
          ? 'opacity-50 cursor-not-allowed'
          : 'hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent cursor-pointer',
        isOpen && !disabled ? 'border-accent ring-1 ring-accent' : '',
      ]"
    >
      <span
        class="truncate flex items-center gap-2 min-w-0"
        :class="hasSelection ? 'text-primary' : 'text-tertiary'"
      >
        <span class="truncate">{{ displayText }}</span>
      </span>
      <span
        class="text-tertiary flex-shrink-0 ml-2 transition-transform duration-200 inline-flex"
        :class="{ 'rotate-180': isOpen }"
      >
        <Icon name="chevronDown" />
      </span>
    </button>

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      :title="placeholder"
      placement="bottom-start"
      react-to-scroll="reposition"
      match-anchor-width
      :min-width="MENU_MIN_WIDTH"
      :offset="2"
      role="listbox"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
      @close="closeDropdown"
    >
      <!-- Search input pinned to the top of the menu. Sticky so
           it stays visible while the user scrolls a long
           filtered list. -->
      <div class="sticky top-0 z-10 bg-surface border-b border-default p-2">
        <div class="relative">
          <span
            class="absolute left-2 top-1/2 -translate-y-1/2 text-tertiary inline-flex pointer-events-none"
          >
            <Icon name="search" />
          </span>
          <input
            ref="searchInputRef"
            v-model="query"
            type="text"
            :placeholder="searchPlaceholder"
            class="w-full pl-7 pr-2 py-1.5 bg-surface-alt text-primary rounded border border-default focus:ring-1 focus:ring-accent focus:outline-none text-sm"
            @keydown="handleSearchKeydown"
          />
        </div>
      </div>

      <div
        ref="menuContentRef"
        class="py-1 overflow-y-auto max-h-64"
        :class="sizeClasses.menu"
      >
        <button
          v-for="(option, index) in filteredOptions"
          :key="option.value"
          role="option"
          :aria-selected="isSelected(option.value)"
          @click="selectOption(option)"
          @mouseenter="highlightedIndex = index"
          class="w-full text-left text-primary transition-colors flex items-center gap-3"
          :class="[
            sizeClasses.option,
            isSelected(option.value)
              ? 'bg-accent/10 text-accent'
              : highlightedIndex === index
                ? 'bg-surface-hover'
                : 'hover:bg-surface-hover',
          ]"
        >
          <span class="w-4 h-4 flex items-center justify-center flex-shrink-0">
            <svg
              v-if="isSelected(option.value)"
              class="w-4 h-4 text-accent"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M5 13l4 4L19 7"
              />
            </svg>
          </span>
          <div class="flex-1 min-w-0">
            <div :class="isSelected(option.value) ? 'font-medium' : ''">
              {{ option.label }}
            </div>
            <div
              v-if="option.description"
              class="text-xs text-tertiary mt-0.5 leading-snug"
            >
              {{ option.description }}
            </div>
          </div>
        </button>

        <div
          v-if="filteredOptions.length === 0"
          class="px-4 py-6 text-center text-sm text-tertiary"
        >
          {{ emptyMessage }}
        </div>
      </div>
    </ResponsiveMenu>
  </div>
</template>
