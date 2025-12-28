<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import type { Page } from '@/services/documentationService'

const props = defineProps<{
  page: Page
  staggerIndex?: number
}>()

// Stagger animation style
const staggerStyle = computed(() => ({
  '--stagger-delay': `${(props.staggerIndex ?? 0) * 40}ms`
}))

// Truncate description to ~50 chars
const truncatedDescription = computed(() => {
  if (!props.page.description) return null
  return props.page.description.length > 50
    ? props.page.description.slice(0, 50) + '...'
    : props.page.description
})
</script>

<template>
  <RouterLink
    :to="`/documentation/${page.id}`"
    class="doc-child-card group"
    :style="staggerStyle"
  >
    <!-- Icon -->
    <span class="child-icon">{{ page.icon || '📄' }}</span>

    <!-- Content -->
    <div class="child-content">
      <span class="child-title">{{ page.title }}</span>
      <span v-if="truncatedDescription" class="child-description">
        {{ truncatedDescription }}
      </span>
    </div>

    <!-- Arrow -->
    <svg
      class="child-arrow"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <path d="M9 18l6-6-6-6" />
    </svg>
  </RouterLink>
</template>

<style scoped>
.doc-child-card {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.625rem 0.75rem;
  border-radius: 0.5rem;
  background: var(--color-surface);
  border: 1px solid var(--color-subtle);
  text-decoration: none;
  transition: all 150ms ease;

  /* Stagger animation */
  animation: childSlideIn 200ms ease-out forwards;
  animation-delay: var(--stagger-delay, 0ms);
  opacity: 0;
}

.doc-child-card:hover {
  background: var(--color-surface-hover);
  border-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
  transform: translateX(4px);
}

.doc-child-card:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 1px;
}

.child-icon {
  font-size: 1.125rem;
  flex-shrink: 0;
  line-height: 1;
}

.child-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
}

.child-title {
  font-size: 0.8125rem;
  font-weight: 500;
  color: var(--color-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition: color 150ms ease;
}

.doc-child-card:hover .child-title {
  color: var(--color-accent);
}

.child-description {
  font-size: 0.6875rem;
  color: var(--color-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.child-arrow {
  flex-shrink: 0;
  width: 1rem;
  height: 1rem;
  color: var(--color-tertiary);
  opacity: 0;
  transform: translateX(-4px);
  transition: all 150ms ease;
}

.doc-child-card:hover .child-arrow {
  opacity: 1;
  transform: translateX(0);
}

@keyframes childSlideIn {
  from {
    opacity: 0;
    transform: translateX(-8px);
  }
  to {
    opacity: 1;
    transform: translateX(0);
  }
}

/* Theme-specific styles */
[data-theme="red-horizon"] .doc-child-card:hover {
  border-color: rgba(255, 100, 50, 0.4);
  box-shadow: 0 0 8px rgba(200, 80, 0, 0.15);
}

[data-theme="epaper"] .doc-child-card {
  border-radius: 0.25rem;
}

[data-theme="epaper"] .doc-child-card:hover {
  transform: none;
}

/* Reduced motion support */
@media (prefers-reduced-motion: reduce) {
  .doc-child-card {
    animation: none;
    opacity: 1;
  }

  .doc-child-card:hover {
    transform: none;
  }

  .child-arrow {
    transition: none;
  }
}
</style>
