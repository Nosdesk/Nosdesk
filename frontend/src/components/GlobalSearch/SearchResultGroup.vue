<script setup lang="ts">
import type { SearchResult, SearchEntityType } from '@/types/search';
import { getEntityTypeLabel } from '@/types/search';
import SearchResultItem from './SearchResultItem.vue';

defineProps<{
  type: SearchEntityType;
  results: SearchResult[];
  selectedId: string | null;
}>();

const emit = defineEmits<{
  select: [result: SearchResult];
}>();
</script>

<template>
  <div v-if="results.length > 0" class="py-1 px-1">
    <!-- Group header. Pure typography — no icon. The result rows
         themselves carry the type-coloured icon, so a header glyph
         on top of that is redundant noise. Raycast lets the label
         alone do the work. -->
    <div class="flex items-baseline gap-2 px-2 pt-2 pb-1">
      <span class="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
        {{ getEntityTypeLabel(type) }}
      </span>
      <span class="text-[10px] text-tertiary/60 tabular-nums">
        {{ results.length }}
      </span>
    </div>

    <SearchResultItem
      v-for="result in results"
      :key="result.id"
      :result="result"
      :is-selected="result.id === selectedId"
      @select="emit('select', $event)"
    />
  </div>
</template>
