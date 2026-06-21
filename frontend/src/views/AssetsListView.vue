<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useMutation, useQueryCache } from '@pinia/colada'
import { extractErrorMessage } from '@/utils/errors'
import { useToastStore } from '@/stores/toast'

import DataTable from '@/components/common/DataTable.vue'
import Icon from '@/components/common/Icon.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import ListViewToolbar from '@/components/views/ListViewToolbar.vue'
import ListViewModals from '@/components/views/ListViewModals.vue'
import { useListView } from '@/composables/useListView'
import type { ChipFacetDef } from '@/composables/useChipFiltersFromControls'
import type { GroupAxisDef } from '@/composables/useListGrouping'
import { useAuthStore } from '@/stores/auth'

import { TextCell, StatusBadgeCell, UserAvatarCell } from '@/components/common/cells'
import AssetViewTabs from '@/components/assets/AssetViewTabs.vue'
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue'
import CreateRolloutModal from '@/components/assets/CreateRolloutModal.vue'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { downloadAssetsCsv, getPaginatedAssets, bulkAction, createEmptyAsset, getAssetGroupingDataset, type AssetGroupingRow } from '@/services/assetService'
import { useAssetLocationsQuery } from '@/composables/useAssetLocationsQuery'
import { assetsKeys } from '@/queries/assets'
import type { Asset } from '@/types/asset'
import {
  assetStatusChipOptions,
  assetStatusLabel,
  assetStatusSortIndex,
} from '@/utils/assetStatusMeta'

defineOptions({ name: 'AssetsListView' })

const router = useRouter()
const queryCache = useQueryCache()
const { isMobile } = useMobileDetection()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toast = useToastStore()
const auth = useAuthStore()
const userUuid = computed<string | null>(() => auth.user?.uuid ?? null)
const { kinds } = useAssetKindsQuery()
const kindLabelBySlug = computed(() => new Map(kinds.value.map((kind) => [kind.slug, kind.label])))
const { locations: knownLocations } = useAssetLocationsQuery()

function assetKindLabel(kind: string): string {
  return kindLabelBySlug.value.get(kind) ?? kind
}

const locationOptions = computed(() =>
  knownLocations.value.map((location) => ({
    value: location.location,
    label: location.location,
    description: t('assets-list-filter-location-count', { count: location.asset_count }),
  })),
)

const layoutRef = useTemplateRef<ListPageLayoutExpose>('layout')
const scrollContainerRef = computed<HTMLElement | null>(
  () => layoutRef.value?.scrollContainerRef ?? null,
)

// Creation mirrors the ticket model: mint an empty asset and drop the
// user on its detail page to fill it in inline. No separate form.
const navigateToCreateAsset = async () => {
  try {
    const asset = await createEmptyAsset()
    await router.push(`/assets/${asset.id}`)
  } catch (err) {
    toast.error(extractErrorMessage(err, t('assets-list-create-error')))
  }
}
const navigateToAsset = (asset: Asset) => {
  void router.push(`/assets/${asset.id}`)
}
usePageCreateAction(navigateToCreateAsset)

// Filter facets (chip UI). Backend encoding:
//   name      -> controls.searchQuery (chip text-facet)
//   status    -> CSV under one filter key, backend eq_any
//   warranty  -> CSV under one filter key, backend ANY-matches
//   lowStock  -> single 'true' when on, absent when off
const assetFacets = computed<ChipFacetDef[]>(() => [
  {
    key: 'name',
    labelKey: 'assets-list-filter-name-label',
    kind: 'text',
    searchInput: true,
    options: () => [],
  },
  {
    key: 'status',
    labelKey: 'assets-list-filter-status-label',
    kind: 'multi',
    options: () => assetStatusChipOptions(t),
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
  {
    key: 'location',
    labelKey: 'assets-list-filter-location-label',
    kind: 'multi',
    options: () => locationOptions.value,
  },
])

// Group-by axes. Client-side bucketing of the loaded page; most
// useful in infinite-scroll mode (default pageSize=0 → up to 50
// rows in one shot). The fleet-planning axes (os_family /
// warranty_window / compliance) instead source the complete
// filtered set via `completeDataset` below, so their counts and
// "select all in a bucket" cover the whole fleet.
const WARRANTY_ORDER = ['Expired', 'Warning', 'Active', 'Unknown'] as const

// Planning-lens orderings + labels. Bucket keys are server-derived
// (os_family / warranty_window); compliance reads the raw attribute.
const OS_ORDER = ['windows', 'macos', 'linux', 'ios', 'android', 'other'] as const
const WARRANTY_WINDOW_ORDER = ['expired', 'expiring_30d', 'expiring_90d', 'active', 'unknown'] as const
const PLANNING_AXES = ['os_family', 'warranty_window', 'compliance'] as const

function osFamilyLabel(fam: string): string {
  return t(`assets-list-os-${fam}`)
}
function warrantyWindowLabel(win: string): string {
  return t(`assets-list-warranty-window-${win.replace(/_/g, '-')}`)
}

const groupAxes: GroupAxisDef<Asset>[] = [
  {
    key: 'status',
    labelKey: 'assets-list-grouping-status',
    bucketFor: (asset) => {
      const status = asset.status || 'in_service'
      return { key: `status:${status}`, label: assetStatusLabel(t, status) }
    },
    sortBy: (bucketKey) => assetStatusSortIndex(bucketKey.replace('status:', '')),
  },
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
    bucketFor: (asset) => ({ key: `kind:${asset.kind}`, label: assetKindLabel(asset.kind) }),
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
  // Fleet-planning lenses. These only render over the complete dataset
  // (see `completeDataset`), where each row carries the server-derived
  // os_family / warranty_window buckets.
  {
    key: 'os_family',
    labelKey: 'assets-list-grouping-os',
    bucketFor: (asset) => {
      const fam = (asset as AssetGroupingRow).os_family || 'other'
      return { key: `os:${fam}`, label: osFamilyLabel(fam) }
    },
    sortBy: (bucketKey) => {
      const i = OS_ORDER.indexOf(bucketKey.replace('os:', '') as (typeof OS_ORDER)[number])
      return i === -1 ? 999 : i
    },
  },
  {
    key: 'warranty_window',
    labelKey: 'assets-list-grouping-warranty-window',
    bucketFor: (asset) => {
      const win = (asset as AssetGroupingRow).warranty_window || 'unknown'
      return { key: `ww:${win}`, label: warrantyWindowLabel(win) }
    },
    sortBy: (bucketKey) => {
      const i = WARRANTY_WINDOW_ORDER.indexOf(
        bucketKey.replace('ww:', '') as (typeof WARRANTY_WINDOW_ORDER)[number],
      )
      return i === -1 ? 999 : i
    },
  },
  {
    key: 'compliance',
    labelKey: 'assets-list-grouping-compliance',
    bucketFor: (asset) => {
      const raw = (asset.attributes?.compliance_state as string | undefined) ?? ''
      const key = raw || 'unknown'
      const label = raw || t('assets-list-compliance-unknown')
      return { key: `compliance:${key}`, label }
    },
  },
]

// Available sortable fields: id, name, hostname, serial_number,
// model, warranty_status, manufacturer, created_at, updated_at,
// last_sync_time.
const columns = computed(() => [
  { field: 'name', label: t('assets-list-column-device'), width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'kind', label: t('asset-detail-field-kind'), width: 'minmax(120px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'serial_number', label: t('assets-list-column-serial'), width: 'minmax(140px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'hostname', label: t('assets-list-column-hostname'), width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const, defaultHidden: true },
  { field: 'model', label: t('assets-list-column-model'), width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'location', label: t('assets-list-column-location'), width: 'minmax(140px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: t('assets-list-column-user'), width: 'minmax(140px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'quantity', label: t('assets-list-column-stock'), width: 'minmax(100px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'status', label: t('assets-list-column-status'), width: 'minmax(110px,auto)', sortable: true, responsive: 'always' as const },
  { field: 'warranty_status', label: t('assets-list-column-warranty'), width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const },
])

// Shell composable bundling controls + page + selection + chip
// filters + grouping + columns + saved-view round-trip in one
// call. View-specific bits (bulk delete, cell renderers,
// navigation) stay in this file below.
const listView = useListView({
  dataset: 'assets',
  userUuid,
  t,
  itemIdField: 'id',
  defaultSortField: 'name',
  pageKeys: assetsKeys,
  fetchPage: (params) => getPaginatedAssets(params, `assets-page-${params.page}`),
  syncAggregates: ['asset'],
  mobileSearch: {
    placeholder: t('assets-list-search-placeholder'),
    createIcon: 'device',
    onCreate: navigateToCreateAsset,
  },
  urlSyncParamKeys: ['status', 'warranty', 'lowStock', 'location'],
  scrollContainerRef,
  facets: assetFacets,
  groupAxes,
  completeDataset: {
    axes: PLANNING_AXES,
    fetch: (params) =>
      getAssetGroupingDataset({
        search: params.search as string | undefined,
        status: params.status as string | undefined,
        warranty: params.warranty as string | undefined,
        location: params.location as string | undefined,
        lowStock: params.lowStock as string | undefined,
      }),
    keyFor: (cacheKeyPart) => [...assetsKeys.root, 'grouping-dataset', cacheKeyPart],
  },
  columns,
  pinnedColumnIds: ['name'],
})

// Display source: the complete planning dataset when a planning lens
// is active, otherwise the paginated page. The loading flags fall back
// to the complete query's status in that mode.
const displayItems = computed(() => listView.effectiveItems.value)
const displayTotal = computed(() =>
  listView.completeActive.value
    ? listView.effectiveItems.value.length
    : listView.page.totalItems.value,
)
const displayFirstLoad = computed(() =>
  listView.completeActive.value
    ? listView.completeLoading.value && listView.effectiveItems.value.length === 0
    : listView.page.isFirstLoad.value,
)
const displayBackgroundRefresh = computed(() =>
  listView.completeActive.value
    ? listView.completeLoading.value && listView.effectiveItems.value.length > 0
    : listView.page.isBackgroundRefresh.value,
)
const displayLoadingMore = computed(() =>
  listView.completeActive.value ? false : listView.page.isLoadingMore.value,
)
const displayError = computed(() =>
  listView.completeActive.value ? null : listView.page.errorMessage.value,
)

// Bulk delete: irreversible (devices aren't soft-deleted), so a
// confirm modal rather than the optimistic Undo-toast pattern.
const showDeleteConfirm = ref(false)
const bulkDelete = useMutation({
  mutation: (ids: number[]) => bulkAction({ action: 'delete', ids }),
  onSettled: () => queryCache.invalidateQueries({ key: assetsKeys.root }),
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
  const ids = listView.selection.selectedIds.value.map((id) => parseInt(id))
  if (ids.length === 0) return
  await bulkDelete.mutateAsync(ids)
  listView.selection.clear()
}

// Create-rollout handoff: turn the selected devices into a project with
// one ticket per device. Operates on the explicit selection so the count
// shown is exactly what gets created.
const showRollout = ref(false)
const selectedAssetIds = computed(() =>
  listView.selection.selectedIds.value.map((id) => parseInt(id)),
)

// Seed the rollout name from the bucket the selection sits in: when every
// selected device falls in one group (the common "select a whole bucket"
// case), suggest that bucket's label (e.g. "Expiring within 90 days").
const rolloutDefaultName = computed<string>(() => {
  const selected = new Set(listView.selection.selectedIds.value)
  if (selected.size === 0) return ''
  const covering = listView.buckets.value.filter((b) =>
    b.items.some((it) => selected.has(String(it.id))),
  )
  if (covering.length !== 1) return ''
  const bucket = covering[0]
  const selectedInBucket = bucket.items.filter((it) => selected.has(String(it.id))).length
  return selectedInBucket === selected.size ? bucket.label : ''
})

function onRolloutCreated() {
  showRollout.value = false
  listView.selection.clear()
}

function filterString(value: string | number | undefined): string | undefined {
  if (value == null || value === '') return undefined
  return String(value)
}

async function exportAssetsCsv() {
  if (listView.page.totalItems.value === 0) {
    toast.info(t('assets-list-export-empty'));
    return;
  }

  const p = listView.controls.requestParams.value
  try {
    await downloadAssetsCsv({
      search: filterString(p.search),
      status: filterString(p.status),
      warranty: filterString(p.warranty),
      location: filterString(p.location),
      lowStock: filterString(p.lowStock),
    })
  } catch (error) {
    toast.error(extractErrorMessage(error, t('assets-list-export-failed')))
  }
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
    :items="displayItems"
    :total-items="displayTotal"
    :is-first-load="displayFirstLoad"
    :is-background-refresh="displayBackgroundRefresh"
    :is-loading-more="displayLoadingMore"
    :error="displayError"
    :search-query="listView.controls.searchQuery.value"
    :search-placeholder="$t('assets-list-search-placeholder')"
    :item-label="$t('assets-list-item-label')"
    bulk-selection-copy-key="bulk-bar-devices-selected"
    bulk-all-selected-copy-key="bulk-bar-devices-all-selected"
    :bulk-selection="listView.selection"
    hide-desktop-search
    @update:search-query="listView.controls.handleSearchUpdate"
    @retry="listView.page.handleRetry"
  >
    <template #view-tabs>
      <AssetViewTabs />
    </template>

    <template #filters>
      <ListViewToolbar
        :list-view="listView"
        :switcher-placeholder="$t('views-asset-switcher-placeholder')"
        @open-editor="listView.openEditor"
        @save-as="listView.showSaveModal.value = true"
      >
        <template #append>
          <button
            type="button"
            class="inline-flex items-center text-[11px] px-2 h-6 rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-secondary"
            :title="listView.page.totalItems.value === 0 ? $t('assets-list-export-empty') : $t('assets-list-export-csv')"
            :disabled="listView.page.totalItems.value === 0"
            @click="exportAssetsCsv"
          >
            {{ $t('assets-list-export-csv') }}
          </button>
        </template>
      </ListViewToolbar>
    </template>

    <template #empty-state>
      <EmptyState
        icon="device"
        :title="listView.controls.searchQuery.value ? $t('empty-assets-search-title') : $t('empty-assets-default-title')"
        :description="listView.controls.searchQuery.value ? $t('empty-assets-search-description') : $t('empty-assets-default-description')"
        :action-label="!listView.controls.searchQuery.value ? $t('assets-list-add-action') : undefined"
        @action="navigateToCreateAsset"
      />
    </template>

    <template #desktop="{ items, isBackgroundRefresh }">
      <DataTable
        :columns="listView.tableColumns.visible.value"
        :data="items"
        :buckets="listView.buckets.value"
        :is-collapsed="listView.grouping.isCollapsed"
        :selected-items="listView.dt.selectedItems"
        :sort-field="listView.controls.sortField.value"
        :sort-direction="listView.controls.sortDirection.value"
        :column-reorder="listView.tableColumns.reorderBundle"
        :column-resize="listView.tableColumns.resizeBundle"
        :loading="isBackgroundRefresh"
        @update:sort="listView.controls.handleSortUpdate"
        @toggle-selection="listView.dt.onToggleSelection"
        @toggle-all="listView.dt.onToggleAll"
        @row-click="navigateToAsset"
        @toggle-bucket="listView.grouping.toggleCollapsed"
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
          <span class="text-xs font-mono text-secondary">{{ item.serial_number || '-' }}</span>
        </template>

        <template #cell-kind="{ item }">
          <span class="text-xs font-medium text-secondary">{{ assetKindLabel(item.kind) }}</span>
        </template>

        <template #cell-hostname="{ item }">
          <span class="text-xs font-mono text-secondary truncate">{{ (item.attributes?.hostname as string) || '-' }}</span>
        </template>

        <template #cell-model="{ item }">
          <TextCell :value="item.model || '-'" />
        </template>

        <template #cell-location="{ item }">
          <TextCell :value="item.location || '-'" />
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
          <span v-else class="text-xs text-tertiary">-</span>
        </template>

        <template #cell-status="{ item }">
          <AssetStatusBadge :status="item.status || 'in_service'" />
        </template>

        <template #cell-warranty_status="{ item }">
          <StatusBadgeCell type="warranty" :value="(item.attributes?.warranty_status as string | undefined) || ''" />
        </template>
      </DataTable>
    </template>

    <template #mobile-row="{ item }">
      <div
        class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
        @click="navigateToAsset(item)"
      >
        <div class="w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
          <Icon name="device" size="md" class="text-secondary" />
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
            <span v-if="item.location" class="text-tertiary truncate max-w-[140px]">{{ item.location }}</span>
            <span class="text-tertiary">{{ assetKindLabel(item.kind) }}</span>
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
            <AssetStatusBadge :status="item.status || 'in_service'" />
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

        <Icon name="chevronRight" size="sm" class="text-tertiary flex-shrink-0" />
      </div>
    </template>

    <template #bulk-actions="{ selectedCount }">
      <button
        type="button"
        class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-accent hover:bg-accent/10 transition-colors whitespace-nowrap"
        @click="showRollout = true"
      >
        <Icon name="send" size="sm" />
        {{ $t('asset-rollout-bulk-action', { count: selectedCount }) }}
      </button>
      <button
        type="button"
        class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-status-error hover:bg-status-error/10 transition-colors whitespace-nowrap disabled:opacity-50"
        :disabled="bulkDelete.asyncStatus.value === 'loading'"
        @click="showDeleteConfirm = true"
      >
        <Icon name="trash" size="sm" />
        {{ selectedCount > 0 ? $t('assets-list-bulk-delete-count', { count: selectedCount }) : $t('assets-list-bulk-delete') }}
      </button>
    </template>

    <template #footer>
      <PaginationControls
        v-if="!isMobile && !listView.completeActive.value"
        :current-page="listView.controls.currentPage.value"
        :total-pages="listView.page.totalPages.value"
        :total-items="listView.page.totalItems.value"
        :page-size="listView.controls.pageSize.value"
        :page-size-options="listView.controls.pageSizeOptions"
        :is-infinite-mode="listView.controls.isInfiniteMode.value"
        :show-import="true"
        @update:current-page="listView.controls.handlePageChange"
        @update:page-size="listView.controls.handlePageSizeChange"
        @import="() => {}"
      />
    </template>
  </ListPageLayout>

  <BulkConfirmDialog
    :show="showDeleteConfirm"
    :title="$t('assets-list-bulk-delete-title', { count: listView.selection.selectedCount.value })"
    :message="$t('assets-list-bulk-delete-message', { count: listView.selection.selectedCount.value })"
    :confirm-label="$t('assets-list-bulk-delete-count', { count: listView.selection.selectedCount.value })"
    @confirm="confirmDelete"
    @close="showDeleteConfirm = false"
  />

  <CreateRolloutModal
    :show="showRollout"
    :asset-ids="selectedAssetIds"
    :default-name="rolloutDefaultName"
    @close="showRollout = false"
    @created="onRolloutCreated"
  />

  <ListViewModals :list-view="listView" />
  </div>
</template>
