<!-- PageUrlDisplay.vue -->
<script setup lang="ts">
import { computed } from 'vue';
import Icon from '@/components/common/Icon.vue';

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
      <Icon name="link" />
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