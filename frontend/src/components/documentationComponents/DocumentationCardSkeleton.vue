<script setup lang="ts">
defineProps<{
  count?: number
}>()
</script>

<template>
  <div class="skeleton-grid">
    <div
      v-for="i in (count || 6)"
      :key="`skeleton-${i}`"
      class="skeleton-card"
    >
      <!-- Icon area skeleton -->
      <div class="skeleton-header">
        <div class="skeleton-icon"></div>
      </div>

      <!-- Content skeleton -->
      <div class="skeleton-content">
        <!-- Title -->
        <div class="skeleton-title"></div>

        <!-- Description lines -->
        <div class="skeleton-description">
          <div class="skeleton-line skeleton-line--full"></div>
          <div class="skeleton-line skeleton-line--long"></div>
          <div class="skeleton-line skeleton-line--medium"></div>
        </div>

        <!-- Metadata -->
        <div class="skeleton-meta">
          <div class="skeleton-avatar"></div>
          <div class="skeleton-meta-text"></div>
          <div class="skeleton-meta-dot"></div>
          <div class="skeleton-meta-text skeleton-meta-text--short"></div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.skeleton-grid {
  display: grid;
  gap: 1.5rem;
  grid-template-columns: 1fr;
}

@media (min-width: 640px) {
  .skeleton-grid {
    grid-template-columns: repeat(2, 1fr);
    gap: 1.25rem;
  }
}

@media (min-width: 1024px) {
  .skeleton-grid {
    grid-template-columns: repeat(3, 1fr);
    gap: 1.5rem;
  }
}

@media (min-width: 1400px) {
  .skeleton-grid {
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  }
}

.skeleton-card {
  background: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 1rem;
  overflow: hidden;
}

.skeleton-header {
  height: 5rem;
  background: var(--color-surface-alt);
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
}

.skeleton-icon {
  width: 2.5rem;
  height: 2.5rem;
  border-radius: 0.5rem;
  background: var(--color-surface-hover);
  animation: shimmer 1.5s ease-in-out infinite;
}

.skeleton-content {
  padding: 1rem 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.skeleton-title {
  height: 1.25rem;
  width: 70%;
  border-radius: 0.25rem;
  background: var(--color-surface-hover);
  animation: shimmer 1.5s ease-in-out infinite;
  animation-delay: 150ms;
}

.skeleton-description {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.skeleton-line {
  height: 0.75rem;
  border-radius: 0.25rem;
  background: var(--color-surface-hover);
  animation: shimmer 1.5s ease-in-out infinite;
}

.skeleton-line--full {
  width: 100%;
  animation-delay: 200ms;
}

.skeleton-line--long {
  width: 90%;
  animation-delay: 250ms;
}

.skeleton-line--medium {
  width: 60%;
  animation-delay: 300ms;
}

.skeleton-meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-top: 0.5rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--color-subtle);
}

.skeleton-avatar {
  width: 1.25rem;
  height: 1.25rem;
  border-radius: 50%;
  background: var(--color-surface-hover);
  animation: shimmer 1.5s ease-in-out infinite;
  animation-delay: 350ms;
}

.skeleton-meta-text {
  height: 0.625rem;
  width: 3.5rem;
  border-radius: 0.125rem;
  background: var(--color-surface-hover);
  animation: shimmer 1.5s ease-in-out infinite;
  animation-delay: 400ms;
}

.skeleton-meta-text--short {
  width: 2.5rem;
  animation-delay: 450ms;
}

.skeleton-meta-dot {
  width: 0.25rem;
  height: 0.25rem;
  border-radius: 50%;
  background: var(--color-surface-hover);
  animation: shimmer 1.5s ease-in-out infinite;
  animation-delay: 425ms;
}

@keyframes shimmer {
  0%, 100% {
    opacity: 1;
  }
  50% {
    opacity: 0.5;
  }
}
</style>
