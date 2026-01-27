<script setup lang="ts">
import { computed } from 'vue';
import type { SearchResult, SearchEntityType } from '@/types/search';
import { getEntityTypeLabel, ENTITY_TYPE_CONFIG } from '@/types/search';
import SearchResultItem from './SearchResultItem.vue';

const props = defineProps<{
  type: SearchEntityType;
  results: SearchResult[];
  selectedId: string | null;
}>();

const emit = defineEmits<{
  select: [result: SearchResult];
}>();

const headerIcon = computed(() => {
  return ENTITY_TYPE_CONFIG[props.type]?.icon ?? 'M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z';
});
</script>

<template>
  <div v-if="results.length > 0" class="py-1.5">
    <!-- Group header -->
    <div class="flex items-center gap-2 px-3 py-2 mb-0.5">
      <svg
        class="w-3.5 h-3.5 text-tertiary"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="2"
      >
        <path stroke-linecap="round" stroke-linejoin="round" :d="headerIcon" />
      </svg>
      <span class="text-[11px] font-semibold text-tertiary uppercase tracking-wider">
        {{ getEntityTypeLabel(type) }}
      </span>
      <span class="text-[10px] text-tertiary/70 font-medium">
        {{ results.length }}
      </span>
    </div>

    <!-- Results -->
    <div class="space-y-px">
      <SearchResultItem
        v-for="result in results"
        :key="result.id"
        :result="result"
        :is-selected="result.id === selectedId"
        @select="emit('select', $event)"
      />
    </div>
  </div>
</template>
