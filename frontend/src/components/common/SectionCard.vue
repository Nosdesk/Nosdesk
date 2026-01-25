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

<style scoped>
@media print {
  /* Compact header for print */
  .bg-surface-alt {
    background: transparent !important;
    padding: 0 0 4pt 0 !important;
    border-bottom: 1px solid #ccc !important;
    margin-bottom: 6pt;
  }

  .bg-surface-alt :deep(h2) {
    font-size: 10pt !important;
    font-weight: 600 !important;
    margin: 0 !important;
  }

  /* Remove card styling for print */
  .bg-surface {
    background: transparent !important;
    border: none !important;
    border-radius: 0 !important;
  }

  /* Reduce content padding */
  .p-3 {
    padding: 0 !important;
  }
}
</style>
