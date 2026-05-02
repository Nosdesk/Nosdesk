// components/StatusBadge.vue
<script setup lang="ts">
import { computed } from 'vue'
import type { WorkflowState } from '@/types/workflow'
import { paletteForColor } from '@/utils/workflowColors'

const props = defineProps<{
  type: 'status' | 'priority'
  value: 'open' | 'in-progress' | 'closed' | 'low' | 'medium' | 'high'
  /**
   * Joined workflow state. When provided, the badge renders the state's
   * configured name and color instead of the legacy three-bucket label.
   * Falls back to `value` when absent so existing callers keep working.
   */
  workflowState?: WorkflowState | null
  customClasses?: string
  short?: boolean
  compact?: boolean
}>()

// Status badges using semantic color tokens
// These classes use CSS variables that adapt to any theme
const statusConfig: Record<string, string> = {
  open: 'bg-status-open-muted text-status-open border border-status-open/30',
  'in-progress': 'bg-status-in-progress-muted text-status-in-progress border border-status-in-progress/30',
  closed: 'bg-status-closed-muted text-status-closed border border-status-closed/30',
}

// Used when the caller passes a value outside the legacy three-bucket
// set AND no workflowState prop is present (e.g. an embedded ticket
// payload that ships a custom state name without the joined object).
const STATUS_FALLBACK = 'bg-surface-alt text-secondary border border-default'

// Priority badges using semantic color tokens
const priorityConfig = {
  low: 'bg-priority-low-muted text-priority-low border border-priority-low/30',
  medium: 'bg-priority-medium-muted text-priority-medium border border-priority-medium/30',
  high: 'bg-priority-high-muted text-priority-high border border-priority-high/30',
}

const displayText = computed(() => {
  // If customClasses includes w- and h- (likely a circle indicator), don't show text
  if (props.customClasses?.includes('w-') && props.customClasses?.includes('h-')) {
    return ''
  }

  if (props.type === 'status') {
    if (props.workflowState) return props.workflowState.name
    return props.value
  }

  // For priority type: use short form if short prop is true
  if (props.short) {
    return props.value
  }

  return `${props.value} priority`
})

const colorClasses = computed(() => {
  if (props.type === 'status') {
    if (props.workflowState) return paletteForColor(props.workflowState.color).badge
    return statusConfig[props.value] ?? STATUS_FALLBACK
  }
  return priorityConfig[props.value as 'low' | 'medium' | 'high']
})

const badgeClasses = computed(() => {
  const sizeClasses =
    props.type === 'status'
      ? props.compact
        ? 'px-2 py-0.5 rounded text-xs'
        : 'px-3 py-1 rounded-full text-sm'
      : props.compact
        ? 'px-1.5 py-0.5 rounded text-xs font-medium'
        : 'px-2 py-0.5 rounded font-medium'

  return props.customClasses
    ? [colorClasses.value, sizeClasses, props.customClasses]
    : [colorClasses.value, sizeClasses]
})
</script>

<template>
  <span :class="badgeClasses">
    {{ displayText }}
  </span>
</template>
