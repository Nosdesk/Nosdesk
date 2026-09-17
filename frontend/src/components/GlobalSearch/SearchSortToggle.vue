<script setup lang="ts">
import { useFluent } from 'fluent-vue';
import { ToggleGroupItem, ToggleGroupRoot } from 'reka-ui';
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
// the default, updated is newest-first. A Reka ToggleGroup: one tab stop,
// Left/Right walk the pair, each is an `aria-pressed` button.
const options: { value: SearchSortOrder; labelKey: string }[] = [
  { value: 'relevance', labelKey: 'search-global-sort-relevance' },
  { value: 'updated', labelKey: 'search-global-sort-updated' },
];

// A picked option stays picked: an empty model is the toggle-off Reka
// reports when the pressed one is clicked again.
function onUpdate(value: unknown) {
  if (value === 'relevance' || value === 'updated') emit('update:modelValue', value);
}
</script>

<template>
  <!-- Enter stays here: the palette's window-level handler would read
       it as "open the selected result". -->
  <ToggleGroupRoot
    type="single"
    :model-value="modelValue"
    class="inline-flex items-center rounded-md bg-surface-alt p-0.5 gap-0.5"
    :aria-label="t('search-global-sort-label')"
    @update:model-value="onUpdate"
    @keydown.enter.stop
  >
    <ToggleGroupItem
      v-for="opt in options"
      :key="opt.value"
      :value="opt.value"
      :data-sort-active="opt.value === modelValue"
      :class="[
        'px-2 h-5 rounded text-2xs font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent',
        opt.value === modelValue
          ? 'bg-surface text-primary shadow-sm'
          : 'text-tertiary hover:text-secondary',
      ]"
    >
      {{ t(opt.labelKey) }}
    </ToggleGroupItem>
  </ToggleGroupRoot>
</template>
