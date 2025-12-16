<script setup lang="ts">
interface Props {
  value: string
  type?: 'status' | 'priority' | 'warranty' | 'role'
}

const props = withDefaults(defineProps<Props>(), {
  type: 'status'
})

const getStatusClasses = (value: string, type: string) => {
  const baseClasses = 'text-xs px-2 py-1 rounded-full whitespace-nowrap'

  if (type === 'warranty') {
    switch (value) {
      case 'Active':
        return `${baseClasses} bg-green-400/20 dark:bg-green-500/20 [color:#14532d] dark:text-green-200`
      case 'Warning':
        return `${baseClasses} bg-amber-400/20 dark:bg-amber-500/20 [color:#78350f] dark:text-amber-200`
      case 'Expired':
        return `${baseClasses} bg-red-400/20 dark:bg-red-500/20 [color:#7f1d1d] dark:text-red-200`
      case 'Unknown':
        return `${baseClasses} text-tertiary bg-surface-alt`
      default:
        return `${baseClasses} text-tertiary bg-surface-alt`
    }
  }

  if (type === 'role') {
    return `${baseClasses} bg-surface-alt text-primary`
  }

  // Default for status/priority
  return `${baseClasses} bg-surface-alt text-primary`
}
</script>

<template>
  <span :class="getStatusClasses(value, type)">
    {{ value }}
  </span>
</template> 