<!--
Filter dropdown row for list pages. Renders a row of `BaseDropdown`
filter pickers (one per filter the view declares) plus an
optional Reset button.

Per-view content stays in the consumer (the consumer builds the
options array via `controls.buildFilterOptions(...)`); this
component just collapses the markup that every list view was
otherwise repeating around a `v-for` loop. Slots into the
`#filters` slot of `ListPageLayout`.

Kept separate from `ListPageLayout` (rather than folded into a
config prop) so views with non-dropdown filters (date pickers,
custom widgets) can keep using the layout's slot directly without
forcing every other view through a generic config object.
-->
<script setup lang="ts">
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import type { BuiltFilterOption, FilterValue } from '@/composables/useListControls'

defineProps<{
  /** Options to render. Build with
   *  `controls.buildFilterOptions({...})`. */
  options: BuiltFilterOption[]
  /** Show a "Reset" button that emits `reset`. Defaults to `true`
   *  when at least one filter is rendered. */
  showReset?: boolean
}>()

const emit = defineEmits<{
  update: [name: string, value: FilterValue]
  reset: []
}>()
</script>

<template>
  <template v-if="options.length > 0">
    <div
      v-for="filter in options"
      :key="filter.name"
      :class="[filter.width || 'w-[120px]']"
    >
      <BaseDropdown
        :model-value="filter.value"
        :options="filter.options"
        :multiple="filter.multiple"
        :placeholder="filter.placeholder"
        size="sm"
        @update:model-value="(value: FilterValue) => emit('update', filter.name, value)"
      />
    </div>
    <button
      v-if="showReset !== false"
      type="button"
      class="px-2 py-1 text-xs font-medium text-on-accent bg-accent rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-accent"
      @click="emit('reset')"
    >
      Reset
    </button>
  </template>
</template>
