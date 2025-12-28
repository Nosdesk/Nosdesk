<script setup lang="ts">
import type { Page } from '@/services/documentationService'
import DocumentationCard from './DocumentationCard.vue'
import EmptyState from '@/components/common/EmptyState.vue'

defineProps<{
  pages: Page[]
}>()

const emit = defineEmits<{
  create: []
}>()
</script>

<template>
  <div class="doc-card-grid">
    <!-- Main Grid -->
    <TransitionGroup
      v-if="pages.length > 0"
      name="card-stagger"
      tag="div"
      class="grid-container"
    >
      <DocumentationCard
        v-for="(page, index) in pages"
        :key="page.id"
        :page="page"
        :stagger-index="index"
      />
    </TransitionGroup>

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

/* Stagger transition group */
.card-stagger-move {
  transition: transform 300ms ease;
}

.card-stagger-enter-active {
  transition: all 300ms ease-out;
}

.card-stagger-leave-active {
  transition: all 200ms ease-in;
  position: absolute;
}

.card-stagger-enter-from {
  opacity: 0;
  transform: translateY(16px) scale(0.95);
}

.card-stagger-leave-to {
  opacity: 0;
  transform: scale(0.95);
}
</style>
