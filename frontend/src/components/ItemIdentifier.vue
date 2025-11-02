<!-- ItemIdentifier.vue - Generic identifier component for tickets, devices, etc. -->
<script setup lang="ts">
import { computed } from 'vue';

interface Props {
  id?: number | string;
  showPrefix?: boolean;
  size?: 'sm' | 'md' | 'lg';
}

const props = withDefaults(defineProps<Props>(), {
  showPrefix: true,
  size: 'md'
});

// Compute the display text
const displayText = computed(() => {
  if (!props.id) return '';
  return props.showPrefix ? `#${props.id}` : `${props.id}`;
});

// Size classes for different display sizes
const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm': return 'text-sm';
    case 'lg': return 'text-lg font-semibold';
    default: return 'text-base';
  }
});
</script>

<template>
  <div v-if="id" class="flex items-center">
    <span
      class="text-secondary font-medium flex items-center select-none"
      :class="sizeClasses"
    >
      {{ displayText }}
    </span>
  </div>
</template>
