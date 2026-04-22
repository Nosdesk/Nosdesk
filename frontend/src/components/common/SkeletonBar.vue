<!--
A single shimmer bar. Shape (height, width, radius) is caller-driven
via Tailwind classes; this component only supplies the animation and
the reduced-motion override. Always aria-hidden — screen-reader
announcement lives on the parent `<Skeleton>`.
-->
<template>
  <div aria-hidden="true" :class="['skeleton-bar', $attrs.class]" />
</template>

<script setup lang="ts">
defineOptions({ inheritAttrs: false });
</script>

<style scoped>
.skeleton-bar {
  background: var(--color-surface-hover);
  border-radius: 0.25rem;
  animation: skeleton-pulse 1.4s ease-in-out infinite;
}

@keyframes skeleton-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.55; }
}

@media (prefers-reduced-motion: reduce) {
  .skeleton-bar {
    animation: none;
    opacity: 0.7;
  }
}
</style>
