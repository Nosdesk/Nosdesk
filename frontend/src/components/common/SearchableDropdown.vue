<script setup lang="ts">
/**
 * Pick-only dropdown with a filter, for long static lists (timezones,
 * big enum selectors). The trigger is a button showing the selection;
 * the surface is a filter input over a listbox, on Reka's Listbox:
 * `ListboxFilter` is the input (it owns `aria-activedescendant`, the
 * arrow keys, Home/End and Enter against the highlighted row), the rows
 * are `ListboxItem`s with roving highlight. Filtering itself is ours,
 * through Reka's locale-aware `useFilter` (accent and case insensitive)
 * across label, description and value.
 *
 * Same responsive shape as BaseDropdown: an anchored popover at `md`
 * and above, the bottom sheet on phones. Single-select only; multi with
 * search would be its own component rather than a flag here.
 */
import { computed, ref, useAttrs, useId, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { ListboxContent, ListboxFilter, ListboxItem, ListboxRoot, useFilter } from 'reka-ui'
import ResponsiveMenu from './ResponsiveMenu.vue'
import Icon from './Icon.vue'
import DropdownOptionRow from './dropdown/DropdownOptionRow.vue'
import type { DropdownOption } from './dropdownOption'

export type { DropdownOption }

defineOptions({ inheritAttrs: false })

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: DropdownOption[]
    placeholder?: string
    searchPlaceholder?: string
    emptyMessage?: string
    disabled?: boolean
    size?: 'xs' | 'sm' | 'md' | 'lg'
    /** Optional label rendered above the trigger in the same shell
     * as FormInput / FormNumber / BaseDropdown. */
    label?: string
    description?: string
    error?: string
    required?: boolean
  }>(),
  {
    placeholder: undefined,
    searchPlaceholder: undefined,
    emptyMessage: undefined,
    disabled: false,
    size: 'md',
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const attrs = useAttrs()
const rootAttrs = computed(() => ({ class: attrs.class, style: attrs.style }))
const triggerAttrs = computed(() => {
  const { class: _c, style: _s, ...rest } = attrs
  return rest
})

const fluent = useFluent()
const generatedId = useId()
const triggerId = computed(() => `searchable-dropdown-${generatedId}`)
const describedById = computed(() =>
  props.error || props.description ? `${triggerId.value}-desc` : undefined,
)
const resolvedPlaceholder = computed(() => props.placeholder ?? fluent.$t('common-dropdown-select-placeholder'))
const resolvedSearchPlaceholder = computed(() => props.searchPlaceholder ?? fluent.$t('common-search-placeholder'))
const resolvedEmptyMessage = computed(() => props.emptyMessage ?? fluent.$t('common-dropdown-empty-message'))

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const query = ref('')

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

const MENU_MIN_WIDTH = 280

// Locale-aware substring match on label, description and value, so a
// timezone is found by IANA name ("Sydney"), display string
// ("Australia") or metadata ("UTC+10"). Empty query returns the list.
const { contains } = useFilter({ sensitivity: 'base' })
const filteredOptions = computed<DropdownOption[]>(() => {
  const q = query.value.trim()
  if (!q) return props.options
  return props.options.filter((option) =>
    contains([option.label, option.description ?? '', option.value].join(' '), q),
  )
})

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
)
const displayText = computed(() => selectedOption.value?.label || resolvedPlaceholder.value)
const hasSelection = computed(() => !!selectedOption.value)

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

function openDropdown() {
  if (props.disabled) return
  query.value = ''
  isOpen.value = true
}

function closeDropdown() {
  isOpen.value = false
  query.value = ''
}

function onSelect(value: unknown) {
  if (typeof value !== 'string') return
  emit('update:modelValue', value)
  closeDropdown()
}

function onTriggerKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown') {
    event.preventDefault()
    openDropdown()
  }
}

// Reset the filter whenever the surface closes from outside (Escape,
// backdrop, breakpoint change), not only through closeDropdown.
watch(isOpen, (open) => {
  if (!open) query.value = ''
})

const optionClasses = (option: DropdownOption) => [
  'w-full text-left text-primary transition-colors flex items-center gap-3 min-h-[44px] md:min-h-0 outline-none',
  sizeClasses.value.option,
  option.value === props.modelValue
    ? 'bg-accent/10 text-accent'
    : 'hover:bg-surface-hover data-[highlighted]:bg-surface-hover',
]
</script>

<template>
  <div class="flex flex-col gap-1.5" v-bind="rootAttrs">
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
        v-bind="triggerAttrs"
        @click="isOpen ? closeDropdown() : openDropdown()"
        @keydown="onTriggerKeydown"
        :disabled="disabled"
        :aria-expanded="isOpen"
        aria-haspopup="dialog"
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
          <span class="truncate">{{ displayText }}</span>
        </span>
        <span
          class="text-tertiary flex-shrink-0 ml-2 transition-transform duration-200 inline-flex"
          :class="{ 'rotate-180': isOpen }"
        >
          <Icon name="chevronDown" />
        </span>
      </button>

      <!-- A dialog surface (it holds an input), not a menu or listbox;
           the listbox is the ListboxContent inside it. -->
      <ResponsiveMenu
        :open="isOpen"
        :anchor="anchor"
        :title="label ?? resolvedPlaceholder"
        placement="bottom-start"
        react-to-scroll="reposition"
        match-anchor-width
        :min-width="MENU_MIN_WIDTH"
        :offset="2"
        role="dialog"
        :auto-focus="false"
        popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
        @close="closeDropdown"
      >
        <ListboxRoot
          :model-value="modelValue"
          selection-behavior="replace"
          highlight-on-hover
          class="flex flex-col"
          @update:model-value="onSelect"
        >
          <!-- Filter pinned to the top; the input owns the keyboard
               (arrows move the highlight, Enter picks it). -->
          <div class="sticky top-0 z-10 bg-surface border-b border-default p-2">
            <div class="relative">
              <span
                class="absolute left-2 top-1/2 -translate-y-1/2 text-tertiary inline-flex pointer-events-none"
              >
                <Icon name="search" />
              </span>
              <ListboxFilter
                v-model="query"
                auto-focus
                :placeholder="resolvedSearchPlaceholder"
                :aria-label="resolvedSearchPlaceholder"
                class="w-full pl-7 pr-2 py-1.5 bg-surface-alt text-primary rounded border border-default focus:ring-1 focus:ring-accent focus:outline-none text-sm"
              />
            </div>
          </div>

          <!-- Named, and a tab stop in its own right: the filter drives it
               through aria-activedescendant, but a scrollable list must
               also be reachable directly (axe scrollable-region-focusable). -->
          <ListboxContent as-child>
            <div
              class="py-1 overflow-y-auto max-h-64 outline-none"
              :class="sizeClasses.menu"
              tabindex="0"
              :aria-label="label ?? resolvedPlaceholder"
            >
            <ListboxItem
              v-for="option in filteredOptions"
              :key="option.value"
              as-child
              :value="option.value"
              :disabled="option.disabled"
            >
              <button type="button" :class="optionClasses(option)">
                <DropdownOptionRow
                  :option="option"
                  :multiple="false"
                  :checked="option.value === modelValue"
                />
              </button>
            </ListboxItem>
            <div
              v-if="filteredOptions.length === 0"
              class="px-4 py-6 text-center text-sm text-tertiary"
            >
              {{ resolvedEmptyMessage }}
            </div>
            </div>
          </ListboxContent>
        </ListboxRoot>
      </ResponsiveMenu>
    </div>
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>
