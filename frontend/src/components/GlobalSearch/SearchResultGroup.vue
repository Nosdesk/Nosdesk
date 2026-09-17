<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import type { SearchResult, SearchEntityType } from '@nosdesk/core/types/search';
import { getEntityTypeLabel } from '@nosdesk/core/types/search';
import SearchResultItem from './SearchResultItem.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  type: SearchEntityType;
  results: SearchResult[];
  selectedId: string | null;
  idPrefix?: string;
}>();

const emit = defineEmits<{
  select: [result: SearchResult];
  scope: [type: SearchEntityType];
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
  <div v-if="results.length > 0" class="py-1 px-1" role="group" :aria-label="groupLabel">
    <!-- Group header. Pure typography, no icon: the result rows carry
         the type-coloured icon. Clicking it scopes the palette to this
         kind (filtering to something you can already see is the most
         natural filter gesture), a pointer affordance only: keyboard
         users scope via the prompt rows or `in:`, and a control inside
         a listbox group is not allowed, so it is hidden from AT and
         the group's own label carries the name. -->
    <div
      aria-hidden="true"
      :title="t('search-global-group-scope-title', { type: groupLabel })"
      class="group/header flex w-full items-baseline gap-2 px-2 pt-2 pb-1 text-left rounded-md transition-colors hover:bg-surface-hover/60 cursor-pointer select-none"
      @click="emit('scope', props.type)"
    >
      <span class="text-3xs font-semibold uppercase tracking-wider text-tertiary">
        {{ groupLabel }}
      </span>
      <span class="text-3xs text-tertiary/60 tabular-nums">
        {{ results.length }}
      </span>
      <span
        class="ml-auto text-3xs text-tertiary/60 opacity-0 group-hover/header:opacity-100 transition-opacity"
      >
        {{ t('search-global-group-scope-hint') }}
      </span>
    </div>

    <SearchResultItem
      v-for="result in results"
      :key="result.id"
      :result="result"
      :is-selected="result.id === selectedId"
      :id-prefix="idPrefix"
      @select="emit('select', $event)"
    />
  </div>
</template>
