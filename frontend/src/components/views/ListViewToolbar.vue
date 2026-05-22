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
import ViewSwitcher from '@/components/views/ViewSwitcher.vue'
import ChipFilterStrip from '@/components/views/ChipFilterStrip.vue'
import GroupByMenu from '@/components/views/GroupByMenu.vue'
import ColumnPickerMenu from '@/components/views/ColumnPickerMenu.vue'
import type { UseListView, BaseListViewShape } from '@/composables/useListView'
import type { DataTableColumnLike } from '@/composables/useDataTableColumns'

defineProps<{
  listView: UseListView<T, C, S>
  /** Switcher placeholder when the active view is null. */
  switcherPlaceholder?: string
}>()

const emit = defineEmits<{
  (e: 'open-editor', uuid: string): void
  (e: 'save-as'): void
}>()
</script>

<template>
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
    class="inline-flex items-center text-[11px] px-2 h-6 rounded-md border border-dashed border-subtle text-tertiary hover:text-primary hover:border-default hover:bg-surface-hover transition-colors"
    :title="$t('views-save-trigger')"
    @click="emit('save-as')"
  >
    {{ $t('views-save-trigger') }}
  </button>
  <slot name="append" />
</template>
