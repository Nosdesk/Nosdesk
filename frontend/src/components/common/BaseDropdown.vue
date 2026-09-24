<script setup lang="ts" generic="T extends string | number = string">
/**
 * Select-style dropdown. Public API (props, emits) is unchanged; the
 * implementation is Reka UI's Select at `md` and above, and the app's
 * bottom sheet on phones.
 *
 * Reka owns the select semantics: `role=combobox` trigger with
 * `aria-expanded` and `aria-controls`, `role=listbox` content with
 * `role=option` rows, roving focus with Home/End/PageUp/PageDown,
 * typeahead in the open list and on the closed trigger (type to select,
 * like a native `<select>`), Enter/Space select, a hidden native select
 * for forms (`name`), and the dismiss layer. Positioning is floating-ui
 * (`position="popper"`), matching the trigger width with a readable
 * floor.
 *
 * Values are `string` by default; number-valued lists pass `T = number`
 * (the Select compares with `===`, no adapter needed). Multi-select
 * keeps the list open and understands the `all` meta option: selecting it
 * selects every real option, clearing it clears them, and it reads as
 * checked when all are.
 *
 * `class` goes to the wrapper; `aria-label`, `aria-labelledby` and
 * `title` go to the trigger.
 */
import { computed, ref, useAttrs, useId } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  SelectContent,
  SelectItem,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectViewport,
} from 'reka-ui'
import BottomSheet from './BottomSheet.vue'
import Icon from './Icon.vue'
import DropdownValue from './dropdown/DropdownValue.vue'
import DropdownOptionRow from './dropdown/DropdownOptionRow.vue'
import { useResponsiveSheet } from '@/composables/useResponsiveSheet'
import type { DropdownOption } from './dropdownOption'

export type { DropdownOption }

defineOptions({ inheritAttrs: false })

const props = withDefaults(
  defineProps<{
    modelValue: T | T[]
    options: DropdownOption<T>[]
    placeholder?: string
    disabled?: boolean
    size?: 'xs' | 'sm' | 'md' | 'lg'
    multiple?: boolean
    /** Optional label rendered above the trigger in the same
     * uppercase-tertiary shell as FormInput / FormNumber. */
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
    placeholder: undefined,
    disabled: false,
    size: 'md',
    multiple: false,
  },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: T | T[]): void
}>()

const fluent = useFluent()
const resolvedPlaceholder = computed(() => props.placeholder ?? fluent.$t('common-dropdown-select-placeholder'))

const attrs = useAttrs()
const rootAttrs = computed(() => ({ class: attrs.class, style: attrs.style }))
const triggerAttrs = computed(() => {
  const { class: _c, style: _s, ...rest } = attrs
  return rest
})

const isOpen = ref(false)
const generatedId = useId()
const triggerId = computed(() => `dropdown-${generatedId}`)
const describedById = computed(() =>
  props.error || props.description ? `${triggerId.value}-desc` : undefined,
)

const { isMobile } = useResponsiveSheet({
  open: isOpen,
  onDismiss: () => (isOpen.value = false),
})

// Min width 240px gives 30-35ch of option text before wrapping kicks
// in. Short triggers still get a readable menu; wide triggers still match.
const MENU_MIN_WIDTH = 240

// ---- Selection state -------------------------------------------------

const ALL = 'all' as T

const selectedValues = computed((): T[] => {
  if (props.multiple) return Array.isArray(props.modelValue) ? props.modelValue : []
  return props.modelValue !== '' && props.modelValue != null ? [props.modelValue as T] : []
})

const isSelected = (value: T): boolean => selectedValues.value.includes(value)

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
)

const realOptionValues = computed(() =>
  props.options.filter((o) => o.value !== ALL).map((o) => o.value),
)

const allSelected = computed(() => {
  if (!props.multiple) return false
  return realOptionValues.value.every((v) => selectedValues.value.includes(v))
})

const isChecked = (option: DropdownOption<T>): boolean =>
  option.value === ALL ? allSelected.value : isSelected(option.value)

const displayText = computed(() => {
  if (props.multiple) {
    const selected = selectedValues.value.filter((v) => v !== ALL)
    if (selected.length === 0) return resolvedPlaceholder.value
    const allOption = props.options.find((o) => o.value === ALL)
    if (selected.length === realOptionValues.value.length && allOption) return allOption.label
    if (selected.length === 1) {
      return props.options.find((o) => o.value === selected[0])?.label || String(selected[0])
    }
    return fluent.$t('common-n-selected', { count: selected.length })
  }
  return selectedOption.value?.label || resolvedPlaceholder.value
})

const hasSelection = computed(() => {
  if (props.multiple) return selectedValues.value.filter((v) => v !== ALL).length > 0
  return !!selectedOption.value
})

// Callers use '' for a "none" / "any" option, but Reka's SelectItem throws on
// an empty value (Reka keeps '' for "cleared"). Swap it for a sentinel at the
// Reka boundary only; callers and the phone sheet keep ''.
const EMPTY = '__base_dropdown_empty__' as T
const toReka = (v: T): T => (v === '' ? EMPTY : v)
const fromReka = (v: T): T => (v === EMPTY ? ('' as T) : v)
const rekaModelValue = computed(() =>
  Array.isArray(props.modelValue) ? props.modelValue.map(toReka) : toReka(props.modelValue as T),
)

// Reka reports the raw toggled array; translate the `all` meta option
// into the real selection before it reaches the consumer.
function onModelUpdate(raw: T | T[] | undefined) {
  if (!props.multiple) {
    if (raw !== undefined && raw !== null) emit('update:modelValue', fromReka(raw as T))
    return
  }
  const arr = Array.isArray(raw) ? raw.map(fromReka) : []
  const wasAll = allSelected.value
  const hasAllToken = arr.includes(ALL)
  if (hasAllToken && !wasAll) return emit('update:modelValue', [...realOptionValues.value])
  if (hasAllToken && wasAll) return emit('update:modelValue', [])
  emit('update:modelValue', arr.filter((v) => v !== ALL))
}

// The sheet has no Reka model; it toggles the same way.
function selectFromSheet(option: DropdownOption<T>) {
  if (option.disabled) return
  if (!props.multiple) {
    emit('update:modelValue', option.value)
    isOpen.value = false
    return
  }
  const current = selectedValues.value.filter((v) => v !== ALL)
  if (option.value === ALL) {
    emit('update:modelValue', allSelected.value ? [] : [...realOptionValues.value])
    return
  }
  const i = current.indexOf(option.value)
  if (i === -1) current.push(option.value)
  else current.splice(i, 1)
  emit('update:modelValue', current)
}

// ---- Sizing ---------------------------------------------------------

const sizeClasses = computed(() => {
  switch (props.size) {
    case 'xs':
      // A 26px trigger is what the toolbars use; grow the target on a
      // coarse pointer (44px floor) without touching the dense desktop
      // size.
      return {
        button: 'px-1.5 py-0.5 text-sm pointer-coarse:min-h-[44px] pointer-coarse:px-3',
        menu: 'text-sm',
        option: 'px-3 py-1.5 pointer-coarse:min-h-[44px]',
      }
    case 'sm':
      return { button: 'px-3 py-1.5 text-sm', menu: 'text-sm', option: 'px-3 py-2' }
    case 'lg':
      return { button: 'px-4 py-3.5 text-base', menu: 'text-base', option: 'px-4 py-3' }
    default:
      return { button: 'px-4 py-3 text-sm', menu: 'text-sm', option: 'px-4 py-2.5' }
  }
})

const triggerClasses = computed(() => [
  'w-full bg-surface-alt border rounded-lg text-left flex items-center justify-between transition-all duration-200',
  sizeClasses.value.button,
  props.error ? 'border-status-error' : 'border-subtle',
  props.disabled
    ? 'opacity-50 cursor-not-allowed'
    : 'hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent cursor-pointer',
  isOpen.value && !props.disabled ? 'border-accent ring-1 ring-accent' : '',
])

const optionClasses = (option: DropdownOption<T>) => [
  'w-full text-left text-primary transition-colors flex items-center gap-3 outline-none',
  sizeClasses.value.option,
  option.disabled
    ? 'opacity-40 cursor-not-allowed'
    : isChecked(option)
      ? 'bg-accent/10 text-accent'
      : 'hover:bg-surface-hover data-[highlighted]:bg-surface-hover',
]

const contentStyle = {
  minWidth: `max(${MENU_MIN_WIDTH}px, var(--reka-select-trigger-width))`,
  maxHeight: 'min(16rem, var(--reka-select-content-available-height))',
}
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

    <!-- Phone: hand-rolled trigger + the app's bottom sheet, with
         option rows on the same DropdownOptionRow as the desktop list. -->
    <template v-if="isMobile">
      <button
        :id="triggerId"
        type="button"
        v-bind="triggerAttrs"
        :disabled="disabled"
        :aria-expanded="isOpen"
        aria-haspopup="dialog"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        :class="triggerClasses"
        @click="!disabled && (isOpen = !isOpen)"
      >
        <DropdownValue :text="displayText" :has-selection="hasSelection" :option="selectedOption" />
        <span
          class="text-tertiary flex-shrink-0 ml-2 transition-transform duration-200 inline-flex"
          :class="{ 'rotate-180': isOpen }"
        >
          <Icon name="chevronDown" />
        </span>
      </button>
      <BottomSheet
        :open="isOpen"
        :title="label ?? resolvedPlaceholder"
        body-role="listbox"
        @close="isOpen = false"
      >
        <div class="py-1" :class="sizeClasses.menu">
          <button
            v-for="option in options"
            :key="String(option.value)"
            type="button"
            role="option"
            :aria-selected="isChecked(option)"
            :disabled="option.disabled"
            :class="optionClasses(option)"
            @click="selectFromSheet(option)"
          >
            <DropdownOptionRow :option="option" :multiple="multiple" :checked="isChecked(option)" />
          </button>
        </div>
      </BottomSheet>
    </template>

    <!-- Desktop: Reka Select. -->
    <SelectRoot
      v-else
      v-model:open="isOpen"
      :model-value="rekaModelValue"
      :multiple="multiple"
      :disabled="disabled"
      @update:model-value="onModelUpdate"
    >
      <SelectTrigger
        :id="triggerId"
        v-bind="triggerAttrs"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        :class="triggerClasses"
      >
        <DropdownValue :text="displayText" :has-selection="hasSelection" :option="selectedOption" />
        <span
          class="text-tertiary flex-shrink-0 ml-2 transition-transform duration-200 inline-flex"
          :class="{ 'rotate-180': isOpen }"
        >
          <Icon name="chevronDown" />
        </span>
      </SelectTrigger>
      <SelectPortal>
        <SelectContent
          position="popper"
          side="bottom"
          align="start"
          :side-offset="2"
          :collision-padding="8"
          class="popover-inner select-surface z-overlay bg-surface border border-default rounded-lg shadow-xl overflow-hidden"
          :style="contentStyle"
        >
          <SelectViewport class="py-1 overflow-y-auto" :class="sizeClasses.menu">
            <SelectItem
              v-for="option in options"
              :key="String(option.value)"
              as-child
              :value="toReka(option.value)"
              :disabled="option.disabled"
              :text-value="option.label"
            >
              <button type="button" :class="optionClasses(option)">
                <DropdownOptionRow :option="option" :multiple="multiple" :checked="isChecked(option)">
                  <template #label>
                    <SelectItemText>{{ option.label }}</SelectItemText>
                  </template>
                </DropdownOptionRow>
              </button>
            </SelectItem>
          </SelectViewport>
        </SelectContent>
      </SelectPortal>
    </SelectRoot>

    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>

<style>
.select-surface {
  transform-origin: var(--reka-select-content-transform-origin);
}
</style>
