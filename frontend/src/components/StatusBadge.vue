// components/StatusBadge.vue
<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  type: 'status' | 'priority'
  value: 'open' | 'in-progress' | 'closed' | 'low' | 'medium' | 'high'
  customClasses?: string
  short?: boolean
  compact?: boolean
}>()

// Status badges - WCAG AA compliant (4.5:1 contrast ratio)
// Light mode: -700 shade text on -100 equivalent background (professional, readable)
// Dark mode: -300 shade text on -500/20 background (softer, easier on eyes)
const statusConfig = {
  open: { classes: 'bg-amber-100 dark:bg-amber-500/20 dark:text-amber-300 border border-amber-300 dark:border-amber-500/30', lightColor: '#b45309' },
  'in-progress': { classes: 'bg-blue-100 dark:bg-blue-500/20 dark:text-blue-300 border border-blue-300 dark:border-blue-500/30', lightColor: '#1d4ed8' },
  closed: { classes: 'bg-emerald-100 dark:bg-emerald-500/20 dark:text-emerald-300 border border-emerald-300 dark:border-emerald-500/30', lightColor: '#047857' }
}

// Priority badges use the same pattern
const priorityConfig = {
  low: { classes: 'bg-emerald-100 dark:bg-emerald-500/20 dark:text-emerald-300 border border-emerald-300 dark:border-emerald-500/30', lightColor: '#047857' },
  medium: { classes: 'bg-amber-100 dark:bg-amber-500/20 dark:text-amber-300 border border-amber-300 dark:border-amber-500/30', lightColor: '#b45309' },
  high: { classes: 'bg-red-100 dark:bg-red-500/20 dark:text-red-300 border border-red-300 dark:border-red-500/30', lightColor: '#b91c1c' }
}

const displayText = computed(() => {
  // If customClasses includes w- and h- (likely a circle indicator), don't show text
  if (props.customClasses?.includes('w-') && props.customClasses?.includes('h-')) {
    return ''
  }

  if (props.type === 'status') {
    return props.value
  }

  // For priority type: use short form if short prop is true
  if (props.short) {
    return props.value
  }

  return `${props.value} priority`
})

const config = computed(() => {
  if (props.type === 'status') {
    return statusConfig[props.value as 'open' | 'in-progress' | 'closed']
  }
  return priorityConfig[props.value as 'low' | 'medium' | 'high']
})

const badgeClasses = computed(() => {
  const colorClasses = config.value.classes

  const sizeClasses = props.type === 'status'
    ? (props.compact ? 'px-2 py-0.5 rounded text-xs' : 'px-3 py-1 rounded-full text-sm')
    : (props.compact ? 'px-1.5 py-0.5 rounded text-xs font-medium' : 'px-2 py-0.5 rounded font-medium')

  return props.customClasses
    ? [colorClasses, sizeClasses, props.customClasses]
    : [colorClasses, sizeClasses]
})

// Light mode text colors applied via inline style
// Dark mode colors defined in the classes (dark:text-*) override via CSS specificity
const textStyle = computed(() => {
  return { color: config.value.lightColor }
})
</script>

<template>
  <span :class="badgeClasses" :style="textStyle" class="status-badge">
    {{ displayText }}
  </span>
</template>

<style scoped>
/* Dark mode: override inline style color with Tailwind dark:text-* classes */
:global(.dark) .status-badge {
  color: inherit !important;
}
</style>