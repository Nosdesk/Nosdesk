<!-- PageUrlDisplay.vue -->
<script setup lang="ts">
import { computed } from 'vue';

interface Props {
  url?: string;
  showIcon?: boolean;
  size?: 'sm' | 'md' | 'lg';
}

const props = withDefaults(defineProps<Props>(), {
  showIcon: true,
  size: 'md'
});

// Size classes for different display sizes
const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm': return 'text-sm';
    case 'lg': return 'text-lg';
    default: return 'text-base';
  }
});

// Format URL for display (remove protocol, etc.)
const displayUrl = computed(() => {
  if (!props.url) return '';
  
  // Remove protocol and trailing slashes
  let formatted = props.url.replace(/^(https?:\/\/)?(www\.)?/, '');
  formatted = formatted.replace(/\/$/, '');
  
  return formatted;
});
</script>

<template>
  <div v-if="url" class="flex items-center text-secondary">
    <span v-if="showIcon" class="mr-1">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
      </svg>
    </span>
    <span 
      class="font-medium truncate max-w-xs"
      :class="sizeClasses"
    >
      {{ displayUrl }}
    </span>
  </div>
</template>

<style scoped>
/* Add any additional styles here */
</style> 