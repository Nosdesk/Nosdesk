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
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  assetsService,
  type AssetPlannerRow,
  type OsFamily,
  type WarrantyBucket,
} from '@/services/assetsService'

const router = useRouter()
const rows = ref<AssetPlannerRow[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

async function load(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    rows.value = await assetsService.planner()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load assets'
  } finally {
    loading.value = false
  }
}

onMounted(load)

// ---------------------------------------------------------------
// Group-by axis. The three axes cover the three planning lenses:
// "what runs on Windows?", "what's about to fall out of warranty?",
// "what's non-compliant under MDM?".
// ---------------------------------------------------------------
type GroupAxis = 'os_family' | 'warranty_bucket' | 'compliance_state'
const axis = ref<GroupAxis>('os_family')

// ---------------------------------------------------------------
// Sidebar filters. Each axis exposes its known buckets; clicking
// one collapses the visible set. Free-text search narrows by
// name / hostname / manufacturer / model.
// ---------------------------------------------------------------
const osFilter = ref<Set<OsFamily>>(new Set())
const warrantyFilter = ref<Set<WarrantyBucket>>(new Set())
const complianceFilter = ref<Set<string>>(new Set())
const search = ref('')

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

const OS_LABELS: Record<OsFamily, string> = {
  windows: 'Windows',
  macos: 'macOS',
  linux: 'Linux',
  ios: 'iOS',
  android: 'Android',
  other: 'Other',
}
const WARRANTY_LABELS: Record<WarrantyBucket, string> = {
  expired: 'Expired',
  expiring_30d: 'Expiring in 30 days',
  expiring_90d: 'Expiring in 90 days',
  active: 'Active',
  unknown: 'Unknown',
}
const WARRANTY_ORDER: WarrantyBucket[] = [
  'expired',
  'expiring_30d',
  'expiring_90d',
  'active',
  'unknown',
]

const buckets = computed<Bucket[]>(() => {
  const out = new Map<string, Bucket>()
  for (const r of filteredRows.value) {
    let key: string
    let label: string
    if (axis.value === 'os_family') {
      key = r.os_family
      label = OS_LABELS[r.os_family]
    } else if (axis.value === 'warranty_bucket') {
      key = r.warranty_bucket
      label = WARRANTY_LABELS[r.warranty_bucket]
    } else {
      key = r.compliance_state ?? '__unknown__'
      label = r.compliance_state ?? 'Unknown'
    }
    if (!out.has(key)) out.set(key, { key, label, rows: [] })
    out.get(key)!.rows.push(r)
  }
  // Stable ordering per axis: warranty has a natural severity
  // order, OS uses the labels map order, compliance is alphabetic.
  if (axis.value === 'warranty_bucket') {
    return WARRANTY_ORDER.flatMap((k) => {
      const b = out.get(k)
      return b ? [b] : []
    })
  }
  if (axis.value === 'os_family') {
    return Object.keys(OS_LABELS).flatMap((k) => {
      const b = out.get(k)
      return b ? [b] : []
    })
  }
  return Array.from(out.values()).sort((a, b) => a.label.localeCompare(b.label))
})

// All known facets for the sidebar — derived from the data so the
// list stays accurate as devices are added / removed.
const knownOsFamilies = computed<OsFamily[]>(() => {
  const set = new Set<OsFamily>()
  for (const r of rows.value) set.add(r.os_family)
  return (Object.keys(OS_LABELS) as OsFamily[]).filter((k) => set.has(k))
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

function warrantyClass(b: WarrantyBucket): string {
  if (b === 'expired') return 'bg-rose-500/20 text-rose-700 dark:text-rose-300'
  if (b === 'expiring_30d') return 'bg-amber-500/30 text-amber-800 dark:text-amber-200'
  if (b === 'expiring_90d') return 'bg-amber-500/15 text-amber-700 dark:text-amber-300'
  if (b === 'active') return 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300'
  return 'bg-surface-hover text-tertiary'
}

function openDevice(id: number): void {
  router.push(`/devices/${id}`)
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
    <!-- Header. On md+ everything sits on one row; below md the
         title stacks above the controls and the search expands to
         full width so it's tappable on a phone. -->
    <header
      class="flex flex-col md:flex-row md:items-center md:justify-between gap-3 px-4 md:px-6 py-3 md:py-4 border-b border-subtle bg-app"
    >
      <div>
        <h1 class="text-xl font-semibold text-primary">Assets</h1>
        <p class="text-xs text-tertiary mt-0.5">
          Plan rollouts by OS, warranty, or compliance state.
        </p>
      </div>
      <div class="flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-3">
        <input
          v-model="search"
          type="text"
          placeholder="Search by name, hostname, model…"
          class="bg-surface border border-subtle rounded-md text-sm px-3 py-1.5 text-primary w-full sm:w-64"
        />
        <label class="flex items-center gap-2 text-xs text-secondary">
          <span class="shrink-0">Group by</span>
          <select
            v-model="axis"
            class="bg-surface border border-subtle rounded-md text-xs px-2 py-1 text-primary flex-1 sm:flex-initial"
          >
            <option value="os_family">OS family</option>
            <option value="warranty_bucket">Warranty</option>
            <option value="compliance_state">Compliance</option>
          </select>
        </label>
      </div>
    </header>

    <div v-if="loading" class="flex-1 flex items-center justify-center text-tertiary text-sm">
      Loading assets…
    </div>
    <div v-else-if="error" class="flex-1 flex items-center justify-center text-status-error text-sm">
      {{ error }}
    </div>
    <!-- Body. md+ uses the side-by-side grid (filter sidebar then
         planner). Below md the sidebar stacks above the planner
         with a capped height so it doesn't dominate the viewport.
         The planner itself horizontally-scrolls within whatever
         column space is left, which is what we want — kanban
         columns stay readable rather than crushing. -->
    <div
      v-else
      class="flex-1 min-h-0 flex flex-col md:grid"
      style="grid-template-columns: 14rem 1fr"
    >
      <!-- Filter sidebar -->
      <aside
        class="border-b md:border-b-0 md:border-r border-subtle bg-surface overflow-y-auto p-4 flex flex-col gap-5 md:flex-col max-h-48 md:max-h-none"
      >
        <div class="flex items-center justify-between">
          <h2 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary">Filters</h2>
          <button
            v-if="activeFilterCount > 0"
            type="button"
            class="text-[11px] text-tertiary hover:text-primary"
            @click="clearAllFilters"
          >Clear ({{ activeFilterCount }})</button>
        </div>

        <section v-if="knownOsFamilies.length > 0" class="flex flex-col gap-1">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary">OS</h3>
          <button
            v-for="os in knownOsFamilies"
            :key="os"
            type="button"
            class="text-xs text-left px-2 py-1 rounded-md hover:bg-surface-hover transition-colors"
            :class="osFilter.has(os) ? 'bg-accent/10 text-accent font-medium' : 'text-secondary'"
            @click="toggleSetMember(osFilter, os)"
          >
            {{ OS_LABELS[os] }}
          </button>
        </section>

        <section v-if="knownWarrantyBuckets.length > 0" class="flex flex-col gap-1">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary">Warranty</h3>
          <button
            v-for="b in knownWarrantyBuckets"
            :key="b"
            type="button"
            class="text-xs text-left px-2 py-1 rounded-md hover:bg-surface-hover transition-colors"
            :class="warrantyFilter.has(b) ? 'bg-accent/10 text-accent font-medium' : 'text-secondary'"
            @click="toggleSetMember(warrantyFilter, b)"
          >
            {{ WARRANTY_LABELS[b] }}
          </button>
        </section>

        <section v-if="knownComplianceStates.length > 0" class="flex flex-col gap-1">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary">Compliance</h3>
          <button
            v-for="c in knownComplianceStates"
            :key="c"
            type="button"
            class="text-xs text-left px-2 py-1 rounded-md hover:bg-surface-hover transition-colors"
            :class="complianceFilter.has(c) ? 'bg-accent/10 text-accent font-medium' : 'text-secondary'"
            @click="toggleSetMember(complianceFilter, c)"
          >
            {{ c }}
          </button>
        </section>

        <p class="text-[11px] text-tertiary mt-2">
          {{ filteredRows.length }} of {{ rows.length }} device{{ rows.length === 1 ? '' : 's' }}
        </p>
      </aside>

      <!-- Group columns -->
      <div class="overflow-auto p-4">
        <div
          v-if="buckets.length === 0"
          class="text-tertiary text-sm italic p-8 text-center"
        >No devices match the current filters.</div>

        <div v-else class="flex gap-4 min-h-full">
          <section
            v-for="b in buckets"
            :key="b.key"
            class="w-72 flex-shrink-0 flex flex-col bg-surface rounded-lg border border-default"
          >
            <header class="flex items-center justify-between px-4 py-3 bg-surface-alt border-b border-subtle">
              <h3 class="text-sm font-semibold text-primary truncate">{{ b.label }}</h3>
              <span class="text-xs text-tertiary bg-surface-hover rounded-md px-2 py-1 tabular-nums">
                {{ b.rows.length }}
              </span>
            </header>

            <div class="flex-1 flex flex-col gap-2 p-2 overflow-y-auto">
              <article
                v-for="d in b.rows"
                :key="d.id"
                class="bg-app rounded-md border border-subtle hover:border-default p-3 cursor-pointer transition-colors"
                @click="openDevice(d.id)"
              >
                <header class="flex items-start justify-between gap-2 mb-1">
                  <h4 class="text-sm font-medium text-primary truncate flex-1">{{ d.name }}</h4>
                  <span
                    class="text-[10px] font-medium rounded px-1.5 py-0.5 shrink-0"
                    :class="warrantyClass(d.warranty_bucket)"
                    :title="d.warranty_end_date ? `Warranty ends ${d.warranty_end_date}` : 'No warranty data'"
                  >
                    {{ d.warranty_bucket === 'unknown' ? '—' : WARRANTY_LABELS[d.warranty_bucket] }}
                  </span>
                </header>
                <dl class="text-[11px] text-tertiary flex flex-col gap-0.5">
                  <div v-if="d.hostname" class="flex justify-between">
                    <dt>Host</dt>
                    <dd class="font-mono text-secondary truncate ml-2">{{ d.hostname }}</dd>
                  </div>
                  <div v-if="d.operating_system" class="flex justify-between">
                    <dt>OS</dt>
                    <dd class="text-secondary truncate ml-2">
                      {{ d.operating_system }}{{ d.os_version ? ` ${d.os_version}` : '' }}
                    </dd>
                  </div>
                  <div v-if="d.model || d.manufacturer" class="flex justify-between">
                    <dt>Model</dt>
                    <dd class="text-secondary truncate ml-2">
                      {{ [d.manufacturer, d.model].filter(Boolean).join(' ') }}
                    </dd>
                  </div>
                  <div v-if="d.asset_tag" class="flex justify-between">
                    <dt>Tag</dt>
                    <dd class="font-mono text-secondary ml-2">{{ d.asset_tag }}</dd>
                  </div>
                  <div v-if="d.compliance_state" class="flex justify-between">
                    <dt>Compliance</dt>
                    <dd class="text-secondary ml-2">{{ d.compliance_state }}</dd>
                  </div>
                </dl>
              </article>
            </div>
          </section>
        </div>
      </div>
    </div>
  </div>
</template>
