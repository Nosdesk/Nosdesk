<!--
Skeleton mirroring `DocumentationCard` / `DocumentationCardGrid`:
icon banner + title + description block + metadata strip. Grid
breakpoints MUST match `DocumentationCardGrid` so the skeleton doesn't
reflow at a different breakpoint than the real cards.
-->
<template>
  <Skeleton label="Loading pages" class="skeleton-grid">
    <div
      v-for="i in count"
      :key="i"
      class="skeleton-card"
    >
      <div class="skeleton-header">
        <SkeletonBar class="w-10 h-10 rounded-lg" />
      </div>
      <div class="skeleton-content">
        <SkeletonBar class="h-5 w-[70%] rounded" />
        <div class="flex flex-col gap-2">
          <SkeletonBar class="h-3 w-full" />
          <SkeletonBar class="h-3 w-[90%]" />
          <SkeletonBar class="h-3 w-[60%]" />
        </div>
        <div class="skeleton-meta">
          <SkeletonBar class="w-5 h-5 rounded-full" />
          <SkeletonBar class="h-2.5 w-14" />
          <SkeletonBar class="w-1 h-1 rounded-full" />
          <SkeletonBar class="h-2.5 w-10" />
        </div>
      </div>
    </div>
  </Skeleton>
</template>

<script setup lang="ts">
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';

withDefaults(defineProps<{ count?: number }>(), { count: 6 });
</script>

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
}

.skeleton-content {
  padding: 1rem 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.skeleton-meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-top: 0.5rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--color-subtle);
}
</style>
