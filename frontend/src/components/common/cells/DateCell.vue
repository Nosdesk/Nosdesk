<script setup lang="ts">
interface Props {
  value: string | Date
  format?: 'short' | 'long' | 'relative'
  emptyText?: string
}

const props = withDefaults(defineProps<Props>(), {
  format: 'short',
  emptyText: 'Never'
})

const formatDate = (dateValue: string | Date, format: string) => {
  try {
    const date = new Date(dateValue)
    if (isNaN(date.getTime())) {
      return props.emptyText
    }

    if (format === 'short') {
      return date.toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
      })
    }
    
    if (format === 'long') {
      return date.toLocaleString()
    }
    
    if (format === 'relative') {
      const now = new Date()
      const diffInMs = now.getTime() - date.getTime()
      const diffInDays = Math.floor(diffInMs / (1000 * 60 * 60 * 24))
      
      if (diffInDays === 0) return 'Today'
      if (diffInDays === 1) return 'Yesterday'
      if (diffInDays < 7) return `${diffInDays} days ago`
      if (diffInDays < 30) return `${Math.floor(diffInDays / 7)} weeks ago`
      if (diffInDays < 365) return `${Math.floor(diffInDays / 30)} months ago`
      return `${Math.floor(diffInDays / 365)} years ago`
    }
    
    return date.toLocaleDateString()
  } catch (error) {
    console.error('Error formatting date:', error)
    return props.emptyText
  }
}
</script>

<template>
  <span class="text-secondary text-sm">
    {{ formatDate(value, format) }}
  </span>
</template> 