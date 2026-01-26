<script setup lang="ts">
interface Props {
  text?: string;
  size?: 'sm' | 'md' | 'lg';
  /** Use 'inline' for no wrapper styling, 'centered' for centered with margin */
  variant?: 'centered' | 'inline';
}

const props = withDefaults(defineProps<Props>(), {
  size: 'md',
  variant: 'centered'
});

const sizeClasses = {
  sm: 'h-5 w-5',
  md: 'h-8 w-8',
  lg: 'h-12 w-12'
};
</script>

<template>
  <div :class="variant === 'centered' ? 'flex justify-center my-8' : 'flex items-center gap-2'">
    <div class="animate-spin text-current" :class="sizeClasses[size]">
      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
      </svg>
    </div>
    <span v-if="text" class="text-current text-sm font-medium">{{ text }}</span>
  </div>
</template>
