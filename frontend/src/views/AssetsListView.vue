<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useMutation, useQueryCache } from '@pinia/colada'
import { extractErrorMessage } from '@/utils/errors'
import { useToastStore } from '@nosdesk/core/stores/toast'

import DataTable from '@/components/common/DataTable.vue'
import Icon from '@/components/common/Icon.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'
import { ICON_REGISTRY } from '@/components/common/icons'
import { useClipboard } from '@/composables/useClipboard'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import ListDensityToggle from '@/components/common/ListDensityToggle.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import { useAssetsDensity } from '@/composables/useTicketsDensity'
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
import AssetMobileRow from '@/components/assets/AssetMobileRow.vue'
import AssetPlanningMobile from '@/components/assets/AssetPlanningMobile.vue'
import type { GroupBucket } from '@/composables/useListGrouping'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { downloadAssetsCsv, getPaginatedAssets, bulkAction, createEmptyAsset, getAssetGroupingDataset, type AssetGroupingRow } from '@/services/assetService'
import { useAssetLocationsQuery } from '@/composables/useAssetLocationsQuery'
import { useAssetGroupsStore } from '@/stores/assetGroups'
import { assetsKeys } from '@nosdesk/core/queries/assets'
import type { Asset } from '@nosdesk/core/types/asset'
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

// Row density, persisted per-view. Assets previously took DataTable's
// `px-2 py-3` default, which put rows at roughly 3x the compact
// tickets row.
//
// `rowClass` pins a fixed row height (28px at compact, a 24px content
// box once padding is off), so every cell has to fit inside it or it
// clips against its neighbours. The current ceiling is the warranty
// exception badge at 22px (`size="xs"`); the status cell is a 16px
// plain glyph, the avatar 20px, the name 20px. Anything taller added
// to a cell here needs that budget rechecked.
const { density, setDensity, rowClass, cellPadding } = useAssetsDensity()

const { locations: knownLocations } = useAssetLocationsQuery()
const assetGroupsStore = useAssetGroupsStore()
void assetGroupsStore.load()
const groupOptions = computed(() =>
  assetGroupsStore.active.map((group) => ({
    value: String(group.id),
    label: group.name,
    hint: t('assets-list-filter-groups-count', { count: group.asset_count }),
  })),
)

function assetKindLabel(kind: string): string {
  return kindLabelBySlug.value.get(kind) ?? kind
}

/** Warranty lives in the `attributes` JSONB bag, not a column. */
function warrantyStatus(asset: Asset): string {
  return (asset.attributes?.warranty_status as string | undefined) ?? ''
}
/** Expiring or already expired: the only two worth a badge. */
function needsWarrantyAttention(asset: Asset): boolean {
  const status = warrantyStatus(asset)
  return status === 'Warning' || status === 'Expired'
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
  {
    key: 'groups',
    labelKey: 'assets-list-filter-groups-label',
    kind: 'multi',
    options: () => groupOptions.value,
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

/**
 * Server-sortable fields, from the match in
 * `backend/src/repository/assets.rs`: id, name, model, manufacturer,
 * location, status, serial_number, quantity, created_at, updated_at.
 * Anything else falls through that match's `_` arm and silently
 * re-sorts by name, so `hostname` and `warranty_status` are marked
 * unsortable here — both live in the `attributes` JSONB bag and have
 * no column to order by.
 */

/**
 * Column widths are `minmax(min, max)` with a *bounded* max, never
 * `auto`. CSS Grid grows non-flexible tracks toward their growth limit
 * before it expands `fr` tracks, so an `auto` max let a long location
 * ("Building B - Comms Room") claim ~300px while the pinned name
 * column collapsed to min-content and truncated. Bounding every other
 * max leaves `name` the slack, which is what `1fr` was meant to do.
 *
 * The minimums are the load-bearing part. Nothing scrolls
 * horizontally — `PageScroll` is `overflow-y-auto` inside an
 * `overflow-hidden` wrapper — so a grid wider than its container is
 * *clipped*, not reachable. The narrowest the desktop table ever
 * renders at is a 1024px viewport (below that `ListPageLayout` swaps
 * in the mobile body) minus the 256px `w-64` navbar, i.e. 768px. The
 * default-visible mins plus the ~40px selection column must stay
 * under that, which is what caps `name` at a 175px minimum.
 *
 * Keep the maxes in px, not `fr`: `useDataTableColumns` skips its
 * stored width override for any column whose width contains `fr`
 * (see its `applyWidth`), so an `fr` max would silently make that
 * column non-resizable. The maxes are layout defaults only — a user
 * drag is clamped by the composable's own 60/800 bounds, deliberately
 * so an explicit resize can override this table's opinion.
 *
 * `kind` and `quantity` are hidden by default: kind reads the same for
 * every row on a device-only workspace, and quantity is populated only
 * for `bulk` kinds. Both are static rather than derived from the kinds
 * registry — that query resolves *after* setup, and a field appearing
 * in the registry late gets tail-appended by `reconcileOrder`, which
 * would shuffle the column to the far right and overwrite the user's
 * saved order on every load.
 */
const columns = computed(() => [
  { field: 'name', label: t('assets-list-column-device'), width: 'minmax(170px,1fr)', sortable: true, responsive: 'always' as const },
  { field: 'kind', label: t('asset-detail-field-kind'), width: 'minmax(85px,125px)', sortable: false, responsive: 'md' as const, defaultHidden: true },
  { field: 'serial_number', label: t('assets-list-column-serial'), width: 'minmax(86px,150px)', sortable: true, responsive: 'md' as const },
  { field: 'hostname', label: t('assets-list-column-hostname'), width: 'minmax(95px,165px)', sortable: false, responsive: 'lg' as const, defaultHidden: true },
  { field: 'model', label: t('assets-list-column-model'), width: 'minmax(98px,185px)', sortable: true, responsive: 'lg' as const },
  { field: 'location', label: t('assets-list-column-location'), width: 'minmax(98px,185px)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: t('assets-list-column-user'), width: 'minmax(98px,170px)', sortable: false, responsive: 'md' as const },
  { field: 'quantity', label: t('assets-list-column-stock'), width: 'minmax(78px,110px)', sortable: true, responsive: 'md' as const, defaultHidden: true },
  { field: 'status', label: t('assets-list-column-status'), width: 'minmax(84px,118px)', sortable: true, responsive: 'always' as const },
  { field: 'warranty_status', label: t('assets-list-column-warranty'), width: 'minmax(70px,98px)', sortable: false, responsive: 'always' as const },
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
  urlSyncParamKeys: ['status', 'warranty', 'lowStock', 'location', 'groups'],
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

// The modal reads from these refs so both entry points (desktop bulk
// selection, mobile whole-bucket) populate the same source.
const rolloutAssetIds = ref<number[]>([])
const rolloutName = ref('')

function openRolloutFromSelection() {
  rolloutAssetIds.value = selectedAssetIds.value
  rolloutName.value = rolloutDefaultName.value
  showRollout.value = true
}

function openRolloutForBucket(bucket: GroupBucket<Asset>) {
  rolloutAssetIds.value = bucket.items.map((a) => a.id)
  rolloutName.value = bucket.label
  showRollout.value = true
}

function onRolloutCreated() {
  showRollout.value = false
  listView.selection.clear()
}

// Right-click row context menu (desktop), mirroring the tickets list.
const { copy } = useClipboard()
const showAssetContextMenu = ref(false)
const contextMenuPos = ref({ x: 0, y: 0 })
const contextAsset = ref<Asset | null>(null)
const showContextDeleteConfirm = ref(false)

const assetContextMenuItems = computed<MenuItem[]>(() => {
  const items: MenuItem[] = [
    { id: 'open', label: t('assets-list-context-open'), icon: ICON_REGISTRY.chevronRight.d },
    { id: 'open-new-tab', label: t('assets-list-context-open-new-tab'), icon: ICON_REGISTRY.openExternal.d },
    { id: 'copy-link', label: t('assets-list-context-copy-link'), icon: ICON_REGISTRY.link.d },
    { id: 'copy-id', label: t('assets-list-context-copy-id'), icon: ICON_REGISTRY.copy.d },
    { id: 'rollout', label: t('assets-list-context-rollout'), icon: ICON_REGISTRY.send.d, divider: true },
  ]
  if (contextAsset.value?.is_editable) {
    items.push({ id: 'delete', label: t('assets-list-context-delete'), icon: ICON_REGISTRY.trash.d, danger: true, divider: true })
  }
  return items
})

function onAssetContextMenu(asset: Asset, event: MouseEvent) {
  contextAsset.value = asset
  contextMenuPos.value = { x: event.clientX, y: event.clientY }
  showAssetContextMenu.value = true
}

async function onAssetContextSelect(actionId: string) {
  const a = contextAsset.value
  if (!a) return
  const url = `/assets/${a.id}`
  switch (actionId) {
    case 'open': navigateToAsset(a); break
    case 'open-new-tab': window.open(url, '_blank'); break
    case 'copy-link': await copy(`${window.location.origin}${url}`); break
    case 'copy-id': await copy(String(a.id)); break
    case 'rollout':
      rolloutAssetIds.value = [a.id]
      rolloutName.value = a.name
      showRollout.value = true
      break
    case 'delete': showContextDeleteConfirm.value = true; break
  }
}

async function confirmContextDelete() {
  showContextDeleteConfirm.value = false
  const a = contextAsset.value
  if (!a) return
  await bulkDelete.mutateAsync([a.id])
}

// Mobile planning lens: when a planning axis is active, the mobile body
// switches from the flat list to the summary -> drill-down view.
const activeAxisLabel = computed<string>(() => {
  const axis = groupAxes.find((a) => a.key === listView.grouping.groupBy.value)
  return axis ? t(axis.labelKey) : ''
})

function filterString(value: string | number | undefined): string | undefined {
  if (value == null || value === '') return undefined
  return String(value)
}

async function exportAssetsCsv(scope?: 'history') {
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
    }, scope)
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
    :mobile-slot-active="listView.completeActive.value"
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
          <!-- Desktop table only; the mobile body is a card list, and
               ListPageLayout swaps to it below 1024px. The `hidden`
               goes on a WRAPPER, not the component: ListDensityToggle's
               own root carries a base `inline-flex`, which Tailwind
               emits after `.hidden` and so wins the merge, silently
               leaving the toggle visible but inert. Same reason
               TicketsHeader wraps it. -->
          <div class="hidden lg:block">
            <ListDensityToggle :density="density" @set-density="setDensity" />
          </div>
          <button
            type="button"
            class="inline-flex items-center text-[11px] px-2 h-6 rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-secondary"
            :title="listView.page.totalItems.value === 0 ? $t('assets-list-export-empty') : $t('assets-list-export-csv')"
            :disabled="listView.page.totalItems.value === 0"
            @click="exportAssetsCsv()"
          >
            {{ $t('assets-list-export-csv') }}
          </button>
          <button
            type="button"
            class="inline-flex items-center text-[11px] px-2 h-6 rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-secondary"
            :title="listView.page.totalItems.value === 0 ? $t('assets-list-export-empty') : $t('assets-list-export-history')"
            :disabled="listView.page.totalItems.value === 0"
            @click="exportAssetsCsv('history')"
          >
            {{ $t('assets-list-export-history') }}
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
        :row-class="rowClass"
        :cell-padding="cellPadding"
        @update:sort="listView.controls.handleSortUpdate"
        @toggle-selection="listView.dt.onToggleSelection"
        @toggle-all="listView.dt.onToggleAll"
        @row-click="navigateToAsset"
        @row-contextmenu="onAssetContextMenu"
        @toggle-bucket="listView.grouping.toggleCollapsed"
      >
        <!-- Single line, manufacturer inline. It used to stack over a
             second line, which is what a fixed row height can't
             accommodate: at compact the row is 28px and a stacked cell
             needs ~38px, so it would overflow its own row. Inline also
             stops the name column wrapping, which was the other half
             of the ~100px rows. -->
        <template #cell-name="{ item }">
          <div class="flex items-center gap-1.5 min-w-0">
            <TextCell :value="item.name" font-weight="medium" />
            <!-- Capped and non-shrinking so it can't compete with the
                 name for the track. As equal-shrink truncating
                 siblings both gave up width proportionally, which made
                 the pinned identity column read worse than before. -->
            <span
              v-if="item.manufacturer"
              class="text-xs text-tertiary truncate flex-shrink-0 max-w-[35%]"
            >
              {{ item.manufacturer }}
            </span>
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
              class="text-[10px] px-1.5 py-0.5 rounded-full bg-status-warning/15 text-status-warning whitespace-nowrap font-medium flex-shrink-0"
              :title="$t('assets-list-low-stock-tooltip', { quantity: item.quantity ?? '', unit: item.unit ?? '', threshold: item.low_stock_threshold ?? '' })"
            >
              {{ $t('assets-list-low-stock-badge') }}
            </span>
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
          <!-- `xs` (20px), not the default `sm` (24px): a compact row's
               content box is exactly 24px, so `sm` sits flush with no
               margin for error. -->
          <UserAvatarCell
            v-if="item.primary_user"
            :user-id="item.primary_user.uuid"
            :user-name="item.primary_user.name"
            :avatar="item.primary_user.avatar_thumb || item.primary_user.avatar_url"
            size="xs"
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
          <AssetStatusBadge :status="item.status || 'in_service'" variant="plain" />
        </template>

        <!-- Only the states that want acting on carry a badge. "Active"
             and "Unknown" are the overwhelming majority, so a pill on
             every row spends attention on the rows that need none. -->
        <template #cell-warranty_status="{ item }">
          <StatusBadgeCell
            v-if="needsWarrantyAttention(item)"
            type="warranty"
            size="xs"
            :value="warrantyStatus(item)"
          />
          <span v-else class="text-xs text-tertiary truncate">{{ warrantyStatus(item) || '-' }}</span>
        </template>
      </DataTable>
    </template>

    <!-- Planning lens (mobileSlotActive): glanceable bucket summary ->
         drill-down -> whole-bucket rollout. The flat device list below
         handles every other state with its default staggered entrance. -->
    <template #mobile>
      <AssetPlanningMobile
        :buckets="listView.buckets.value"
        :axis-label="activeAxisLabel"
        @open="navigateToAsset"
        @rollout="openRolloutForBucket"
      />
    </template>

    <template #mobile-row="{ item }">
      <AssetMobileRow :asset="item" @open="navigateToAsset" />
    </template>

    <template #bulk-actions="{ selectedCount }">
      <button
        type="button"
        class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-accent hover:bg-accent/10 transition-colors whitespace-nowrap"
        @click="openRolloutFromSelection"
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
        @update:current-page="listView.controls.handlePageChange"
        @update:page-size="listView.controls.handlePageSizeChange"
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
    :asset-ids="rolloutAssetIds"
    :default-name="rolloutName"
    @close="showRollout = false"
    @created="onRolloutCreated"
  />

  <ContextMenu
    :open="showAssetContextMenu"
    :items="assetContextMenuItems"
    :x="contextMenuPos.x"
    :y="contextMenuPos.y"
    @select="onAssetContextSelect"
    @close="showAssetContextMenu = false"
  />

  <BulkConfirmDialog
    :show="showContextDeleteConfirm"
    :title="$t('assets-list-bulk-delete-title', { count: 1 })"
    :message="$t('assets-list-bulk-delete-message', { count: 1 })"
    :confirm-label="$t('assets-list-bulk-delete-count', { count: 1 })"
    @confirm="confirmContextDelete"
    @close="showContextDeleteConfirm = false"
  />

  <ListViewModals :list-view="listView" />
  </div>
</template>
