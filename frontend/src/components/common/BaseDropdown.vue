<script setup lang="ts">
/**
 * Select-style dropdown. Public API (props, emits, slots) is
 * unchanged from the standalone implementation; the entire
 * positioning, click-outside-dismiss, scroll-tracking and focus
 * machinery now delegates to `<Popover>`. What's left in this
 * file is the dropdown's actual job: trigger rendering, option
 * rendering, multi-select wiring, keyboard navigation.
 */
import { computed, nextTick, ref, useId, watch } from 'vue'
import ResponsiveMenu from './ResponsiveMenu.vue'
import Icon from './Icon.vue'

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

const props = withDefaults(
  defineProps<{
    modelValue: string | string[]
    options: DropdownOption[]
    placeholder?: string
    disabled?: boolean
    size?: 'xs' | 'sm' | 'md' | 'lg'
    multiple?: boolean
    /** Optional label rendered above the trigger in the same
     * uppercase-tertiary shell as FormInput / FormNumber, so a
     * dropdown sitting alongside text inputs in a form looks
     * coherent without the consumer wrapping it in a hand-rolled
     * `<label>`. */
    label?: string
    /** Helper text shown below the trigger. */
    description?: string
    /** Error text shown below the trigger; flags the trigger as
     * invalid via aria-invalid + a red border. */
    error?: string
    /** Marks the field required for the label asterisk; the
     * dropdown itself doesn't enforce a non-empty selection. */
    required?: boolean
  }>(),
  {
    placeholder: 'Select an option',
    disabled: false,
    size: 'md',
    multiple: false,
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | string[]): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuContentRef = ref<HTMLElement | null>(null)
const highlightedIndex = ref(-1)
const generatedId = useId()
const triggerId = computed(() => `dropdown-${generatedId}`)
const describedById = computed(() =>
  props.error || props.description ? `${triggerId.value}-desc` : undefined,
)

// Anchor descriptor passed to <Popover>. The function form keeps
// the lookup live so the popover repositions correctly even if
// the trigger element re-mounts (e.g. v-if elsewhere in the
// parent tree).
const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

// Min width 240px gives 30-35ch of option text before wrapping
// kicks in. Short triggers ("Priority", icon-only, etc.) still
// get a readable menu; wide triggers still match.
const MENU_MIN_WIDTH = 240

// ---- Selection state -------------------------------------------------

const selectedValues = computed((): string[] => {
  if (props.multiple) {
    return Array.isArray(props.modelValue) ? props.modelValue : []
  }
  return props.modelValue ? [props.modelValue as string] : []
})

const isSelected = (value: string): boolean => selectedValues.value.includes(value)

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
)

const displayText = computed(() => {
  if (props.multiple) {
    const selected = selectedValues.value.filter((v) => v !== 'all')
    if (selected.length === 0) return props.placeholder
    const allOption = props.options.find((o) => o.value === 'all')
    const nonAllOptions = props.options.filter((o) => o.value !== 'all')
    if (selected.length === nonAllOptions.length && allOption) {
      return allOption.label
    }
    if (selected.length === 1) {
      return props.options.find((o) => o.value === selected[0])?.label || selected[0]
    }
    return `${selected.length} selected`
  }
  return selectedOption.value?.label || props.placeholder
})

const hasSelection = computed(() => {
  if (props.multiple) {
    return selectedValues.value.filter((v) => v !== 'all').length > 0
  }
  return !!selectedOption.value
})

// ---- "All" meta-option handling for multi-select ---------------------

const allOptionValues = computed(() =>
  props.options.filter((o) => o.value !== 'all').map((o) => o.value),
)

const allSelected = computed(() => {
  if (!props.multiple) return false
  return allOptionValues.value.every((v) => selectedValues.value.includes(v))
})

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

const openDropdown = () => {
  if (props.disabled) return
  isOpen.value = true
  highlightedIndex.value = props.options.findIndex((o) => o.value === props.modelValue)
}

const closeDropdown = () => {
  isOpen.value = false
}

const toggleDropdown = () => {
  if (isOpen.value) closeDropdown()
  else openDropdown()
}

const selectOption = (option: DropdownOption) => {
  if (props.multiple) {
    if (option.value === 'all') {
      if (allSelected.value) {
        emit('update:modelValue', [])
      } else {
        emit('update:modelValue', [...allOptionValues.value])
      }
      return
    }
    const currentValues = [...selectedValues.value].filter((v) => v !== 'all')
    const index = currentValues.indexOf(option.value)
    if (index === -1) currentValues.push(option.value)
    else currentValues.splice(index, 1)
    emit('update:modelValue', currentValues)
    // Stay open in multi-select so the user can pick more.
  } else {
    emit('update:modelValue', option.value)
    closeDropdown()
  }
}

// ---- Keyboard navigation -------------------------------------------
//
// Trigger keydown: Enter / Space / ArrowDown opens the menu and
// focuses the first option. Once open, the popover root has focus
// and we route arrow / Enter / Escape through the same handler.

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
      if (highlightedIndex.value >= 0) selectOption(props.options[highlightedIndex.value])
      break
    case 'Escape':
      event.preventDefault()
      closeDropdown()
      break
  }
}

// Keep the highlighted item visible inside the menu's overflow
// container as the user arrow-keys down a long list.
watch(highlightedIndex, async (index) => {
  if (index < 0) return
  await nextTick()
  const items = menuContentRef.value?.querySelectorAll('[role="option"]')
  items?.[index]?.scrollIntoView({ block: 'nearest' })
})
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <label
      v-if="label"
      :for="triggerId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label
      }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <div class="relative" ref="triggerRef">
      <button
        :id="triggerId"
        type="button"
        @click="toggleDropdown"
        @keydown="handleKeydown"
        :disabled="disabled"
        :aria-expanded="isOpen"
        :aria-haspopup="true"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        class="w-full bg-surface-alt border rounded-lg text-left flex items-center justify-between transition-all duration-200"
        :class="[
          sizeClasses.button,
          error ? 'border-status-error' : 'border-subtle',
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
      <span
        class="text-tertiary flex-shrink-0 ml-2 transition-transform duration-200 inline-flex"
        :class="{ 'rotate-180': isOpen }"
      >
        <Icon name="chevronDown" />
      </span>
    </button>

    <!--
      ResponsiveMenu picks the layout from viewport width: at md+
      it renders as the previous Popover (anchored, fade-scale,
      click-outside dismiss). On phone it renders as a bottom
      sheet — the touch-native pattern. Same slot content; no
      consumer of BaseDropdown needs to opt in. The trigger
      button label is mirrored into the sheet's title so the
      user knows what they're choosing on mobile.
    -->
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
      <div
        ref="menuContentRef"
        class="py-1 overflow-y-auto max-h-64"
        :class="sizeClasses.menu"
        @keydown="handleKeydown"
      >
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
                : 'hover:bg-surface-hover',
          ]"
        >
          <template v-if="multiple">
            <div
              class="w-4 h-4 border rounded flex-shrink-0 flex items-center justify-center transition-colors"
              :class="
                (option.value === 'all' ? allSelected : isSelected(option.value))
                  ? 'bg-accent border-accent'
                  : 'border-default'
              "
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
            <span v-else-if="option.tones?.length" aria-hidden="true" class="flex items-center gap-0.5">
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
    </ResponsiveMenu>
    </div>
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>
