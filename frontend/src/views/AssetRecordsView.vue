<script setup lang="ts">
/**
 * The Records tab: work done across every asset, beside Inventory, Catalog
 * and Groups. An MSP technician or IT lead reads it to answer: what is
 * still open, what did we spend and with whom, what happened under this
 * RMA number, show me every record of one type.
 *
 * Structure, the same at every width:
 *   - two edges, 16px in from each side of the page, that the tabs, the
 *     count, the first column, the totals and the Cost column all sit on;
 *   - a toolbar of two groups (where you are, then what you are looking
 *     at) that share a line when there is room and stack when there isn't;
 *   - one line that says what the filtered set is: how many, how many
 *     still in progress, and what they cost per currency;
 *   - the list: a table from 1024px, two-line rows below.
 *
 * The server pages the list. The count, the totals and each group's
 * totals cover the whole filtered set whatever is loaded; grouped, the
 * rows arrive group by group, so loading more continues the last group.
 */
import { computed, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import DataTable, { type Column, type DataTableBucket } from '@/components/common/DataTable.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import IconButton from '@/components/common/IconButton.vue'
import ListPageLayout from '@/components/common/ListPageLayout.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import AddFilterMenu, { type AddFilterFacet } from '@/components/views/AddFilterMenu.vue'
import FilterPill from '@/components/views/FilterPill.vue'
import AssetRecordDetail from '@/components/assets/AssetRecordDetail.vue'
import AssetViewTabs from '@/components/assets/AssetViewTabs.vue'
import AssetRecordMobileRow from '@/components/assets/records/AssetRecordMobileRow.vue'
import PeriodPill from '@/components/assets/records/PeriodPill.vue'
import {
  GROUP_AXES,
  clearFilters,
  listFilter,
  useRecordsQuery,
  type GroupAxis,
  type PeriodPreset,
  type SortKey,
  type StatusValue,
} from '@/components/assets/records/recordsQuery'
import { formatDay, todayDay, typeName } from '@/components/assets/assetTimeline'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import type { FilterOption } from '@/composables/useListFilters'
import { useAssetRecordTypes } from '@/composables/useAssetTimeline'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useSyncActions } from '@/composables/useSyncActions'
import {
  useWorkspaceInfiniteQuery,
  useWorkspaceQuery,
  useWorkspaceQueryCache,
} from '@/queries/workspaceQuery'
import { downloadAssetRecordsCsv } from '@/services/assetService'
import { useAuthStore } from '@/stores/auth'
import { extractErrorMessage } from '@/utils/errors'
import { assetRecordKeys, assetRecordService } from '@nosdesk/core/services/assetRecordService'
import { useToastStore } from '@nosdesk/core/stores/toast'
import type {
  AssetRecordCurrencyTotal,
  AssetRecordList,
  AssetRecordListRow,
} from '@nosdesk/core/types/asset'
import { formatMoney } from '@nosdesk/core/utils/money'
import { totalText } from '@/components/assets/records/recordTotals'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toasts = useToastStore()
const router = useRouter()
const auth = useAuthStore()
const query = useRecordsQuery()
const { types, currency: typesCurrency, suggestedCurrency, timezone } = useAssetRecordTypes(true)
/** Today in the workspace's timezone, the one the server dates work in. */
const today = computed(() => todayDay(timezone.value ?? undefined))
const typeBySlug = computed(() => new Map(types.value.map((type) => [type.slug, type])))
const typeById = computed(() => new Map(types.value.map((type) => [type.id, type])))
const typeLabel = (slug: string | null) => typeName(t, typeBySlug.value.get(slug ?? ''), slug)

// --- The filtered set -------------------------------------------------

const PAGE = 100
const filter = computed(() => listFilter(query, today.value))

const listQuery = useWorkspaceInfiniteQuery<AssetRecordList, Error, number>({
  key: () => [...assetRecordKeys.listFiltered(filter.value)],
  initialPageParam: 0,
  query: ({ pageParam }) => assetRecordService.listAll({ ...filter.value, offset: pageParam, limit: PAGE }),
  getNextPageParam: (last) => last.next_offset,
  // A changed filter keeps the list it had until the new one answers.
  placeholderData: (previous) => previous,
})
const facetsQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.facets],
  query: () => assetRecordService.facets(),
})

const pages = computed(() => listQuery.data.value?.pages ?? [])
const first = computed<AssetRecordList | null>(() => pages.value[0] ?? null)
const records = computed<AssetRecordListRow[]>(() => pages.value.flatMap((page) => page.records))
const total = computed(() => first.value?.total ?? 0)
const openCount = computed(() => first.value?.open ?? 0)
const totals = computed(() => first.value?.totals ?? [])
const remaining = computed(() => Math.max(0, total.value - records.value.length))
const loadingMore = computed(() => listQuery.asyncStatus.value === 'loading' && pages.value.length > 0)
/** Nothing to say yet: the first answer hasn't come. No skeleton; the
 *  chrome renders and the body waits. */
const firstLoad = computed(() => listQuery.data.value === undefined || facetsQuery.data.value === undefined)
const hasAny = computed(() => (facetsQuery.data.value?.total ?? 0) > 0)
/** The current query has nothing of its own to show because it failed (a
 *  refused filter, say): say so rather than keep the last list under the
 *  new pills. A failed Show more or refetch keeps its list and says so in a
 *  toast instead. */
const loadError = computed(() =>
  listQuery.status.value === 'error' &&
  listQuery.error.value &&
  (listQuery.data.value === undefined || listQuery.isPlaceholderData.value)
    ? extractErrorMessage(listQuery.error.value, t('asset-records-load-failed'))
    : null,
)
watch(
  () => listQuery.error.value,
  (error) => {
    if (!error || listQuery.data.value === undefined || listQuery.isPlaceholderData.value) return
    toasts.error(extractErrorMessage(error, t('asset-records-load-failed')))
  },
)

/** The workspace currency, which totals put last (on the Cost edge). */
const currency = computed(
  () => first.value?.currency ?? typesCurrency.value ?? suggestedCurrency.value ?? '',
)
const totalLabel = (total: AssetRecordCurrencyTotal) => totalText(total)
const costLabel = (cost: AssetRecordListRow['cost']) => (cost ? formatMoney(cost.amount, cost.currency) : null)

async function loadMore() {
  if (listQuery.asyncStatus.value === 'loading') return
  // A failure lands in the query's error, which the toast above reports.
  await listQuery.loadNextPage().catch(() => undefined)
}

// Records written anywhere (the asset page, an import, a rule) refetch
// the list and the filters' choices, and so does a change to an asset the
// rows or the asset filter name (renamed, retagged). Other assets' changes,
// an MDM sync's bursts among them, leave the list alone.
const cache = useWorkspaceQueryCache()
function namesAsset(id: string): boolean {
  return (
    records.value.some((r) => String(r.asset.id) === id) ||
    (facetsQuery.data.value?.assets ?? []).some((a) => String(a.asset.id) === id)
  )
}
useSyncActions(
  (actions) => {
    if (actions.some((a) => a.aggregate === 'asset_record' || namesAsset(a.aggregate_id))) {
      void cache.invalidateQueries({ key: [...assetRecordKeys.list] })
    }
  },
  {
    aggregates: ['asset_record', 'asset'],
    debounceMs: 250,
  },
)

// --- Grouping ---------------------------------------------------------

const grouped = computed(() => query.group !== 'none')
/** Groups render once the server has answered for them; until then (the
 *  ungrouped list held while the grouped one loads) the rows stay flat. */
const showGroups = computed(
  () => grouped.value && !!first.value?.groups && first.value.group === query.group,
)

interface RecordBucket extends DataTableBucket<AssetRecordListRow> {
  totals: AssetRecordCurrencyTotal[]
}

function groupLabel(key: string, axis: GroupAxis, label: string | null, items: AssetRecordListRow[]): string {
  if (axis === 'vendor') return key === '' ? t('asset-records-group-no-vendor') : (label ?? items[0]?.vendor ?? key)
  if (axis === 'type') {
    const type = typeById.value.get(Number(key))
    return typeName(t, type, type?.slug ?? items[0]?.record_type)
  }
  if (axis === 'asset') {
    const asset = items[0]?.asset
    return asset ? `${asset.asset_tag ?? ''}  ${asset.name}`.trim() : key
  }
  return formatDay(`${key}-01`, 'monthYear')
}

/** The server's groups, in its order, each with the rows loaded so far.
 *  Groups whose rows haven't loaded yet wait below "Show more". */
const buckets = computed<RecordBucket[]>(() => {
  if (!showGroups.value || !first.value?.groups) return []
  const byKey = new Map<string, AssetRecordListRow[]>()
  for (const record of records.value) {
    const key = record.group ?? ''
    const items = byKey.get(key) ?? []
    items.push(record)
    byKey.set(key, items)
  }
  return first.value.groups
    .filter((group) => byKey.has(group.key))
    .map((group) => {
      const items = byKey.get(group.key) ?? []
      return {
        key: group.key,
        label: groupLabel(group.key, query.group, group.label, items),
        items,
        count: group.count,
        totals: group.totals,
      }
    })
})

const collapsed = ref(new Set<string>())
watch(
  () => query.group,
  () => (collapsed.value = new Set()),
)
function toggleBucket(key: string) {
  const next = new Set(collapsed.value)
  if (!next.delete(key)) next.add(key)
  collapsed.value = next
}

// --- Columns ----------------------------------------------------------
// The table has as little as 768px (a 1024px window beside the sidebar)
// and seven things to say. Date, Type, Reference and Cost take what their
// content needs; Summary, Asset and Vendor share the rest, Summary first.
// When the list is narrow, the two that repeat what the row already says
// step out, in this order: the asset's name (its tag stays), then Type
// (the summary all but names it).

const listBox = ref<HTMLElement | null>(null)
const listWidth = ref(1200)
const observer = new ResizeObserver(([entry]) => (listWidth.value = entry.contentRect.width))
onMounted(() => listBox.value && observer.observe(listBox.value))
onBeforeUnmount(() => observer.disconnect())
const showType = computed(() => listWidth.value >= 880)
const showAssetName = computed(() => listWidth.value >= 1040)

const allColumns = computed<Column[]>(() => [
  { field: 'occurred_on', label: t('asset-records-column-date'), width: 'max-content', sortable: true, sortKey: 'date' },
  {
    field: 'asset',
    label: t('asset-records-column-asset'),
    width: showAssetName.value ? 'minmax(9rem, 1.3fr)' : 'max-content',
    sortable: true,
  },
  ...(showType.value
    ? [{ field: 'record_type', label: t('asset-records-column-type'), width: 'max-content' }]
    : []),
  { field: 'summary', label: t('asset-records-column-summary'), width: 'minmax(10rem, 2.5fr)' },
  { field: 'vendor', label: t('asset-records-column-vendor'), width: 'minmax(6rem, 1fr)', sortable: true },
  { field: 'reference', label: t('asset-records-column-reference'), width: 'max-content' },
  { field: 'cost', label: t('asset-records-column-cost'), width: 'max-content', sortable: true, align: 'end' },
])
/** The column a group's header already says steps out too. */
const GROUP_COLUMN: Partial<Record<GroupAxis, string>> = { vendor: 'vendor', type: 'record_type', asset: 'asset' }
const columns = computed(() => allColumns.value.filter((column) => column.field !== GROUP_COLUMN[query.group]))

function onSort(field: string, direction: 'asc' | 'desc') {
  // A column first sorts the way it is read for: newest and dearest first.
  const fresh = query.sort !== field
  query.sort = field as SortKey
  query.dir = fresh && (field === 'date' || field === 'cost') ? 'desc' : direction
}

// --- Filters ----------------------------------------------------------

type FacetKey = 'type' | 'status' | 'vendor' | 'asset'
const FACETS: FacetKey[] = ['status', 'type', 'vendor', 'asset']

const facetDefs = computed<AddFilterFacet[]>(() =>
  FACETS.map((key) => ({ key, label: t(`asset-records-filter-${key}`), kind: 'multi' })),
)

const assetOptions = computed(() =>
  [...(facetsQuery.data.value?.assets ?? [])].sort((a, b) =>
    (a.asset.asset_tag ?? a.asset.name).localeCompare(b.asset.asset_tag ?? b.asset.name),
  ),
)

function optionsFor(key: string): FilterOption[] {
  if (key === 'status') {
    return [
      { value: 'open', label: t('asset-timeline-in-progress') },
      { value: 'completed', label: t('asset-records-filter-status-completed') },
    ]
  }
  if (key === 'type') {
    return types.value
      .filter((type) => !type.archived_at || query.type.has(type.slug))
      .map((type) => ({ value: type.slug, label: typeLabel(type.slug) }))
  }
  if (key === 'vendor') {
    return (facetsQuery.data.value?.vendors ?? []).map(({ vendor, count }) => ({
      value: vendor,
      label: vendor,
      hint: t('asset-records-filter-count', { count }),
    }))
  }
  return assetOptions.value.map(({ asset, count }) => ({
    value: String(asset.id),
    label: asset.asset_tag ? `${asset.asset_tag}  ${asset.name}` : asset.name,
    hint: t('asset-records-filter-count', { count }),
  }))
}
const selectedFor = (key: string): Set<string> => query[key as FacetKey] as Set<string>
function toggle(key: string, value: string) {
  const set = selectedFor(key)
  if (!set.delete(value)) set.add(value as StatusValue)
}
const clearFacet = (key: string) => selectedFor(key).clear()

/** What a pill says for one chosen value: an asset by its tag. */
function pillValue(key: FacetKey, value: string, options: FilterOption[]): string {
  if (key === 'asset') {
    const asset = assetOptions.value.find((a) => String(a.asset.id) === value)?.asset
    return asset ? (asset.asset_tag ?? asset.name) : `#${value}`
  }
  return options.find((o) => o.value === value)?.label ?? value
}

const pills = computed(() =>
  FACETS.filter((key) => selectedFor(key).size > 0).map((key) => {
    const options = optionsFor(key)
    const selected = selectedFor(key)
    const labels = [...selected].map((value) => pillValue(key, value, options))
    return {
      facet: key,
      label: t(`asset-records-filter-${key}`),
      options,
      selected,
      valueSummary: labels.length > 2 ? `${labels.slice(0, 2).join(', ')} +${labels.length - 2}` : labels.join(', '),
    }
  }),
)
const activeFacets = computed(() => pills.value.map((pill) => pill.facet))

function setPreset(preset: PeriodPreset) {
  query.period = preset
  query.from = ''
  query.to = ''
}
function setRangeEnd(end: 'from' | 'to', day: string) {
  // Coming from a named period, the range starts from nothing.
  if (query.period !== 'custom') {
    query.from = ''
    query.to = ''
  }
  query[end] = day
  query.period = query.from || query.to ? 'custom' : ''
}
function clearPeriod() {
  query.period = ''
  query.from = ''
  query.to = ''
}

// Phones search from the app's bar above the bottom nav.
const mobileSearch = useMobileSearch()
onMounted(() =>
  mobileSearch.registerMobileSearch({
    searchQuery: query.q,
    placeholder: t('asset-records-search-placeholder'),
    showCreateButton: false,
    onSearchUpdate: (value) => (query.q = value),
  }),
)
onUnmounted(mobileSearch.deregisterMobileSearch)
watch(
  () => query.q,
  (value) => mobileSearch.updateSearchQuery(value),
)

// --- More: grouping, export, import -----------------------------------

const moreRef = ref<HTMLElement | null>(null)
const moreOpen = ref(false)
const moreAnchor = computed<PopoverAnchor>(() => ({ type: 'element', element: () => moreRef.value }))
/** Importing is a workspace admin's job. */
const canImport = computed(() => auth.isAdmin)
const moreItems = computed<MenuItem[]>(() => [
  { id: 'heading', label: t('list-grouping-trigger'), heading: true },
  ...GROUP_AXES.map((axis) => ({
    id: `group:${axis}`,
    label: t(`asset-records-group-${axis}`),
    checked: query.group === axis,
  })),
  {
    id: 'export',
    label: t('asset-records-export-csv'),
    icon: 'download' as const,
    trailing: t('asset-records-count', { count: total.value }),
    disabled: total.value === 0,
    divider: true,
  },
  ...(canImport.value ? [{ id: 'import', label: t('asset-records-import'), icon: 'upload' as const }] : []),
])

async function exportCsv() {
  // The whole filtered set, in the list's order, one row per record.
  const count = total.value
  try {
    const saved = await downloadAssetRecordsCsv(filter.value)
    // One export holds so many records; past that it says how many it has.
    if (saved.rows < saved.total) {
      toasts.warning(t('asset-records-export-capped', { rows: saved.rows, total: saved.total }))
    } else {
      toasts.success(t('asset-records-export-started', { count }))
    }
  } catch (error) {
    toasts.error(extractErrorMessage(error, t('asset-records-export-failed')))
  }
}

function onMore(id: string) {
  moreOpen.value = false
  if (id.startsWith('group:')) query.group = id.slice(6) as GroupAxis
  else if (id === 'export') void exportCsv()
  else if (id === 'import') importHistory()
}
function importHistory() {
  void router.push({ path: '/admin/data-import/csv', query: { type: 'asset_records' } })
}

// --- One record -------------------------------------------------------

// A link can name a record that isn't on a loaded page: read it alone.
const loadedRecord = computed(() =>
  query.record ? (records.value.find((r) => r.id === query.record) ?? null) : null,
)
const recordQuery = useWorkspaceQuery({
  key: () => [...assetRecordKeys.listFiltered({ record: query.record ? [query.record] : [] })],
  query: () => assetRecordService.listAll({ record: query.record ? [query.record] : [], limit: 1 }),
  enabled: () => query.record !== null && loadedRecord.value === null && listQuery.data.value !== undefined,
})
const viewing = computed<AssetRecordListRow | null>(
  () => loadedRecord.value ?? (query.record ? (recordQuery.data.value?.records[0] ?? null) : null),
)
const open = (record: AssetRecordListRow) => (query.record = record.id)

const rowTitle = (record: AssetRecordListRow) => record.summary ?? typeLabel(record.record_type)
const inProgress = (record: AssetRecordListRow) => (record.completed_on === null ? t('asset-timeline-in-progress') : null)
</script>

<template>
  <div class="h-full">
    <ListPageLayout
      :items="records"
      :total-items="total"
      :is-first-load="firstLoad"
      :is-background-refresh="false"
      :is-loading-more="loadingMore"
      :error="loadError"
      :mobile-slot-active="true"
      hide-desktop-search
      no-pull-to-refresh
      @retry="listQuery.refetch()"
    >
      <template #view-tabs>
        <AssetViewTabs />
      </template>

      <template #filters>
        <!-- What you are looking at. One group: it shares the tabs' line
             when it has 30rem beside them, else takes the next line whole. -->
        <div v-if="hasAny" class="flex min-w-0 flex-1 basis-[30rem] items-start gap-2" data-records-toolbar>
          <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
            <!-- Phones search from the app's bar above the bottom nav. -->
            <div class="hidden sm:flex w-64 max-w-full">
              <DebouncedSearchInput v-model="query.q" :placeholder="t('asset-records-search-placeholder')" />
            </div>
            <!-- Add filter keeps one place, straight after search; what is
                 set follows it, the period first. -->
            <AddFilterMenu
              :facets="facetDefs"
              :active-facets="activeFacets"
              :options-for="optionsFor"
              :selected-for="selectedFor"
              :text-value-for="() => ''"
              @toggle="toggle"
              @clear="clearFacet"
            />

            <PeriodPill
              :period="query.period"
              :from="query.from"
              :to="query.to"
              :max="today"
              @set-preset="setPreset"
              @set-from="(day: string) => setRangeEnd('from', day)"
              @set-to="(day: string) => setRangeEnd('to', day)"
              @clear="clearPeriod"
            />
            <FilterPill
              v-for="pill in pills"
              :key="pill.facet"
              :facet="pill.facet"
              kind="multi"
              :label="pill.label"
              :value-summary="pill.valueSummary"
              :options="pill.options"
              :selected="pill.selected"
              @toggle="(v: string) => toggle(pill.facet, v)"
              @clear="clearFacet(pill.facet)"
              @remove="clearFacet(pill.facet)"
            />
            <!-- Grouping wears the pills' chrome while it is on, so the row
                 says everything that shapes the list. -->
            <div
              v-if="grouped"
              class="inline-flex items-center h-6 rounded-md border border-accent/40 bg-accent/10 text-accent text-2xs overflow-hidden"
            >
              <span class="inline-flex items-center gap-1 pl-2 pr-1.5">
                <span class="font-medium">{{ t('list-grouping-trigger') }}:</span>
                <span>{{ t(`asset-records-group-${query.group}`) }}</span>
              </span>
              <button
                type="button"
                class="inline-flex items-center justify-center h-full pr-1.5 pl-0.5 hover:bg-accent/15 transition-colors"
                :aria-label="t('asset-records-group-clear')"
                :title="t('asset-records-group-clear')"
                @click="query.group = 'none'"
              >
                <Icon name="close" class="w-3 h-3" />
              </button>
            </div>
          </div>

          <!-- Grouping, export and import: one menu, on the Cost edge, at
               every width. It keeps the first line however the filters wrap. -->
          <div class="flex shrink-0 items-center gap-1 min-h-6 sm:min-h-[2.125rem]">
            <span ref="moreRef" class="inline-flex">
              <IconButton
                :label="t('asset-records-more')"
                icon="moreHorizontal"
                size="sm"
                :aria-expanded="moreOpen"
                aria-haspopup="menu"
                @click="moreOpen = !moreOpen"
              />
            </span>
            <ResponsiveMenu
              :open="moreOpen"
              :anchor="moreAnchor"
              :title="t('asset-records-more')"
              placement="bottom-end"
              popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[14rem] py-1"
              @close="moreOpen = false"
            >
              <MenuList :items="moreItems" @select="onMore" />
            </ResponsiveMenu>
          </div>
        </div>

        <!-- What the filtered set is. The count sits on the left edge and
             the totals on the Cost column's edge, workspace currency last. -->
        <div
          v-if="hasAny && first"
          class="basis-full flex flex-wrap items-baseline justify-between gap-x-4 gap-y-0.5 text-xs"
          data-records-summary
        >
          <span class="text-secondary tabular-nums">
            {{ t('asset-records-count', { count: total }) }}<template v-if="openCount > 0">, {{ t('asset-records-open-count', { count: openCount }) }}</template>
          </span>
          <span v-if="totals.length" class="ml-auto flex items-baseline gap-3 tabular-nums">
            <span class="text-tertiary">{{ t('asset-records-total-label') }}</span>
            <span
              v-for="item in totals"
              :key="item.currency"
              :class="item.currency === currency ? 'font-medium text-primary' : 'text-secondary'"
            >{{ totalLabel(item) }}</span>
          </span>
        </div>
      </template>

      <template #empty-state>
        <EmptyState
          v-if="!hasAny"
          icon="clipboardCheck"
          :title="t('asset-records-empty-title')"
          :description="t('asset-records-empty-description')"
        >
          <template v-if="canImport" #actions>
            <Button icon="upload" @click="importHistory">{{ t('asset-records-import') }}</Button>
          </template>
        </EmptyState>
        <EmptyState
          v-else
          icon="search"
          :title="t('asset-records-no-match-title')"
          :description="t('asset-records-no-match-description')"
        >
          <template #actions>
            <Button variant="secondary" @click="clearFilters(query)">{{ t('asset-records-clear-filters') }}</Button>
          </template>
        </EmptyState>
      </template>

      <template #desktop>
        <div ref="listBox">
          <DataTable
            :columns="columns"
            :data="showGroups ? [] : records"
            :buckets="buckets"
            :is-collapsed="(key: string) => collapsed.has(key)"
            :selected-items="[]"
            :selectable="false"
            :sort-field="query.sort"
            :sort-direction="query.dir"
            row-class="h-9"
            cell-padding="px-2 py-1.5"
            @update:sort="onSort"
            @row-click="open"
            @toggle-bucket="toggleBucket"
          >
            <template #bucket-meta="{ bucket }">
              <span class="flex items-baseline gap-3 text-xs tabular-nums">
                <span
                  v-for="item in (bucket as RecordBucket).totals"
                  :key="item.currency"
                  :class="item.currency === currency ? 'font-medium text-primary' : 'text-secondary'"
                >{{ totalLabel(item) }}</span>
              </span>
            </template>

            <template #cell-occurred_on="{ item }">
              <span class="text-xs text-secondary tabular-nums whitespace-nowrap">{{ formatDay(item.occurred_on) }}</span>
            </template>
            <template #cell-asset="{ item }">
              <span class="flex min-w-0 items-baseline gap-2">
                <span v-if="item.asset.asset_tag" class="shrink-0 font-mono text-xs text-primary">{{ item.asset.asset_tag }}</span>
                <span
                  v-if="showAssetName || !item.asset.asset_tag"
                  class="truncate text-xs"
                  :class="item.asset.asset_tag ? 'text-tertiary' : 'text-primary'"
                >{{ item.asset.name }}</span>
              </span>
            </template>
            <template #cell-record_type="{ item }">
              <span class="truncate text-xs text-secondary">{{ typeLabel(item.record_type) }}</span>
            </template>
            <template #cell-summary="{ item }">
              <span class="flex min-w-0 flex-1 items-baseline gap-2">
                <span class="truncate text-sm" :class="item.summary ? 'text-primary' : 'text-tertiary'">{{ rowTitle(item) }}</span>
                <span v-if="inProgress(item)" class="ml-auto shrink-0 text-xs font-medium text-primary whitespace-nowrap">{{ inProgress(item) }}</span>
              </span>
            </template>
            <template #cell-vendor="{ item }">
              <span class="truncate text-xs text-secondary">{{ item.vendor }}</span>
            </template>
            <template #cell-reference="{ item }">
              <span class="truncate font-mono text-xs text-secondary">{{ item.reference }}</span>
            </template>
            <template #cell-cost="{ item }">
              <span v-if="item.cost" class="text-sm text-primary tabular-nums whitespace-nowrap">{{ costLabel(item.cost) }}</span>
            </template>
          </DataTable>
        </div>
        <div v-if="remaining > 0" class="px-4 py-2 border-t border-default">
          <button
            type="button"
            class="text-xs font-medium text-accent hover:underline disabled:opacity-50"
            :disabled="loadingMore"
            @click="loadMore"
          >
            {{ t('asset-records-show-more', { count: Math.min(PAGE, remaining) }) }}
          </button>
        </div>
      </template>

      <!-- Below 1024px: two-line rows, or the same groups and what each
           adds up to. -->
      <template #mobile>
        <template v-if="showGroups">
          <template v-for="bucket in buckets" :key="bucket.key">
            <button
              type="button"
              class="sticky top-0 z-10 flex w-full items-baseline gap-2 border-t border-default bg-surface-alt px-4 py-1.5 text-left"
              :aria-expanded="!collapsed.has(bucket.key)"
              @click="toggleBucket(bucket.key)"
            >
              <span class="min-w-0 truncate text-xs font-medium text-primary">{{ bucket.label }}</span>
              <span class="text-3xs text-tertiary tabular-nums">{{ bucket.count ?? bucket.items.length }}</span>
              <span class="ml-auto flex shrink-0 items-baseline gap-3 text-xs tabular-nums">
                <span
                  v-for="item in bucket.totals"
                  :key="item.currency"
                  :class="item.currency === currency ? 'font-medium text-primary' : 'text-secondary'"
                >{{ totalLabel(item) }}</span>
              </span>
            </button>
            <template v-if="!collapsed.has(bucket.key)">
              <AssetRecordMobileRow
                v-for="item in bucket.items"
                :key="item.id"
                :record="item"
                :title="rowTitle(item)"
                :day="formatDay(item.occurred_on)"
                :type-label="query.group === 'type' ? '' : typeLabel(item.record_type)"
                :vendor="query.group === 'vendor' ? null : item.vendor"
                :asset-tag="query.group === 'asset' ? '' : (item.asset.asset_tag ?? item.asset.name)"
                :cost="costLabel(item.cost)"
                :in-progress="inProgress(item)"
                @open="open"
              />
            </template>
          </template>
        </template>
        <template v-else>
          <AssetRecordMobileRow
            v-for="item in records"
            :key="item.id"
            :record="item"
            :title="rowTitle(item)"
            :day="formatDay(item.occurred_on)"
            :type-label="typeLabel(item.record_type)"
            :vendor="item.vendor"
            :asset-tag="item.asset.asset_tag ?? item.asset.name"
            :cost="costLabel(item.cost)"
            :in-progress="inProgress(item)"
            @open="open"
          />
        </template>
        <div v-if="remaining > 0" class="px-4 py-2.5 border-t border-default">
          <button
            type="button"
            class="text-xs font-medium text-accent hover:underline disabled:opacity-50"
            :disabled="loadingMore"
            @click="loadMore"
          >
            {{ t('asset-records-show-more', { count: Math.min(PAGE, remaining) }) }}
          </button>
        </div>
      </template>
    </ListPageLayout>

    <!-- The record, opened from its row: the same view the asset's
         timeline opens, under the asset it belongs to. -->
    <Modal
      :show="viewing !== null"
      :title="viewing ? t('asset-record-detail-title', { type: typeLabel(viewing.record_type), date: formatDay(viewing.occurred_on) }) : ''"
      size="sm"
      @close="query.record = null"
    >
      <div v-if="viewing" class="flex flex-col gap-3">
        <RouterLink
          :to="`/assets/${viewing.asset.id}`"
          class="group flex items-baseline gap-2 text-xs-plus leading-5"
          data-record-asset
        >
          <span v-if="viewing.asset.asset_tag" class="shrink-0 font-mono text-primary">{{ viewing.asset.asset_tag }}</span>
          <span class="min-w-0 truncate text-secondary underline decoration-strong underline-offset-2 group-hover:decoration-current">{{ viewing.asset.name }}</span>
        </RouterLink>
        <AssetRecordDetail :record="viewing" :type="typeBySlug.get(viewing.record_type ?? '')" />
      </div>
    </Modal>
  </div>
</template>
