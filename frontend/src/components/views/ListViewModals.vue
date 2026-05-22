<script setup lang="ts" generic="T extends object, C extends DataTableColumnLike, S extends BaseListViewShape = BaseListViewShape">
/**
 * Saved-view modals for `useListView`-driven list views. Mounts
 * the SaveViewModal (name prompt for "Save current view as")
 * and the SavedViewEditorModal (rename + delete). Consumers
 * drop this once and let it handle the round-trip; view-
 * specific modals (bulk delete confirms, role pickers, etc.)
 * stay where they are.
 *
 * Generic over the same type params as `useListView` so the
 * typed bundle is accepted verbatim.
 */
import SaveViewModal from '@/components/views/SaveViewModal.vue'
import SavedViewEditorModal from '@/components/views/SavedViewEditorModal.vue'
import type { UseListView, BaseListViewShape } from '@/composables/useListView'
import type { DataTableColumnLike } from '@/composables/useDataTableColumns'

defineProps<{
  listView: UseListView<T, C, S>
}>()
</script>

<template>
  <SaveViewModal
    :show="listView.showSaveModal.value"
    :default-name="listView.defaultSaveName.value"
    @save="listView.handleSaveAs"
    @close="listView.closeSaveModal"
  />
  <SavedViewEditorModal
    :view="listView.editingView.value"
    @rename="listView.handleRename"
    @delete="listView.handleDelete"
    @close="listView.closeEditor"
  />
</template>
