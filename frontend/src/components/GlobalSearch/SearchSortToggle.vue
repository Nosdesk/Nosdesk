<script setup lang="ts">
import { useFluent } from 'fluent-vue';
import type { SearchSortOrder } from '@nosdesk/core/types/search';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

defineProps<{
  modelValue: SearchSortOrder;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: SearchSortOrder];
}>();

// Two options only, by design (see search-ux-plan Phase 3): relevance is
// the default, updated is newest-first. tabindex=-1 so the toggle never
// steals the roving focus from the search input — it's pointer-driven.
const options: { value: SearchSortOrder; labelKey: string }[] = [
  { value: 'relevance', labelKey: 'search-global-sort-relevance' },
  { value: 'updated', labelKey: 'search-global-sort-updated' },
];
</script>

<template>
  <div
    class="inline-flex items-center rounded-md bg-surface-alt p-0.5 gap-0.5"
    role="group"
    :aria-label="t('search-global-sort-label')"
  >
    <button
      v-for="opt in options"
      :key="opt.value"
      type="button"
      tabindex="-1"
      :data-sort-active="opt.value === modelValue"
      :aria-pressed="opt.value === modelValue"
      :class="[
        'px-2 h-5 rounded text-2xs font-medium transition-colors',
        opt.value === modelValue
          ? 'bg-surface text-primary shadow-sm'
          : 'text-tertiary hover:text-secondary',
      ]"
      @click="emit('update:modelValue', opt.value)"
    >
      {{ t(opt.labelKey) }}
    </button>
  </div>
</template>
