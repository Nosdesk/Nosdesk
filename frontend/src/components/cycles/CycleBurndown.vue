<script setup lang="ts">
/**
 * Cycle burndown summary. For completed cycles the API returns the
 * frozen completion_snapshot; for planned/active cycles it computes
 * the same shape on the fly. We render both through the same UI:
 * total / completed / by-category breakdown plus a "% done" bar
 * and a "days remaining" line when the cycle has an end date.
 *
 * The line-chart-style burndown over time lands once cycles emit
 * sync_actions; until then this snapshot view is the honest
 * representation of what we can derive.
 */
import { computed, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { cyclesService, type CycleStats, type BurnupSeries } from '@/services/cyclesService'
import type { Cycle } from '@/services/cyclesService'
import { formatDateTime } from '@/utils/dateUtils'
import { WORKFLOW_CATEGORIES, coarseStatusBucket, type WorkflowStateCategory } from '@/types/workflow'
import { cycleHealth, cycleHealthPresentation } from '@/utils/cycleHealth'
import StatusPill from '@/components/common/StatusPill.vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import Icon from '@/components/common/Icon.vue'
import CycleBurnupChart from './CycleBurnupChart.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  cycle: Cycle
  /** When set, the card header becomes a link to this route (e.g. the
   *  cycle's board), so the featured active cycle is tappable. */
  to?: string
}>()

const stats = ref<CycleStats | null>(null)
const burnup = ref<BurnupSeries | null>(null)
const isLoading = ref(false)
const error = ref<string | null>(null)

const isFrozen = computed<boolean>(() => props.cycle.state === 'completed')

// Burnup only makes sense for a live cycle with a placed timeline:
// frozen cycles keep the snapshot view (no daily series is stored),
// and without start + end dates there's nothing to plot against.
const showBurnup = computed<boolean>(
  () => !isFrozen.value && !!props.cycle.start_at && !!props.cycle.end_at,
)

async function load(): Promise<void> {
  isLoading.value = true
  error.value = null
  try {
    stats.value = await cyclesService.stats(props.cycle.uuid)
    burnup.value = showBurnup.value ? await cyclesService.burnup(props.cycle.uuid) : null
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('tickets-cycle-burndown-load-error')
  } finally {
    isLoading.value = false
  }
}

watch(() => props.cycle.uuid, () => { void load() }, { immediate: true })

const completionPct = computed<number>(() => {
  if (!stats.value || stats.value.tickets === 0) return 0
  return Math.round((stats.value.completed / stats.value.tickets) * 100)
})

const daysRemaining = computed<number | null>(() => {
  if (!props.cycle.end_at) return null
  const ms = new Date(props.cycle.end_at).getTime() - Date.now()
  return Math.max(0, Math.ceil(ms / 86_400_000))
})

// On-track / at-risk / behind, from completed-vs-elapsed pace. The frozen
// snapshot of a completed cycle reads as "complete"; we only surface the
// pill once stats have loaded so it doesn't flash a misleading state.
const health = computed(() => {
  const s = stats.value
  if (!s) return null
  const h = cycleHealth({
    total: s.tickets,
    completed: s.completed,
    startAt: props.cycle.start_at,
    endAt: props.cycle.end_at,
  })
  const { tone, labelKey } = cycleHealthPresentation(h)
  return { tone, label: t(labelKey) }
})

const sortedCategories = computed<[string, number][]>(() => {
  if (!stats.value) return []
  // Order by the canonical workflow progression (triage → done), not
  // alphabetically, so the breakdown reads in the order work flows.
  const order = WORKFLOW_CATEGORIES as readonly string[]
  const rank = (c: string) => {
    const i = order.indexOf(c)
    return i === -1 ? order.length : i
  }
  return Object.entries(stats.value.by_category).sort(([a], [b]) => rank(a) - rank(b))
})

const categoryLabels = computed<Record<string, string>>(() => ({
  triage: t('tickets-cycle-burndown-cat-triage'),
  backlog: t('tickets-cycle-burndown-cat-backlog'),
  active: t('tickets-cycle-burndown-cat-active'),
  in_review: t('tickets-cycle-burndown-cat-in-review'),
  done: t('tickets-cycle-burndown-cat-done'),
  cancelled: t('tickets-cycle-burndown-cat-cancelled'),
}))

// Color-code the breakdown by the category's coarse bucket (the same
// amber / blue / green the status dots use), with a proportion bar so
// the distribution reads at a glance. Paired with the StatusIndicator
// dot, which falls back to shapes in colour-blind mode, so colour is
// never the only signal.
function categoryBarClass(cat: string): string {
  switch (coarseStatusBucket(cat as WorkflowStateCategory)) {
    case 'open':
      return 'bg-status-open'
    case 'in-progress':
      return 'bg-status-in-progress'
    default:
      return 'bg-status-closed'
  }
}
function categoryPct(count: number): number {
  const total = stats.value?.tickets ?? 0
  return total > 0 ? Math.round((count / total) * 100) : 0
}
</script>

<template>
  <div class="rounded-md border border-subtle bg-surface p-4">
    <!-- Clickable top summary (header + headline metrics + progress): a
         link to the cycle's board when `to` is set, static otherwise.
         The burnup chart and breakdown below stay non-navigating. -->
    <component
      :is="to ? RouterLink : 'div'"
      :to="to || undefined"
      class="block rounded"
      :class="
        to
          ? 'group -mx-1 px-1 py-1 cursor-pointer transition-colors hover:bg-accent/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent'
          : ''
      "
    >
      <div class="flex items-center justify-between gap-2">
        <h3
          class="text-sm font-semibold text-primary truncate"
          :class="to ? 'transition-colors group-hover:text-accent' : ''"
        >{{ cycle.name }}</h3>
        <!-- Frozen cycles show their archival state; live cycles show the
             on-track / at-risk / behind pace pill instead. -->
        <div class="flex items-center gap-1.5 shrink-0">
          <span
            v-if="isFrozen"
            class="text-[10px] uppercase tracking-wide font-semibold text-tertiary"
          >{{ t('tickets-cycle-burndown-frozen') }}</span>
          <StatusPill v-else-if="health" :tone="health.tone" :label="health.label" />
          <Icon
            v-if="to"
            name="chevronRight"
            size="xs"
            class="text-tertiary transition-colors group-hover:text-accent"
          />
        </div>
      </div>

      <template v-if="stats && !isLoading && !error">
        <!-- Headline numbers -->
        <div class="mt-3 flex items-baseline gap-4">
          <div>
            <div class="text-2xl font-semibold text-primary tabular-nums">
              {{ stats.completed }}<span class="text-tertiary">/{{ stats.tickets }}</span>
            </div>
            <div class="text-[10px] uppercase tracking-wide text-tertiary">{{ t('tickets-cycle-burndown-tickets-done') }}</div>
          </div>
          <div>
            <div class="text-2xl font-semibold text-primary tabular-nums">{{ completionPct }}%</div>
            <div class="text-[10px] uppercase tracking-wide text-tertiary">{{ t('tickets-cycle-burndown-complete') }}</div>
          </div>
          <div v-if="daysRemaining != null">
            <div class="text-2xl font-semibold text-primary tabular-nums">{{ daysRemaining }}</div>
            <div class="text-[10px] uppercase tracking-wide text-tertiary">
              {{ t('tickets-cycle-burndown-days-remaining', { count: daysRemaining }) }}
            </div>
          </div>
        </div>

        <!-- Progress bar -->
        <div class="mt-3 h-1.5 rounded-full bg-surface-hover overflow-hidden">
          <div
            class="h-full bg-accent transition-all motion-reduce:transition-none"
            :style="{ width: `${completionPct}%` }"
          />
        </div>
      </template>
    </component>

    <div v-if="isLoading" class="text-xs text-tertiary italic">{{ t('tickets-cycle-burndown-loading') }}</div>
    <div v-else-if="error" class="text-xs text-status-error">{{ error }}</div>

    <div v-else-if="stats" class="mt-3 flex flex-col gap-3">
      <!-- Cycle signals: scope creep + carryover, promoted from fine
           print so they read alongside the headline, not as a footnote. -->
      <div
        v-if="(stats.scope_added ?? 0) > 0 || (isFrozen && (stats.carried_over ?? 0) > 0)"
        class="flex flex-wrap items-center gap-2"
      >
        <span
          v-if="(stats.scope_added ?? 0) > 0"
          class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium bg-status-warning-muted text-status-warning"
        >
          <Icon name="add" size="xs" />
          {{ t('tickets-cycle-scope-added', { count: stats.scope_added ?? 0 }) }}
        </span>
        <span
          v-if="isFrozen && (stats.carried_over ?? 0) > 0"
          class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium bg-surface-hover text-secondary"
        >
          <Icon name="move" size="xs" />
          {{ t('tickets-cycle-burndown-carried-over', { count: stats.carried_over ?? 0 }) }}
        </span>
      </div>

      <!-- Burnup chart (live cycles with start + end dates only) -->
      <CycleBurnupChart v-if="showBurnup && burnup" :series="burnup" />

      <!-- By category: colour-coded dot + proportion bar + count -->
      <div v-if="sortedCategories.length" class="flex flex-col gap-1.5">
        <div
          v-for="[cat, count] in sortedCategories"
          :key="cat"
          class="flex items-center gap-2 text-xs"
        >
          <StatusIndicator :category="(cat as WorkflowStateCategory)" size="sm" />
          <span class="text-secondary w-20 shrink-0 truncate">{{ categoryLabels[cat] ?? cat }}</span>
          <div class="flex-1 h-1.5 rounded-full bg-surface-hover overflow-hidden">
            <div
              class="h-full rounded-full transition-all motion-reduce:transition-none"
              :class="categoryBarClass(cat)"
              :style="{ width: `${categoryPct(count)}%` }"
            />
          </div>
          <span class="text-tertiary tabular-nums w-6 text-right shrink-0">{{ count }}</span>
        </div>
      </div>

      <!-- Frozen-snapshot timestamp -->
      <p v-if="isFrozen" class="text-[10px] text-tertiary italic">
        {{ t('tickets-cycle-burndown-snapshot-frozen', { date: formatDateTime(stats.frozen_at) }) }}
      </p>
    </div>
  </div>
</template>
