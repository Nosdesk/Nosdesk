<script setup lang="ts">
/**
 * Phase 9 Asset rollout planner.
 *
 * The "differentiator" view per the architecture spec: an IT team
 * uses this to plan device rollouts (replace all Win 10 boxes
 * before EOL, refresh laptops whose warranties expire in the
 * next 90 days, swap out non-compliant managed devices).
 *
 * Layout: a sidebar of filters drives a grid of device cards
 * grouped by the chosen axis. Group counts ride on the column
 * headers so capacity / scope reads at a glance.
 *
 * v1 ships with three group-by axes: OS family, warranty bucket,
 * compliance state. All data comes from existing device columns
 * plus two server-side derived buckets (os_family,
 * warranty_bucket).
 */
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import AssetViewTabs from '@/components/assets/AssetViewTabs.vue'
import AsyncBoundary from '@/components/common/AsyncBoundary.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import {
  assetsService,
  type AssetPlannerRow,
  type OsFamily,
  type WarrantyBucket,
} from '@/services/assetsService'

const router = useRouter()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

// Cache-first: the planner dataset is fetched once and filtered/grouped
// client-side, so a revisit renders instantly from cache then refreshes
// silently (SWR). Not a synced aggregate, so this is the source of truth.
const plannerQuery = useQuery({
  key: () => ['assets', 'planner'],
  query: () => assetsService.planner(),
})
const rows = computed<AssetPlannerRow[]>(() => plannerQuery.data.value ?? [])
const loadOp = computed(() => ({
  isPending: plannerQuery.asyncStatus.value === 'loading',
  isError: plannerQuery.state.value.status === 'error',
  error: plannerQuery.error.value,
}))
// True once the fetch resolves (even to an empty set), so the body's own
// empty state shows instead of the pending slot.
const hasData = computed(() => plannerQuery.data.value !== undefined)

// ---------------------------------------------------------------
// Group-by axis. The three axes cover the three planning lenses:
// "what runs on Windows?", "what's about to fall out of warranty?",
// "what's non-compliant under MDM?".
// ---------------------------------------------------------------
type GroupAxis = 'os_family' | 'warranty_bucket' | 'compliance_state'
const axis = ref<GroupAxis>('os_family')

const axisOptions = computed(() => [
  { value: 'os_family' as GroupAxis, label: t('asset-planner-axis-os') },
  { value: 'warranty_bucket' as GroupAxis, label: t('asset-planner-axis-warranty') },
  { value: 'compliance_state' as GroupAxis, label: t('asset-planner-axis-compliance') },
])

// ---------------------------------------------------------------
// Sidebar filters. Each axis exposes its known buckets; clicking
// one collapses the visible set. Free-text search narrows by
// name / hostname / manufacturer / model.
// ---------------------------------------------------------------
const osFilter = ref<Set<OsFamily>>(new Set())
const warrantyFilter = ref<Set<WarrantyBucket>>(new Set())
const complianceFilter = ref<Set<string>>(new Set())
const search = ref('')

// Mobile only: the filter sidebar collapses into a disclosure so it doesn't
// eat vertical space above the columns. Always shown at md+ (the persistent
// sidebar column), so this flag is irrelevant there.
const filtersOpen = ref(false)

function toggleSetMember<T>(s: Set<T>, value: T): void {
  if (s.has(value)) s.delete(value)
  else s.add(value)
}

const filteredRows = computed<AssetPlannerRow[]>(() => {
  const q = search.value.trim().toLowerCase()
  return rows.value.filter((r) => {
    if (osFilter.value.size > 0 && !osFilter.value.has(r.os_family)) return false
    if (warrantyFilter.value.size > 0 && !warrantyFilter.value.has(r.warranty_bucket)) return false
    if (complianceFilter.value.size > 0) {
      if (!r.compliance_state || !complianceFilter.value.has(r.compliance_state)) return false
    }
    if (q) {
      const hay = [r.name, r.hostname, r.manufacturer, r.model, r.asset_tag]
        .filter(Boolean)
        .join(' ')
        .toLowerCase()
      if (!hay.includes(q)) return false
    }
    return true
  })
})

interface Bucket {
  key: string
  label: string
  rows: AssetPlannerRow[]
}

// Stable enum-style key lists; labels are resolved through Fluent
// at render time so they follow the active locale.
const OS_KEYS: OsFamily[] = ['windows', 'macos', 'linux', 'ios', 'android', 'other']
const WARRANTY_ORDER: WarrantyBucket[] = [
  'expired',
  'expiring_30d',
  'expiring_90d',
  'active',
  'unknown',
]

const osLabels = computed<Record<OsFamily, string>>(() => ({
  windows: t('asset-planner-os-windows'),
  macos: t('asset-planner-os-macos'),
  linux: t('asset-planner-os-linux'),
  ios: t('asset-planner-os-ios'),
  android: t('asset-planner-os-android'),
  other: t('asset-planner-os-other'),
}))

const warrantyLabels = computed<Record<WarrantyBucket, string>>(() => ({
  expired: t('asset-planner-warranty-expired'),
  expiring_30d: t('asset-planner-warranty-expiring-30d'),
  expiring_90d: t('asset-planner-warranty-expiring-90d'),
  active: t('asset-planner-warranty-active'),
  unknown: t('asset-planner-warranty-unknown'),
}))

const buckets = computed<Bucket[]>(() => {
  const out = new Map<string, Bucket>()
  for (const r of filteredRows.value) {
    let key: string
    let label: string
    if (axis.value === 'os_family') {
      key = r.os_family
      label = osLabels.value[r.os_family]
    } else if (axis.value === 'warranty_bucket') {
      key = r.warranty_bucket
      label = warrantyLabels.value[r.warranty_bucket]
    } else {
      key = r.compliance_state ?? '__unknown__'
      label = r.compliance_state ?? t('asset-planner-compliance-unknown')
    }
    if (!out.has(key)) out.set(key, { key, label, rows: [] })
    out.get(key)!.rows.push(r)
  }
  // Stable ordering per axis: warranty has a natural severity
  // order, OS uses the OS_KEYS order, compliance is alphabetic.
  if (axis.value === 'warranty_bucket') {
    return WARRANTY_ORDER.flatMap((k) => {
      const b = out.get(k)
      return b ? [b] : []
    })
  }
  if (axis.value === 'os_family') {
    return OS_KEYS.flatMap((k) => {
      const b = out.get(k)
      return b ? [b] : []
    })
  }
  return Array.from(out.values()).sort((a, b) => a.label.localeCompare(b.label))
})

// All known facets for the sidebar, derived from the data so the
// list stays accurate as devices are added / removed.
const knownOsFamilies = computed<OsFamily[]>(() => {
  const set = new Set<OsFamily>()
  for (const r of rows.value) set.add(r.os_family)
  return OS_KEYS.filter((k) => set.has(k))
})

const knownWarrantyBuckets = computed<WarrantyBucket[]>(() => {
  const set = new Set<WarrantyBucket>()
  for (const r of rows.value) set.add(r.warranty_bucket)
  return WARRANTY_ORDER.filter((k) => set.has(k))
})

const knownComplianceStates = computed<string[]>(() => {
  const set = new Set<string>()
  for (const r of rows.value) {
    if (r.compliance_state) set.add(r.compliance_state)
  }
  return Array.from(set).sort()
})

// Per-facet totals across the full dataset, surfaced on each filter chip so
// capacity reads at a glance ("Windows 42") without applying the filter.
const osCounts = computed<Record<string, number>>(() => {
  const m: Record<string, number> = {}
  for (const r of rows.value) m[r.os_family] = (m[r.os_family] ?? 0) + 1
  return m
})
const warrantyCounts = computed<Record<string, number>>(() => {
  const m: Record<string, number> = {}
  for (const r of rows.value) m[r.warranty_bucket] = (m[r.warranty_bucket] ?? 0) + 1
  return m
})
const complianceCounts = computed<Record<string, number>>(() => {
  const m: Record<string, number> = {}
  for (const r of rows.value) {
    if (r.compliance_state) m[r.compliance_state] = (m[r.compliance_state] ?? 0) + 1
  }
  return m
})

function warrantyClass(b: WarrantyBucket): string {
  if (b === 'expired') return 'bg-rose-500/20 text-rose-700 dark:text-rose-300'
  if (b === 'expiring_30d') return 'bg-amber-500/30 text-amber-800 dark:text-amber-200'
  if (b === 'expiring_90d') return 'bg-amber-500/15 text-amber-700 dark:text-amber-300'
  if (b === 'active') return 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300'
  return 'bg-surface-hover text-tertiary'
}

function warrantyTooltip(date: string | null | undefined): string {
  return date
    ? t('asset-planner-warranty-ends', { date })
    : t('asset-planner-no-warranty-data')
}

function openDevice(id: number): void {
  router.push(`/assets/${id}`)
}

function clearAllFilters(): void {
  osFilter.value = new Set()
  warrantyFilter.value = new Set()
  complianceFilter.value = new Set()
  search.value = ''
}

const activeFilterCount = computed<number>(() =>
  osFilter.value.size + warrantyFilter.value.size + complianceFilter.value.size + (search.value ? 1 : 0),
)
</script>

<template>
  <div class="flex flex-col h-full">
    <AssetViewTabs />
    <!-- Header: title + live search + segmented group-by. One row at md+; below
         md the title stacks above full-width controls so they stay tappable. -->
    <header
      class="flex flex-col gap-3 md:flex-row md:items-start md:justify-between px-4 md:px-6 py-3 md:py-4 border-b border-subtle"
    >
      <div class="min-w-0">
        <h1 class="text-lg font-semibold text-primary">{{ $t('asset-planner-title') }}</h1>
        <p class="text-xs text-tertiary mt-0.5">{{ $t('asset-planner-subtitle') }}</p>
      </div>

      <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:gap-3 md:shrink-0">
        <DebouncedSearchInput
          v-model="search"
          :placeholder="$t('asset-planner-search-placeholder')"
          class="w-full sm:w-64 sm:grow-0"
        />
        <div class="flex items-center gap-2">
          <span class="hidden text-xs text-secondary shrink-0 sm:inline">
            {{ $t('asset-planner-group-by') }}
          </span>
          <SegmentedControl
            v-model="axis"
            :options="axisOptions"
            :aria-label="$t('asset-planner-group-by')"
            size="sm"
            class="grow sm:grow-0"
          />
          <!-- Mobile-only: collapse/expand the filter sidebar. -->
          <button
            type="button"
            class="md:hidden inline-flex items-center gap-1 h-7 px-2.5 rounded-md text-xs font-medium border border-default text-secondary transition-colors hover:bg-surface-hover hover:text-primary shrink-0"
            :class="filtersOpen ? 'bg-surface-hover text-primary' : ''"
            :aria-expanded="filtersOpen"
            @click="filtersOpen = !filtersOpen"
          >
            {{ $t('asset-planner-filters-heading') }}
            <span v-if="activeFilterCount > 0" class="tabular-nums">({{ activeFilterCount }})</span>
          </button>
        </div>
      </div>
    </header>

    <AsyncBoundary :op="loadOp" :has-data="hasData">
      <template #pending>
        <div class="flex-1 flex items-center justify-center text-tertiary text-sm">
          {{ $t('asset-planner-loading') }}
        </div>
      </template>
      <template #error="{ error: boundaryError }">
        <div class="flex-1 flex items-center justify-center text-status-error text-sm">
          {{ (boundaryError as Error)?.message ?? $t('asset-planner-load-error') }}
        </div>
      </template>
      <!-- Body: side-by-side grid at md+ (filter sidebar + planner); below md the
           sidebar is a collapsible disclosure and the columns take the rest. The
           planner scrolls horizontally so kanban columns stay readable. -->
      <div
        class="flex-1 min-h-0 flex flex-col md:grid"
        style="grid-template-columns: 15rem 1fr"
      >
        <!-- Filter sidebar: persistent at md+, collapsible below. -->
        <aside
          class="md:flex flex-col gap-4 border-b md:border-b-0 md:border-r border-subtle bg-surface overflow-y-auto p-4 max-h-[55vh] md:max-h-none"
          :class="filtersOpen ? 'flex' : 'hidden'"
        >
          <div class="flex items-center justify-between">
            <h2 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary">
              {{ $t('asset-planner-filters-heading') }}
            </h2>
            <button
              v-if="activeFilterCount > 0"
              type="button"
              class="text-[11px] text-tertiary transition-colors hover:text-primary"
              @click="clearAllFilters"
            >
              {{ $t('asset-planner-filters-clear', { count: activeFilterCount }) }}
            </button>
          </div>

          <section v-if="knownOsFamilies.length > 0" class="flex flex-col gap-0.5">
            <h3 class="mb-1 text-[10px] uppercase tracking-wide font-semibold text-tertiary">
              {{ $t('asset-planner-section-os') }}
            </h3>
            <button
              v-for="os in knownOsFamilies"
              :key="os"
              type="button"
              :aria-pressed="osFilter.has(os)"
              class="flex items-center gap-2 px-2 py-1 rounded-md text-xs text-left transition-colors"
              :class="osFilter.has(os) ? 'bg-accent/10 text-accent font-medium' : 'text-secondary hover:bg-surface-hover'"
              @click="toggleSetMember(osFilter, os)"
            >
              <span class="truncate">{{ osLabels[os] }}</span>
              <span class="ml-auto tabular-nums text-[11px] text-tertiary">{{ osCounts[os] ?? 0 }}</span>
            </button>
          </section>

          <section v-if="knownWarrantyBuckets.length > 0" class="flex flex-col gap-0.5">
            <h3 class="mb-1 text-[10px] uppercase tracking-wide font-semibold text-tertiary">
              {{ $t('asset-planner-section-warranty') }}
            </h3>
            <button
              v-for="b in knownWarrantyBuckets"
              :key="b"
              type="button"
              :aria-pressed="warrantyFilter.has(b)"
              class="flex items-center gap-2 px-2 py-1 rounded-md text-xs text-left transition-colors"
              :class="warrantyFilter.has(b) ? 'bg-accent/10 text-accent font-medium' : 'text-secondary hover:bg-surface-hover'"
              @click="toggleSetMember(warrantyFilter, b)"
            >
              <span class="truncate">{{ warrantyLabels[b] }}</span>
              <span class="ml-auto tabular-nums text-[11px] text-tertiary">{{ warrantyCounts[b] ?? 0 }}</span>
            </button>
          </section>

          <section v-if="knownComplianceStates.length > 0" class="flex flex-col gap-0.5">
            <h3 class="mb-1 text-[10px] uppercase tracking-wide font-semibold text-tertiary">
              {{ $t('asset-planner-section-compliance') }}
            </h3>
            <button
              v-for="c in knownComplianceStates"
              :key="c"
              type="button"
              :aria-pressed="complianceFilter.has(c)"
              class="flex items-center gap-2 px-2 py-1 rounded-md text-xs text-left transition-colors"
              :class="complianceFilter.has(c) ? 'bg-accent/10 text-accent font-medium' : 'text-secondary hover:bg-surface-hover'"
              @click="toggleSetMember(complianceFilter, c)"
            >
              <span class="truncate">{{ c }}</span>
              <span class="ml-auto tabular-nums text-[11px] text-tertiary">{{ complianceCounts[c] ?? 0 }}</span>
            </button>
          </section>

          <p class="mt-auto pt-2 text-[11px] text-tertiary">
            {{ $t('asset-planner-count', { visible: filteredRows.length, total: rows.length }) }}
          </p>
        </aside>

      <!-- Group columns -->
      <div class="overflow-auto p-4">
        <div
          v-if="buckets.length === 0"
          class="flex h-full items-center justify-center p-8 text-center text-sm italic text-tertiary"
        >
          {{ $t('asset-planner-empty') }}
        </div>

        <div v-else class="flex gap-4 min-h-full">
          <section
            v-for="b in buckets"
            :key="b.key"
            class="w-72 flex-shrink-0 flex flex-col bg-surface rounded-lg border border-default"
          >
            <header class="flex items-center justify-between gap-2 px-3 py-2.5 bg-surface-alt border-b border-subtle rounded-t-lg">
              <h3 class="text-sm font-semibold text-primary truncate">{{ b.label }}</h3>
              <span class="shrink-0 text-xs text-secondary bg-surface-hover rounded-md px-1.5 py-0.5 tabular-nums">
                {{ b.rows.length }}
              </span>
            </header>

            <div class="flex-1 flex flex-col gap-2 p-2 overflow-y-auto">
              <article
                v-for="d in b.rows"
                :key="d.id"
                class="bg-app rounded-md border border-subtle hover:border-strong hover:bg-surface-hover/40 p-3 cursor-pointer transition-colors"
                @click="openDevice(d.id)"
              >
                <header class="flex items-start justify-between gap-2 mb-1">
                  <h4 class="text-sm font-medium text-primary truncate flex-1">{{ d.name }}</h4>
                  <span
                    class="text-[10px] font-medium rounded px-1.5 py-0.5 shrink-0"
                    :class="warrantyClass(d.warranty_bucket)"
                    :title="warrantyTooltip(d.warranty_end_date)"
                  >
                    {{ d.warranty_bucket === 'unknown' ? $t('asset-planner-warranty-unknown-short') : warrantyLabels[d.warranty_bucket] }}
                  </span>
                </header>
                <dl class="text-[11px] text-tertiary flex flex-col gap-0.5">
                  <div v-if="d.hostname" class="flex justify-between">
                    <dt>{{ $t('asset-planner-card-host') }}</dt>
                    <dd class="font-mono text-secondary truncate ml-2">{{ d.hostname }}</dd>
                  </div>
                  <div v-if="d.operating_system" class="flex justify-between">
                    <dt>{{ $t('asset-planner-card-os') }}</dt>
                    <dd class="text-secondary truncate ml-2">
                      {{ d.operating_system }}{{ d.os_version ? ` ${d.os_version}` : '' }}
                    </dd>
                  </div>
                  <div v-if="d.model || d.manufacturer" class="flex justify-between">
                    <dt>{{ $t('asset-planner-card-model') }}</dt>
                    <dd class="text-secondary truncate ml-2">
                      {{ [d.manufacturer, d.model].filter(Boolean).join(' ') }}
                    </dd>
                  </div>
                  <div v-if="d.asset_tag" class="flex justify-between">
                    <dt>{{ $t('asset-planner-card-tag') }}</dt>
                    <dd class="font-mono text-secondary ml-2">{{ d.asset_tag }}</dd>
                  </div>
                  <div v-if="d.compliance_state" class="flex justify-between">
                    <dt>{{ $t('asset-planner-card-compliance') }}</dt>
                    <dd class="text-secondary ml-2">{{ d.compliance_state }}</dd>
                  </div>
                </dl>
              </article>
            </div>
          </section>
        </div>
      </div>
    </div>
    </AsyncBoundary>
  </div>
</template>
