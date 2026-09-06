<script setup lang="ts" generic="T extends object, C extends DataTableColumnLike, S extends BaseListViewShape = BaseListViewShape">
/**
 * Shared toolbar for `useListView`-driven list views. Renders
 * the saved-view switcher, chip filter strip, group-by picker,
 * column picker, and "Save view as" trigger in a consistent
 * order. Consumers pass the `useListView` bundle in once;
 * everything wires through.
 *
 * Slots `prepend` and `append` are pass-throughs for view-
 * specific toolbar additions (eg. a per-view density toggle
 * once that lands on this surface).
 *
 * Generic over the item type T, the column shape C, and the
 * saved-view shape S so the typed bundle from useListView is
 * accepted verbatim without variance casts.
 */
import { computed, ref } from 'vue'
import ViewSwitcher from '@/components/views/ViewSwitcher.vue'
import ChipFilterStrip from '@/components/views/ChipFilterStrip.vue'
import GroupByMenu from '@/components/views/GroupByMenu.vue'
import ColumnPickerMenu from '@/components/views/ColumnPickerMenu.vue'
import MobileFilterGroupSheet from '@/components/views/MobileFilterGroupSheet.vue'
import Icon from '@/components/common/Icon.vue'
import { NONE_AXIS_KEY } from '@/composables/useListGrouping'
import type { UseListView, BaseListViewShape } from '@/composables/useListView'
import type { DataTableColumnLike } from '@/composables/useDataTableColumns'

const props = defineProps<{
  listView: UseListView<T, C, S>
  /** Switcher placeholder when the active view is null. */
  switcherPlaceholder?: string
}>()

const emit = defineEmits<{
  (e: 'open-editor', uuid: string): void
  (e: 'save-as'): void
}>()

// Mobile collapses the whole toolbar into one sheet trigger. The badge
// counts active filters plus an active group-by lens.
const sheetOpen = ref(false)
const activeCount = computed(() => {
  const filters = props.listView.chipFilters.pills.value.length
  const grouped = props.listView.grouping.groupBy.value !== NONE_AXIS_KEY ? 1 : 0
  return filters + grouped
})
</script>

<template>
  <!-- Desktop: the full control row. `sm:contents` lets the children
       lay out directly in the toolbar's flex-wrap; hidden on mobile. -->
  <div class="hidden sm:contents">
    <ViewSwitcher
      v-if="listView.savedViews.switcherItems.value.length > 0"
      :items="listView.savedViews.switcherItems.value"
      :active-id="listView.savedViews.activeViewId.value ?? ''"
      size="sm"
      :placeholder="switcherPlaceholder"
      @select="listView.savedViews.switchTo"
      @edit="(uuid: string) => emit('open-editor', uuid)"
    />
    <slot name="prepend" />
    <ChipFilterStrip
      :pills="listView.chipFilters.pills.value"
      :add-filter-facets="listView.chipFilters.addFilterFacets.value"
      :active-facets="listView.chipFilters.activeFacets.value"
      :options-for="listView.chipFilters.optionsFor"
      :selected-for="listView.chipFilters.selectedFor"
      :text-value-for="listView.chipFilters.textValueFor"
      :on-toggle="listView.chipFilters.toggleValue"
      :on-clear="listView.chipFilters.clearFacet"
      :on-set-text="listView.chipFilters.setText"
    />
    <GroupByMenu
      :options="listView.grouping.axisOptions.value"
      :model-value="listView.grouping.groupBy.value"
      @update:model-value="listView.grouping.setGroupBy"
    />
    <ColumnPickerMenu
      :columns="listView.tableColumns.ordered.value"
      :is-hidden="listView.tableColumns.isHidden"
      :is-pinned="listView.tableColumns.isPinned"
      @toggle="listView.tableColumns.toggleVisible"
      @reset="listView.tableColumns.reset"
    />
    <button
      type="button"
      class="inline-flex items-center text-2xs px-2 h-6 rounded-md border border-dashed border-subtle text-tertiary hover:text-primary hover:border-default hover:bg-surface-hover transition-colors"
      :title="$t('views-save-trigger')"
      @click="emit('save-as')"
    >
      {{ $t('views-save-trigger') }}
    </button>
    <slot name="append" />
  </div>

  <!-- Mobile: one trigger that opens the filter/group sheet. -->
  <button
    type="button"
    class="sm:hidden inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-default text-sm font-medium text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
    @click="sheetOpen = true"
  >
    <Icon name="settings" size="sm" />
    {{ $t('list-mobile-filter-group-title') }}
    <span
      v-if="activeCount > 0"
      class="inline-flex items-center justify-center min-w-[18px] h-[18px] px-1 rounded-full bg-accent text-on-accent text-2xs font-semibold tabular-nums"
    >
      {{ activeCount }}
    </span>
  </button>
  <MobileFilterGroupSheet :show="sheetOpen" :list-view="listView" @close="sheetOpen = false" />
</template>
