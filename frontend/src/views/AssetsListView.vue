<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useMutation, useQueryCache } from '@pinia/colada'
import { extractErrorMessage } from '@/utils/errors'
import { useToastStore } from '@/stores/toast'

import DataTable from '@/components/common/DataTable.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import ChipFilterStrip from '@/components/views/ChipFilterStrip.vue'
import GroupByMenu from '@/components/views/GroupByMenu.vue'
import ViewSwitcher from '@/components/views/ViewSwitcher.vue'
import SavedViewEditorModal from '@/components/views/SavedViewEditorModal.vue'
import SaveViewModal from '@/components/views/SaveViewModal.vue'
import {
  useChipFiltersFromControls,
  type ChipFacetDef,
} from '@/composables/useChipFiltersFromControls'
import {
  useListGrouping,
  type GroupAxisDef,
} from '@/composables/useListGrouping'
import { useSavedListViews } from '@/composables/useSavedListViews'
import type { SavedView } from '@/services/savedViewsService'
import type { Filters } from '@/composables/useListControls'
import { useAuthStore } from '@/stores/auth'

import { TextCell, StatusBadgeCell, UserAvatarCell } from '@/components/common/cells'
import AssetViewTabs from '@/components/assets/AssetViewTabs.vue'
import { useListControls } from '@/composables/useListControls'
import { useListPage } from '@/composables/useListPage'
import { useBulkSelection } from '@/composables/useBulkSelection'
import { useBulkSelectionForDataTable } from '@/composables/useBulkSelectionForDataTable'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { getPaginatedDevices, bulkAction } from '@/services/assetService'
import { devicesKeys } from '@/queries/assets'
import type { Asset } from '@/types/asset'

defineOptions({ name: 'DevicesListView' })

const router = useRouter()
const queryCache = useQueryCache()
const { isMobile } = useMobileDetection()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toast = useToastStore()

// `useTemplateRef` (Vue 3.5+) typed against the layout's exported
// expose interface. TypeScript can't `InstanceType<>` a generic
// component, so the layout exports `ListPageLayoutExpose` as the
// public typed shape of its `defineExpose` payload.
const layoutRef = useTemplateRef<ListPageLayoutExpose>('layout')
const scrollContainerRef = computed<HTMLElement | null>(
  () => layoutRef.value?.scrollContainerRef ?? null,
)

const navigateToCreateDevice = () => {
  void router.push('/assets/new')
}
const navigateToDevice = (device: Asset) => {
  void router.push(`/assets/${device.id}`)
}

const controls = useListControls<Asset>({
  itemIdField: 'id',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  defaultPageSize: 0,
})

const page = useListPage({
  controls,
  keys: devicesKeys,
  fetchPage: (params) => getPaginatedDevices(params, `devices-page-${params.page}`),
  scrollContainerRef,
  sseEvents: ['asset-updated', 'asset-created', 'asset-deleted'],
  mobileSearch: {
    placeholder: t('assets-list-search-placeholder'),
    createIcon: 'device',
    onCreate: navigateToCreateDevice,
  },
  urlSync: { paramKeys: ['warranty', 'lowStock'] },
  // No per-page user prewarm: primary_user uuids are already in
  // the sync engine's user pool (workspace:1 bootstrap), so the
  // avatar cells resolve from there without an extra round trip.
})

usePageCreateAction(navigateToCreateDevice)

const selection = useBulkSelection<Asset>({
  items: page.items,
  cacheKey: controls.cacheKeyPart,
  totalCount: page.totalItems,
})
const dt = useBulkSelectionForDataTable(selection)

// Filter facets (chip UI). Backend encoding:
//   warranty  -> CSV under one filter key, backend ANY-matches
//   lowStock  -> single 'true' when on, absent when off
// State lives in `controls.filters` so URL sync + backend query
// + saved views all see the same source of truth.
const assetFacets = computed<ChipFacetDef[]>(() => [
  {
    key: 'name',
    labelKey: 'assets-list-filter-name-label',
    kind: 'text',
    searchInput: true,
    options: () => [],
  },
  {
    key: 'warranty',
    labelKey: 'assets-list-filter-warranty-label',
    kind: 'multi',
    options: () => [
      { value: 'Active', label: t('assets-list-filter-warranty-active'), swatchClass: 'bg-emerald-500' },
      { value: 'Warning', label: t('assets-list-filter-warranty-warning'), swatchClass: 'bg-amber-500' },
      { value: 'Expired', label: t('assets-list-filter-warranty-expired'), swatchClass: 'bg-rose-500' },
      { value: 'Unknown', label: t('assets-list-filter-warranty-unknown'), swatchClass: 'bg-zinc-400' },
    ],
  },
  {
    key: 'lowStock',
    labelKey: 'assets-list-filter-low-stock-label',
    kind: 'multi',
    options: () => [
      { value: 'true', label: t('assets-list-filter-low-stock-on'), swatchClass: 'bg-amber-500' },
    ],
  },
])

const chipFilters = useChipFiltersFromControls({
  controls,
  facets: assetFacets,
  t,
})

// ---------------------------------------------------------------
// Group-by. Client-side bucketing of the currently-loaded page,
// so grouping is most meaningful in infinite-scroll mode (default
// pageSize=0 → backend returns up to 50 rows in one shot). The
// axes pick from columns that the table already shows, so the
// bucket label and the grouped column carry the same information.
// ---------------------------------------------------------------
const WARRANTY_ORDER = ['Expired', 'Warning', 'Active', 'Unknown'] as const

const groupAxes: GroupAxisDef<Asset>[] = [
  {
    key: 'warranty',
    labelKey: 'assets-list-grouping-warranty',
    bucketFor: (asset) => {
      const raw = (asset.attributes?.warranty_status as string | undefined) ?? ''
      const key = raw || 'unknown'
      const label = raw || t('assets-list-filter-warranty-unknown')
      return { key: `warranty:${key}`, label }
    },
    sortBy: (bucketKey) => {
      const v = bucketKey.replace('warranty:', '') as (typeof WARRANTY_ORDER)[number]
      const idx = WARRANTY_ORDER.indexOf(v)
      return idx === -1 ? 999 : idx
    },
  },
  {
    key: 'kind',
    labelKey: 'assets-list-grouping-kind',
    bucketFor: (asset) => ({
      key: `kind:${asset.kind}`,
      label: asset.kind,
    }),
  },
  {
    key: 'manufacturer',
    labelKey: 'assets-list-grouping-manufacturer',
    bucketFor: (asset) => {
      const m = asset.manufacturer ?? ''
      return {
        key: `manufacturer:${m || '__none'}`,
        label: m || t('assets-list-grouping-manufacturer-none'),
      }
    },
  },
  {
    key: 'location',
    labelKey: 'assets-list-grouping-location',
    bucketFor: (asset) => {
      const l = asset.location ?? ''
      return {
        key: `location:${l || '__none'}`,
        label: l || t('assets-list-grouping-location-none'),
      }
    },
  },
  {
    key: 'primary_user',
    labelKey: 'assets-list-grouping-primary-user',
    bucketFor: (asset) => {
      const uuid = asset.primary_user?.uuid ?? '__unassigned'
      return {
        key: `user:${uuid}`,
        label: asset.primary_user?.name ?? t('assets-list-unassigned'),
      }
    },
  },
]

const grouping = useListGrouping<Asset>({
  axes: groupAxes,
  storageNamespace: 'assets',
  // No saved-views story yet for assets; everything lives under
  // the single 'default' scope. When saved views land, swap this
  // for `() => activeViewId.value`.
  getViewId: () => 'default',
  t,
})

const itemsRef = computed(() => page.items.value)
const buckets = grouping.buckets(itemsRef)

// ---------------------------------------------------------------
// Saved views. Each view persists the user's filter facets plus
// the display shape (group axis + sort). Search query stays out
// because it's an in-the-moment input, not durable state.
//
// Shape and filter blobs round-trip through the backend as
// opaque JSON; the schema is defined here so applyShape and
// applyFilter can rebuild controls + grouping from a row.
// ---------------------------------------------------------------
interface AssetViewShape {
  groupBy: string
  sortField: string
  sortDirection: 'asc' | 'desc'
}
type AssetViewFilter = Filters

const auth = useAuthStore()
const userUuid = computed<string | null>(() => auth.user?.uuid ?? null)

const savedViews = useSavedListViews<AssetViewShape, AssetViewFilter>({
  dataset: 'assets',
  userUuid,
  captureShape: () => ({
    groupBy: grouping.groupBy.value,
    sortField: controls.sortField.value,
    sortDirection: controls.sortDirection.value,
  }),
  captureFilter: () => ({ ...controls.filters.value }),
  applyShape: (shape) => {
    grouping.setGroupBy(shape.groupBy)
    controls.handleSortUpdate(shape.sortField, shape.sortDirection)
  },
  applyFilter: (filter) => {
    // Replace the whole filter map so axes the saved view didn't
    // touch get cleared. Search query (a separate ref) stays
    // alone so the user can search inside a saved view without
    // re-typing.
    controls.filters.value = { ...filter }
  },
  t,
})

const showSaveModal = ref(false)
const editingView = ref<SavedView<AssetViewShape, AssetViewFilter> | null>(null)

function openEditor(uuid: string): void {
  editingView.value = savedViews.views.value.find((v) => v.uuid === uuid) ?? null
}

async function handleSaveAs(name: string): Promise<boolean> {
  const created = await savedViews.saveAs(name)
  if (!created) {
    toast.error(t('views-save-as-error'))
    return false
  }
  toast.success(t('views-save-as-success', { name: created.name }))
  return true
}

async function handleRename(uuid: string, name: string): Promise<boolean> {
  const ok = await savedViews.rename(uuid, name)
  if (!ok) toast.error(t('views-saved-editor-rename-error'))
  return ok
}

async function handleDelete(uuid: string): Promise<boolean> {
  const ok = await savedViews.deleteView(uuid)
  if (!ok) toast.error(t('views-saved-editor-delete-error'))
  return ok
}

/** Default name pre-filled in the SaveViewModal: when the user
 *  is currently viewing a saved view, suggest "<name> (copy)";
 *  otherwise an empty string. Mirrors the tickets save-as flow. */
const defaultSaveName = computed<string>(() => {
  const active = savedViews.activeView.value
  if (!active) return ''
  return t('views-save-default-suffix') === '(copy)'
    ? `${active.name} (copy)`
    : `${active.name} ${t('views-save-default-suffix')}`
})

// Available sortable fields: id, name, hostname, serial_number,
// model, warranty_status, manufacturer, created_at, updated_at,
// last_sync_time.
const columns = computed(() => [
  { field: 'name', label: t('assets-list-column-device'), width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'serial_number', label: t('assets-list-column-serial'), width: 'minmax(140px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'hostname', label: t('assets-list-column-hostname'), width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'model', label: t('assets-list-column-model'), width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: t('assets-list-column-user'), width: 'minmax(140px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'quantity', label: t('assets-list-column-stock'), width: 'minmax(100px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'warranty_status', label: t('assets-list-column-warranty'), width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const },
])

const gridClass =
  'grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(140px,auto)_minmax(100px,auto)_minmax(100px,auto)] lg:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(120px,auto)_minmax(120px,auto)_minmax(140px,auto)_minmax(100px,auto)_minmax(100px,auto)]'

// Bulk delete: irreversible (devices aren't soft-deleted), so a
// confirm modal rather than the optimistic Undo-toast pattern.
const showDeleteConfirm = ref(false)
const bulkDelete = useMutation({
  mutation: (ids: number[]) => bulkAction({ action: 'delete', ids }),
  onSettled: () => queryCache.invalidateQueries({ key: devicesKeys.root }),
  onError: (err) => {
    console.error('Bulk delete failed:', err)
    toast.error(extractErrorMessage(err, t('assets-list-bulk-action-error')))
  },
})

/** Asset row is low-stock when both quantity and threshold are
 *  set and the on-hand count has fallen to at or below the
 *  threshold. parseFloat is fine for the comparison; both
 *  strings come from the same NUMERIC(12,3) column so any
 *  precision loss is symmetric. */
function isLowStock(asset: Asset): boolean {
  const q = asset.quantity
  const th = asset.low_stock_threshold
  if (q == null || th == null) return false
  return parseFloat(q) <= parseFloat(th)
}

async function confirmDelete() {
  showDeleteConfirm.value = false
  const ids = selection.selectedIds.value.map((id) => parseInt(id))
  if (ids.length === 0) return
  await bulkDelete.mutateAsync(ids)
  selection.clear()
}
</script>

<template>
  <!--
    Single root element. Multi-root (fragment) views break the
    `<Transition mode="out-in">` wrapping our RouterView in
    App.vue: Vue's transition system can't attach
    `.page-leave-active` to a fragment, the leave never finishes,
    and the next route never enters. The dialog teleports to body
    so its DOM placement here is purely organisational; the
    wrapper just gives Transition a real element to bind to.
  -->
  <div class="h-full">
  <ListPageLayout
    ref="layout"
    :items="page.items.value"
    :total-items="page.totalItems.value"
    :is-first-load="page.isFirstLoad.value"
    :is-background-refresh="page.isBackgroundRefresh.value"
    :is-loading-more="page.isLoadingMore.value"
    :error="page.errorMessage.value"
    :search-query="controls.searchQuery.value"
    :search-placeholder="$t('assets-list-search-placeholder')"
    :item-label="$t('assets-list-item-label')"
    bulk-selection-copy-key="bulk-bar-devices-selected"
    bulk-all-selected-copy-key="bulk-bar-devices-all-selected"
    :bulk-selection="selection"
    hide-desktop-search
    @update:search-query="controls.handleSearchUpdate"
    @retry="page.handleRetry"
  >
    <template #view-tabs>
      <AssetViewTabs />
    </template>

    <template #filters>
      <!-- Saved-view switcher. Hidden until the user has at least
           one saved view; until then "Save view as" is the only
           affordance and the empty switcher would read as dead
           chrome. -->
      <ViewSwitcher
        v-if="savedViews.switcherItems.value.length > 0"
        :items="savedViews.switcherItems.value"
        :active-id="savedViews.activeViewId.value ?? ''"
        size="sm"
        :placeholder="$t('views-asset-switcher-placeholder')"
        @select="savedViews.switchTo"
        @edit="openEditor"
      />
      <ChipFilterStrip
        :pills="chipFilters.pills.value"
        :add-filter-facets="chipFilters.addFilterFacets.value"
        :active-facets="chipFilters.activeFacets.value"
        :options-for="chipFilters.optionsFor"
        :selected-for="chipFilters.selectedFor"
        :text-value-for="chipFilters.textValueFor"
        :on-toggle="chipFilters.toggleValue"
        :on-clear="chipFilters.clearFacet"
        :on-set-text="chipFilters.setText"
      />
      <GroupByMenu
        :options="grouping.axisOptions.value"
        :model-value="grouping.groupBy.value"
        @update:model-value="grouping.setGroupBy"
      />
      <button
        type="button"
        class="inline-flex items-center text-[11px] px-2 h-6 rounded-md border border-dashed border-subtle text-tertiary hover:text-primary hover:border-default hover:bg-surface-hover transition-colors"
        :title="$t('views-save-trigger')"
        @click="showSaveModal = true"
      >
        {{ $t('views-save-trigger') }}
      </button>
    </template>

    <template #empty-state>
      <EmptyState
        icon="device"
        :title="controls.searchQuery.value ? $t('empty-assets-search-title') : $t('empty-assets-default-title')"
        :description="controls.searchQuery.value ? $t('empty-assets-search-description') : $t('empty-assets-default-description')"
        :action-label="!controls.searchQuery.value ? $t('assets-list-add-action') : undefined"
        @action="navigateToCreateDevice"
      />
    </template>

    <template #desktop="{ items, isBackgroundRefresh }">
      <DataTable
        :columns="columns"
        :data="items"
        :buckets="buckets"
        :is-collapsed="grouping.isCollapsed"
        :selected-items="dt.selectedItems"
        :sort-field="controls.sortField.value"
        :sort-direction="controls.sortDirection.value"
        :grid-class="gridClass"
        :loading="isBackgroundRefresh"
        @update:sort="controls.handleSortUpdate"
        @toggle-selection="dt.onToggleSelection"
        @toggle-all="dt.onToggleAll"
        @row-click="navigateToDevice"
        @toggle-bucket="grouping.toggleCollapsed"
      >
        <template #cell-name="{ item }">
          <div class="flex flex-col gap-0.5">
            <div class="flex items-center gap-1.5">
              <TextCell :value="item.name" font-weight="medium" />
              <div v-if="item.groups?.length" class="flex items-center gap-1 flex-shrink-0">
                <span
                  v-for="group in item.groups.slice(0, 3)"
                  :key="group.id"
                  class="w-2 h-2 rounded-full flex-shrink-0"
                  :style="{ backgroundColor: group.color || 'var(--color-text-tertiary)' }"
                  :title="group.name"
                />
                <span v-if="item.groups.length > 3" class="text-[10px] text-tertiary">+{{ item.groups.length - 3 }}</span>
              </div>
              <span
                v-if="isLowStock(item)"
                class="text-[10px] px-1.5 py-0.5 rounded-full bg-status-warning/15 text-status-warning whitespace-nowrap font-medium"
                :title="$t('assets-list-low-stock-tooltip', { quantity: item.quantity ?? '', unit: item.unit ?? '', threshold: item.low_stock_threshold ?? '' })"
              >
                {{ $t('assets-list-low-stock-badge') }}
              </span>
            </div>
            <span v-if="item.manufacturer" class="text-xs text-tertiary">{{ item.manufacturer }}</span>
          </div>
        </template>

        <template #cell-serial_number="{ item }">
          <span class="text-xs font-mono text-secondary">{{ item.serial_number || '—' }}</span>
        </template>

        <template #cell-hostname="{ item }">
          <span class="text-xs font-mono text-secondary truncate">{{ (item.attributes?.hostname as string) || '—' }}</span>
        </template>

        <template #cell-model="{ item }">
          <TextCell :value="item.model || '—'" />
        </template>

        <template #cell-primary_user="{ item }">
          <UserAvatarCell
            v-if="item.primary_user"
            :user-id="item.primary_user.uuid"
            :user-name="item.primary_user.name"
            :avatar="item.primary_user.avatar_thumb || item.primary_user.avatar_url"
            :show-name="true"
          />
          <span v-else class="text-xs text-tertiary">{{ $t('assets-list-unassigned') }}</span>
        </template>

        <template #cell-quantity="{ item }">
          <span v-if="item.quantity != null" class="text-sm text-primary tabular-nums whitespace-nowrap">
            {{ item.quantity }}<span v-if="item.unit" class="text-tertiary ml-1">{{ item.unit }}</span>
          </span>
          <span v-else class="text-xs text-tertiary">—</span>
        </template>

        <template #cell-warranty_status="{ item }">
          <StatusBadgeCell type="warranty" :value="(item.attributes?.warranty_status as string | undefined) || ''" />
        </template>
      </DataTable>
    </template>

    <template #mobile-row="{ item }">
      <div
        class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
        @click="navigateToDevice(item)"
      >
        <div class="w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
          <svg class="w-5 h-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
          </svg>
        </div>

        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-1.5">
            <span class="text-sm text-primary font-medium truncate">{{ item.name }}</span>
            <div v-if="item.groups?.length" class="flex items-center gap-1 flex-shrink-0">
              <span
                v-for="group in item.groups.slice(0, 3)"
                :key="group.id"
                class="w-1.5 h-1.5 rounded-full"
                :style="{ backgroundColor: group.color || 'var(--color-text-tertiary)' }"
                :title="group.name"
              />
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-1 text-xs">
            <span v-if="item.model" class="text-secondary">{{ item.model }}</span>
            <span v-if="item.serial_number" class="text-tertiary font-mono">{{ item.serial_number }}</span>
          </div>

          <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-0.5 text-xs">
            <span v-if="item.attributes?.hostname" class="text-tertiary font-mono truncate max-w-[160px]">{{ item.attributes.hostname }}</span>
            <span v-if="item.primary_user" class="text-secondary truncate max-w-[120px]">{{ item.primary_user.name }}</span>
            <span
              v-if="isLowStock(item)"
              class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border bg-status-warning-muted text-status-warning border-status-warning/30"
            >
              {{ $t('assets-list-low-stock-badge') }}
            </span>
            <span
              v-if="item.attributes?.warranty_status"
              class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border"
              :class="{
                'bg-status-success-muted text-status-success border-status-success/30': item.attributes.warranty_status === 'Active',
                'bg-status-warning-muted text-status-warning border-status-warning/30': item.attributes.warranty_status === 'Warning',
                'bg-status-error-muted text-status-error border-status-error/30': item.attributes.warranty_status === 'Expired',
                'bg-surface-alt text-secondary border-default': item.attributes.warranty_status === 'Unknown'
              }"
            >
              {{ item.attributes.warranty_status }}
            </span>
          </div>
        </div>

        <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
        </svg>
      </div>
    </template>

    <template #bulk-actions="{ selectedCount }">
      <button
        type="button"
        class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-status-error hover:bg-status-error/10 transition-colors whitespace-nowrap disabled:opacity-50"
        :disabled="bulkDelete.asyncStatus.value === 'loading'"
        @click="showDeleteConfirm = true"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
        </svg>
        {{ selectedCount > 0 ? $t('assets-list-bulk-delete-count', { count: selectedCount }) : $t('assets-list-bulk-delete') }}
      </button>
    </template>

    <template #footer>
      <PaginationControls
        v-if="!isMobile"
        :current-page="controls.currentPage.value"
        :total-pages="page.totalPages.value"
        :total-items="page.totalItems.value"
        :page-size="controls.pageSize.value"
        :page-size-options="controls.pageSizeOptions"
        :is-infinite-mode="controls.isInfiniteMode.value"
        :show-import="true"
        @update:current-page="controls.handlePageChange"
        @update:page-size="controls.handlePageSizeChange"
        @import="() => {}"
      />
    </template>
  </ListPageLayout>

  <BulkConfirmDialog
    :show="showDeleteConfirm"
    :title="$t('assets-list-bulk-delete-title', { count: selection.selectedCount.value })"
    :message="$t('assets-list-bulk-delete-message', { count: selection.selectedCount.value })"
    :confirm-label="$t('assets-list-bulk-delete-count', { count: selection.selectedCount.value })"
    @confirm="confirmDelete"
    @close="showDeleteConfirm = false"
  />

  <SaveViewModal
    :show="showSaveModal"
    :default-name="defaultSaveName"
    @save="handleSaveAs"
    @close="showSaveModal = false"
  />

  <SavedViewEditorModal
    :view="editingView"
    @rename="handleRename"
    @delete="handleDelete"
    @close="editingView = null"
  />
  </div>
</template>
