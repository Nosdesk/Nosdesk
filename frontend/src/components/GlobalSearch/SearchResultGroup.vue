<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import type { SearchResult, SearchEntityType } from '@/types/search';
import { getEntityTypeLabel } from '@/types/search';
import SearchResultItem from './SearchResultItem.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  type: SearchEntityType;
  results: SearchResult[];
  selectedId: string | null;
}>();

const emit = defineEmits<{
  select: [result: SearchResult];
}>();

const groupLabel = computed(() => {
  const key = `search-result-group-${props.type}`;
  const localized = t(key);
  // fluent-vue returns the key unchanged when missing; fall back to the
  // built-in English label in that case so we don't render raw keys.
  return localized === key ? getEntityTypeLabel(props.type) : localized;
});
</script>

<template>
  <div v-if="results.length > 0" class="py-1 px-1">
    <!-- Group header. Pure typography — no icon. The result rows
         themselves carry the type-coloured icon, so a header glyph
         on top of that is redundant noise. Raycast lets the label
         alone do the work. -->
    <div class="flex items-baseline gap-2 px-2 pt-2 pb-1">
      <span class="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
        {{ groupLabel }}
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
