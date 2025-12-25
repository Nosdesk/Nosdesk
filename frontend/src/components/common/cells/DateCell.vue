<script setup lang="ts">
import { computed } from 'vue'
import { formatDate, formatRelativeTime, formatSmartDate, formatCompactDate } from '@/utils/dateUtils'
import { useDateStore } from '@/stores/dateStore'

interface Props {
  value: string | Date | null | undefined
  format?: 'short' | 'long' | 'relative' | 'smart' | 'compact'
  emptyText?: string
  showTimezone?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  format: 'short',
  emptyText: 'Never',
  showTimezone: false
})

const dateStore = useDateStore()

const formattedDate = computed(() => {
  if (!props.value) return props.emptyText

  switch (props.format) {
    case 'relative':
      return formatRelativeTime(props.value)
    case 'smart':
      return formatSmartDate(props.value)
    case 'compact':
      return formatCompactDate(props.value)
    case 'long':
      return formatDate(props.value, 'MMMM d, yyyy h:mm a', dateStore.effectiveTimezone)
    case 'short':
    default:
      return formatDate(props.value, undefined, dateStore.effectiveTimezone)
  }
})

const timezoneAbbr = computed(() => {
  if (!props.showTimezone || !props.value) return ''

  try {
    const date = new Date(props.value)
    const formatter = new Intl.DateTimeFormat('en-US', {
      timeZone: dateStore.effectiveTimezone,
      timeZoneName: 'short'
    })

    const parts = formatter.formatToParts(date)
    const tzPart = parts.find(p => p.type === 'timeZoneName')
    return tzPart?.value || ''
  } catch {
    return ''
  }
})
</script>

<template>
  <span class="text-secondary text-sm">
    {{ formattedDate }}
    <span v-if="showTimezone && timezoneAbbr" class="text-xs opacity-60 ml-1">
      {{ timezoneAbbr }}
    </span>
  </span>
</template> 