<template>
  <div
    class="bg-surface-alt rounded animate-pulse"
    :class="rounded"
    :style="{ width, height }"
    aria-hidden="true"
  ></div>
</template>

<script setup lang="ts">
// Minimal building block for skeleton loaders on the public pages.
// Composes into whatever shape the real content will occupy so the
// page doesn't jump when data arrives.
withDefaults(
  defineProps<{
    width?: string;
    height?: string;
    rounded?: string;
  }>(),
  { width: '100%', height: '1rem', rounded: 'rounded' }
);
</script>
