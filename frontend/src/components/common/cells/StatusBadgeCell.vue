<script setup lang="ts">
interface Props {
  value: string
  type?: 'status' | 'priority' | 'warranty' | 'role'
}

const props = withDefaults(defineProps<Props>(), {
  type: 'status'
})

const getStatusClasses = (value: string, type: string) => {
  const baseClasses = 'text-xs px-2 py-1 rounded-full whitespace-nowrap border'

  if (type === 'warranty') {
    switch (value) {
      case 'Active':
        return `${baseClasses} bg-status-success-muted text-status-success border-status-success/30`
      case 'Warning':
        return `${baseClasses} bg-status-warning-muted text-status-warning border-status-warning/30`
      case 'Expired':
        return `${baseClasses} bg-status-error-muted text-status-error border-status-error/30`
      case 'Unknown':
        return `${baseClasses} bg-surface-alt text-secondary border-default`
      default:
        return `${baseClasses} bg-surface-alt text-secondary border-default`
    }
  }

  if (type === 'status') {
    switch (value?.toLowerCase()) {
      case 'open':
        return `${baseClasses} bg-status-open-muted text-status-open border-status-open/30`
      case 'in-progress':
      case 'in progress':
        return `${baseClasses} bg-status-in-progress-muted text-status-in-progress border-status-in-progress/30`
      case 'closed':
      case 'resolved':
        return `${baseClasses} bg-status-closed-muted text-status-closed border-status-closed/30`
      default:
        return `${baseClasses} bg-surface-alt text-secondary border-default`
    }
  }

  if (type === 'priority') {
    switch (value?.toLowerCase()) {
      case 'high':
        return `${baseClasses} bg-priority-high-muted text-priority-high border-priority-high/30`
      case 'medium':
        return `${baseClasses} bg-priority-medium-muted text-priority-medium border-priority-medium/30`
      case 'low':
        return `${baseClasses} bg-priority-low-muted text-priority-low border-priority-low/30`
      default:
        return `${baseClasses} bg-surface-alt text-secondary border-default`
    }
  }

  if (type === 'role') {
    switch (value?.toLowerCase()) {
      case 'admin':
        return `${baseClasses} bg-status-error-muted text-status-error border-status-error/30`
      case 'technician':
        return `${baseClasses} bg-accent-muted text-accent border-accent/30`
      default:
        return `${baseClasses} bg-surface-alt text-secondary border-default`
    }
  }

  // Default fallback
  return `${baseClasses} bg-surface-alt text-secondary border-default`
}
</script>

<template>
  <span :class="getStatusClasses(value, type)">
    {{ value }}
  </span>
</template> 