<script setup lang="ts">
/**
 * SectionCard - A standardized card component with optional header
 *
 * Provides consistent styling for cards throughout the application:
 * - Proper rounded corners with overflow-hidden
 * - Optional header with contrasting background
 * - Consistent border and hover states
 * - Flexible content area
 */

interface Props {
  /** Show the header section */
  showHeader?: boolean;
  /** Card size variant */
  size?: 'sm' | 'md' | 'lg';
  /** Disable hover border effect */
  noHover?: boolean;
  /** Custom padding for content area */
  contentPadding?: string;
}

const props = withDefaults(defineProps<Props>(), {
  showHeader: true,
  size: 'md',
  noHover: false,
  contentPadding: 'p-3'
});

// Size-based padding for header
const headerPadding = {
  sm: 'px-3 py-2',
  md: 'px-4 py-3',
  lg: 'px-6 py-4'
};
</script>

<template>
  <div
    class="bg-surface rounded-xl border border-default transition-colors overflow-hidden"
    :class="{ 'hover:border-strong': !noHover }"
  >
    <!-- Header Section (optional) -->
    <div
      v-if="showHeader"
      class="bg-surface-alt border-b border-default"
      :class="headerPadding[size]"
    >
      <slot name="header">
        <!-- Default header content if none provided -->
        <h2 class="text-lg font-medium text-primary">
          <slot name="title"></slot>
        </h2>
      </slot>
    </div>

    <!-- Content Section -->
    <div :class="contentPadding">
      <slot></slot>
    </div>
  </div>
</template>
