<script setup lang="ts">
import type { Page } from '@/services/documentationService'
import DocumentationCard from './DocumentationCard.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import { useStaggeredList } from '@/composables/useStaggeredList'

defineProps<{
  pages: Page[]
}>()

const emit = defineEmits<{
  create: []
}>()

// Staggered animation for cards
const { getStyle } = useStaggeredList({
  staggerDelay: 40,
  maxStaggerItems: 12
})
</script>

<template>
  <div class="doc-card-grid">
    <!-- Main Grid -->
    <div
      v-if="pages.length > 0"
      class="grid-container"
    >
      <DocumentationCard
        v-for="(page, index) in pages"
        :key="page.id"
        :page="page"
        :style="getStyle(index)"
        class="card-animate"
      />
    </div>

    <!-- Empty State -->
    <EmptyState
      v-else
      icon="document"
      title="No documentation yet"
      description="Create your first documentation page to get started."
      action-label="Create Page"
      variant="card"
      @action="emit('create')"
    />
  </div>
</template>

<style scoped>
.doc-card-grid {
  width: 100%;
}

/* Responsive grid container */
.grid-container {
  display: grid;
  gap: 1.5rem;

  /* Mobile: single column */
  grid-template-columns: 1fr;
}

/* Tablet: 2 columns */
@media (min-width: 640px) {
  .grid-container {
    grid-template-columns: repeat(2, 1fr);
    gap: 1.25rem;
  }
}

/* Desktop: 3 columns */
@media (min-width: 1024px) {
  .grid-container {
    grid-template-columns: repeat(3, 1fr);
    gap: 1.5rem;
  }
}

/* Large desktop: auto-fill */
@media (min-width: 1400px) {
  .grid-container {
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  }
}

/* Staggered card animation */
.card-animate {
  animation: cardFadeIn var(--animation-duration, 150ms) ease-out forwards;
  animation-delay: var(--stagger-delay, 0ms);
  opacity: 0;
  transform: translateY(8px);
}

@keyframes cardFadeIn {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* Respect reduced motion preferences */
@media (prefers-reduced-motion: reduce) {
  .card-animate {
    animation: none;
    opacity: 1;
    transform: none;
  }
}
</style>
