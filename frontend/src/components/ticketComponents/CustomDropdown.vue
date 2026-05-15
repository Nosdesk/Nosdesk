<script setup lang="ts">
/**
 * Ticket sidebar status / priority / category picker. Trigger
 * styling and option rendering stay bespoke — the trigger is
 * intentionally transparent (it sits in a card-style row) and
 * the StatusIndicator / PriorityIndicator atoms have a
 * color-blind-friendly shape variant that BaseDropdown's
 * generic tone-dot mechanism doesn't express.
 *
 * What's no longer bespoke: positioning, dismiss, scroll-tracking,
 * focus, breakpoint detection, drag-to-dismiss bottom sheet —
 * all delegated to `<ResponsiveMenu>`. That cuts ~150 lines of
 * floating-element code that used to be hand-rolled here, and
 * gives mobile users the same touch-friendly sheet treatment
 * every other dropdown in the app already has.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import { paletteForColor } from '@/utils/workflowColors'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

interface DropdownOption {
  value: string
  label: string
  /** When true, the option renders as a non-selectable group header. */
  disabled?: boolean
  /**
   * Workflow-state design-token color. When present (status type
   * only), the row renders a colored dot using this token instead of
   * the legacy three-bucket StatusIndicator shapes.
   */
  color?: string
}

const props = defineProps<{
  value: string
  options: DropdownOption[]
  type: 'status' | 'priority' | 'category'
  /** Mobile sheet header label. Defaults to a sensible value
   * derived from `type` so existing call sites don't need to
   * pass it. */
  placeholder?: string
}>()

const emit = defineEmits<{
  (e: 'update:value', value: string): void
}>()

const LEGACY_STATUS_VALUES = new Set(['open', 'in-progress', 'closed'])
function isLegacyStatusValue(v: string): boolean {
  return LEGACY_STATUS_VALUES.has(v)
}

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.value),
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

function selectOption(option: DropdownOption) {
  if (option.disabled) return
  emit('update:value', option.value)
  isOpen.value = false
}
</script>

<template>
  <div class="relative">
    <button
      ref="triggerRef"
      type="button"
      @click="toggle"
      class="w-full px-3 py-2.5 sm:py-2 min-h-[44px] sm:min-h-[40px] bg-transparent text-primary text-left flex items-center justify-between hover:bg-surface-hover active:bg-surface-alt transition-colors rounded-lg cursor-pointer"
    >
      <div class="flex items-center gap-2.5 sm:gap-2">
        <template v-if="type === 'status'">
          <StatusIndicator
            v-if="isLegacyStatusValue(value)"
            :status="value as 'open' | 'in-progress' | 'closed'"
            size="sm"
          />
          <span
            v-else
            :class="['inline-block w-2.5 h-2.5 rounded-full', paletteForColor(selectedOption?.color).solid, 'bg-current']"
            aria-hidden="true"
          />
        </template>
        <PriorityIndicator
          v-else-if="type === 'priority'"
          :priority="value as 'low' | 'medium' | 'high'"
          size="sm"
        />
        <span class="text-sm font-medium">{{ selectedOption?.label || $t('ticket-chip-dropdown-select') }}</span>
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

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      :title="sheetTitle"
      placement="bottom-start"
      react-to-scroll="reposition"
      match-anchor-width
      :min-width="160"
      :offset="4"
      :auto-focus="false"
      role="listbox"
      popover-class="bg-surface border border-default rounded-xl shadow-2xl overflow-hidden"
      @close="isOpen = false"
    >
      <div class="py-1">
        <template v-for="option in options" :key="option.value">
          <div
            v-if="option.disabled"
            class="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wide text-tertiary font-semibold select-none"
          >
            {{ option.label }}
          </div>
          <button
            v-else
            @click="selectOption(option)"
            class="w-full px-3 py-2.5 md:py-2 min-h-[44px] md:min-h-0 text-left text-primary hover:bg-surface-hover active:bg-surface-alt transition-colors flex items-center gap-2.5"
            :class="{ 'bg-accent/10': option.value === value }"
          >
            <template v-if="type === 'status'">
              <StatusIndicator
                v-if="isLegacyStatusValue(option.value)"
                :status="option.value as 'open' | 'in-progress' | 'closed'"
                size="sm"
              />
              <span
                v-else
                :class="['inline-block w-2.5 h-2.5 rounded-full bg-current', paletteForColor(option.color).solid]"
                aria-hidden="true"
              />
            </template>
            <PriorityIndicator
              v-else-if="type === 'priority'"
              :priority="option.value as 'low' | 'medium' | 'high'"
              size="sm"
            />
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
        </template>
      </div>
    </ResponsiveMenu>
  </div>
</template>
