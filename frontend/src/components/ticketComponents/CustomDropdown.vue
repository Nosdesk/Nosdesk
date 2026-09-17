<script setup lang="ts">
/**
 * Ticket sidebar status / priority / category picker. Trigger
 * styling and option rendering stay bespoke — the trigger is
 * intentionally transparent (it sits in a card-style row) and
 * the StatusIndicator / PriorityIndicator atoms have a
 * color-blind-friendly shape variant that BaseDropdown's
 * generic tone-dot mechanism doesn't express. Status rows
 * render via `WorkflowStateGlyph` when options carry `category`.
 *
 * What's no longer bespoke: positioning, dismiss, scroll-tracking,
 * focus, breakpoint detection, drag-to-dismiss bottom sheet, all
 * delegated to `<ResponsiveMenu>`; and the option list itself, a
 * Reka Listbox (`role=listbox` of `role=option` rows, arrow keys,
 * Home/End, typeahead, Enter to pick) inside a `dialog` surface,
 * the same shape as `SearchableDropdown`.
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { ListboxContent, ListboxGroup, ListboxGroupLabel, ListboxItem, ListboxRoot } from 'reka-ui'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import WorkflowStateGlyph from '@/components/views/WorkflowStateGlyph.vue'
import type { WorkflowDropdownOption } from '@nosdesk/core/types/workflow'
import { paletteForColor } from '@nosdesk/core/utils/workflowColors'
import { priorityForBadge } from '@/utils/priorityHelpers'
import type { Priority as CardPriority } from '@nosdesk/core/sync/views/types'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

type DropdownOption = WorkflowDropdownOption

const props = defineProps<{
  value: string
  options: DropdownOption[]
  type: 'status' | 'priority' | 'category'
  /** Mobile sheet header label. Defaults to a sensible value
   * derived from `type` so existing call sites don't need to
   * pass it. */
  placeholder?: string
  /** Dense trigger for split-view preview / property rows. */
  compact?: boolean
  /** Shrink-wrap the trigger instead of filling the row width. */
  inline?: boolean
}>()

const emit = defineEmits<{
  (e: 'update:value', value: string): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const listbox = ref<{ highlightSelected: () => Promise<void> } | null>(null)

// Focus lands on the current option, as a native select would: Reka
// highlights it on mount without focusing, and the popover's own
// focus scope would take the first option instead, so it is off.
watch(isOpen, async (open) => {
  if (!open) return
  await nextTick()
  await listbox.value?.highlightSelected()
})

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

// Disabled entries are section headings (the call sites use them that
// way): each starts a group holding the options that follow it.
const groups = computed(() => {
  const out: { heading: DropdownOption | null; options: DropdownOption[] }[] = []
  for (const option of props.options) {
    if (option.disabled) out.push({ heading: option, options: [] })
    else if (out.length) out[out.length - 1].options.push(option)
    else out.push({ heading: null, options: [option] })
  }
  return out
})

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.value),
)

// True when the trigger should read as "no value selected" —
// either no option matches `value`, OR the matching option's
// value is the empty-string sentinel (which call sites use to
// represent "uncategorised" / "no priority"; see
// `categoryOptions` in TicketView.vue). Without the empty-string
// branch the placeholder would render in primary body weight
// because the dropdown technically has a "selection", which is
// the design-review issue surfaced for the Category row.
const isEmptySelection = computed(
  () => !props.value || !selectedOption.value,
)

const sheetTitle = computed(() => {
  if (props.placeholder) return props.placeholder
  switch (props.type) {
    case 'status':
      return t('ticket-chip-dropdown-status')
    case 'priority':
      return t('ticket-chip-dropdown-priority')
    case 'category':
      return t('ticket-chip-dropdown-category')
    default:
      return t('ticket-chip-dropdown-option')
  }
})

function toggle() {
  isOpen.value = !isOpen.value
}

function onSelect(value: unknown) {
  if (typeof value !== 'string') return
  emit('update:value', value)
  isOpen.value = false
}

const glyphSize = computed(() => (props.compact ? 12 : 14))
</script>

<template>
  <div class="relative" :class="inline ? 'inline-flex max-w-full' : 'w-full'">
    <button
      ref="triggerRef"
      type="button"
      aria-haspopup="dialog"
      :aria-expanded="isOpen"
      @click="toggle"
      class="group bg-transparent text-primary text-left flex items-center justify-between hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer"
      :class="[
        inline ? 'w-auto max-w-full' : 'w-full',
        compact
          ? 'px-1.5 py-0.5 min-h-0 text-xs rounded-md gap-1.5'
          : 'px-3 py-2.5 sm:py-2 min-h-[44px] sm:min-h-[40px] rounded-lg',
      ]"
    >
      <div class="flex items-center gap-2 min-w-0" :class="compact ? 'gap-1.5' : 'gap-2.5 sm:gap-2'">
        <!-- Indicator only renders when a real value is selected.
             Empty-selection state (including the "" sentinel option
             used by Category) has no chip dot; the placeholder copy
             carries the meaning. Stops the empty-row "shouting in
             primary body weight" pattern flagged in design review. -->
        <WorkflowStateGlyph
          v-if="!isEmptySelection && type === 'status' && selectedOption?.category && selectedOption?.color"
          :category="selectedOption.category"
          :color="selectedOption.color"
          :name="selectedOption.label"
          :size="glyphSize"
        />
        <template v-else-if="!isEmptySelection && type === 'status' && selectedOption?.color">
          <span
            :class="['inline-block w-2.5 h-2.5 rounded-full', paletteForColor(selectedOption.color).solid, 'bg-current']"
            aria-hidden="true"
          />
        </template>
        <PriorityIndicator
          v-else-if="!isEmptySelection && type === 'priority' && priorityForBadge(value as CardPriority)"
          :priority="priorityForBadge(value as CardPriority)!"
          :size="compact ? 'xs' : 'sm'"
        />
        <span
          class="truncate"
          :class="[
            compact ? 'text-2xs leading-tight' : 'text-sm',
            isEmptySelection ? 'text-tertiary' : compact && type === 'priority' ? 'font-medium' : 'text-primary font-medium',
          ]"
        >{{ selectedOption?.label || placeholder || $t('ticket-chip-dropdown-select') }}</span>
      </div>
      <!-- Chevron hidden at rest, revealed on hover (or whenever the
           menu is open so the user sees the rotation cue). Always
           visible on coarse pointers (touch) since there's no hover
           state to reveal with. Matches the "display, click to edit"
           register the rest of the sidebar settled on. -->
      <svg
        class="text-tertiary transition-all duration-200 shrink-0"
        :class="[
          compact ? 'w-3 h-3' : 'w-4 h-4',
          compact ? 'opacity-60 group-hover:opacity-100' : 'opacity-0 group-hover:opacity-100 pointer-coarse:opacity-100',
          { 'rotate-180 opacity-100': isOpen },
        ]"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
      </svg>
    </button>

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      :title="sheetTitle"
      placement="bottom-start"
      react-to-scroll="reposition"
      match-anchor-width
      :min-width="160"
      :offset="4"
      role="dialog"
      :aria-label="sheetTitle"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-xl shadow-2xl overflow-hidden"
      @close="isOpen = false"
    >
      <!-- `replace`: picking the current value again is still a pick
           (it closes the list), not a deselect. -->
      <ListboxRoot
        ref="listbox"
        :model-value="value"
        selection-behavior="replace"
        highlight-on-hover
        @update:model-value="onSelect"
      >
        <ListboxContent as-child>
          <div class="py-1 outline-none" :aria-label="sheetTitle">
            <ListboxGroup v-for="(group, i) in groups" :key="group.heading?.value ?? i">
              <ListboxGroupLabel
                v-if="group.heading"
                class="px-3 pt-2 pb-1 text-3xs uppercase tracking-wide text-tertiary font-semibold select-none"
              >
                {{ group.heading.label }}
              </ListboxGroupLabel>
              <ListboxItem v-for="option in group.options" :key="option.value" as-child :value="option.value">
                <button
                  type="button"
                  class="w-full px-3 py-2.5 md:py-2 min-h-[44px] md:min-h-0 text-left text-primary hover:bg-surface-hover data-[highlighted]:bg-surface-hover active:bg-surface-alt transition-colors flex items-center gap-2.5 outline-none"
                  :class="{ 'bg-accent/10': option.value === value }"
                >
                  <WorkflowStateGlyph
                    v-if="type === 'status' && option.category && option.color"
                    :category="option.category"
                    :color="option.color"
                    :name="option.label"
                    :size="14"
                  />
                  <template v-else-if="type === 'status' && option.color">
                    <span
                      :class="['inline-block w-2.5 h-2.5 rounded-full bg-current', paletteForColor(option.color).solid]"
                      aria-hidden="true"
                    />
                  </template>
                  <PriorityIndicator
                    v-else-if="type === 'priority' && priorityForBadge(option.value as CardPriority)"
                    :priority="priorityForBadge(option.value as CardPriority)!"
                    size="sm"
                  />
                  <span
                    v-else-if="type === 'priority'"
                    class="inline-flex w-3 h-3 items-center justify-center shrink-0"
                    aria-hidden="true"
                  >
                    <span class="w-2 h-2 rounded-full border border-tertiary" />
                  </span>
                  <span class="text-sm flex-1" :class="{ 'font-medium': option.value === value }">
                    {{ option.label }}
                  </span>
                  <svg
                    v-if="option.value === value"
                    class="w-4 h-4 text-accent flex-shrink-0"
                    fill="currentColor"
                    viewBox="0 0 20 20"
                  >
                    <path
                      fill-rule="evenodd"
                      d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
                      clip-rule="evenodd"
                    />
                  </svg>
                </button>
              </ListboxItem>
            </ListboxGroup>
          </div>
        </ListboxContent>
      </ListboxRoot>
    </ResponsiveMenu>
  </div>
</template>
