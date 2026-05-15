<!--
Skeleton for the flat list rows rendered by Archived / Trash views.
Mirrors the real row layout: icon + title/date stack + action buttons,
same height so resolving the fetch doesn't shift the layout down.
-->
<template>
  <Skeleton :label="resolvedLabel" class="flex flex-col gap-2">
    <div
      v-for="i in count"
      :key="i"
      class="flex items-center gap-3 px-4 py-3 bg-surface border border-default rounded-lg"
    >
      <SkeletonBar class="h-6 w-6 rounded flex-shrink-0" />
      <div class="flex-1 min-w-0 flex flex-col gap-2">
        <SkeletonBar class="h-3.5 w-56 max-w-full" />
        <SkeletonBar class="h-3 w-32 max-w-full" />
      </div>
      <div class="flex items-center gap-2 flex-shrink-0">
        <SkeletonBar v-for="j in actionsPerRow" :key="j" class="h-7 w-20 rounded-md" />
      </div>
    </div>
  </Skeleton>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';

const fluent = useFluent();

const props = withDefaults(
  defineProps<{
    count?: number;
    actionsPerRow?: number;
    label?: string;
  }>(),
  {
    count: 3,
    actionsPerRow: 1,
    label: undefined,
  },
);

// Default label is localised; an explicit prop wins so callers
// can override with feature-specific copy.
const resolvedLabel = computed(() => props.label ?? fluent.$t('docs-row-skeleton-label'));
</script>
