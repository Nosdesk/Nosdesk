<script setup lang="ts">
interface Props {
  value: string
  type?: 'status' | 'priority' | 'warranty' | 'role' | 'membership'
  /** Display text override. The `value` keys the colour map, so pass
   *  the localized string here rather than translating `value` and
   *  losing the match. */
  label?: string
  /** `xs` trims the vertical padding from `py-1` to `py-0.5`, taking
   *  the badge from 26px to 22px so it fits inside a compact table
   *  row's 24px content box without clipping against its neighbours. */
  size?: 'sm' | 'xs'
}

const props = withDefaults(defineProps<Props>(), {
  type: 'status',
  label: undefined,
  size: 'sm'
})

const getStatusClasses = (value: string, type: string) => {
  const baseClasses = `text-xs px-2 ${props.size === 'xs' ? 'py-0.5' : 'py-1'} rounded-full whitespace-nowrap border`

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

  // Covers both role vocabularies: the platform roles on the users
  // list (admin / technician / audit_reviewer / user) and the
  // workspace roles on the Team view (owner / admin / agent /
  // member). The two never appear in the same table, so `owner` can
  // reuse the purple `audit_reviewer` treatment and `agent` the
  // accent `technician` one without them reading as the same thing.
  if (type === 'role') {
    switch (value?.toLowerCase()) {
      case 'admin':
        return `${baseClasses} bg-status-error-muted text-status-error border-status-error/30`
      case 'technician':
      case 'agent':
        return `${baseClasses} bg-accent-muted text-accent border-accent/30`
      case 'audit_reviewer':
      case 'owner':
        return `${baseClasses} bg-purple-500/10 text-purple-700 dark:text-purple-400 border-purple-500/30`
      default:
        return `${baseClasses} bg-surface-alt text-secondary border-default`
    }
  }

  // Workspace membership: accepted the invite, or still pending.
  if (type === 'membership') {
    return value === 'active'
      ? `${baseClasses} bg-status-success-muted text-status-success border-status-success/30`
      : `${baseClasses} bg-status-warning-muted text-status-warning border-status-warning/30`
  }

  // Default fallback
  return `${baseClasses} bg-surface-alt text-secondary border-default`
}

// Role values are stored snake_case (e.g. audit_reviewer); render them
// with spaces so the badge reads naturally. Other badge types pass
// through unchanged.
const displayText = (value: string, type: string) =>
  type === 'role' ? value?.replace(/_/g, ' ') : value
</script>

<template>
  <span :class="getStatusClasses(value, type)">
    {{ label ?? displayText(value, type) }}
  </span>
</template>