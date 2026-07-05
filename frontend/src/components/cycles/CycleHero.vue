<script setup lang="ts">
/**
 * The one cycle header: "are we on track?" in a single glance.
 * Name + health pill, headline done/total + % + days remaining, a
 * progress bar, honest scope-creep / carryover chips, and (full
 * variant) the burnup chart + category breakdown.
 *
 * Purely presentational: stats come from `useCycleStats` (pool fold
 * or frozen snapshot) and the burnup series from `useCycleBurnup`,
 * so the hero renders live cycles and frozen history through one
 * code path. Used by ProjectCyclesView (full) and CycleDetailView
 * (dense, pinned above the board). Replaces CycleBurndown, which
 * fetched its own stats per mount.
 */
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import type { BurnupSeries } from '@nosdesk/core/services/cyclesService'
import { formatDate, formatDateTime } from '@nosdesk/core/utils/dateUtils'
import {
  WORKFLOW_CATEGORIES,
  coarseStatusBucket,
  getCategoryLabel,
  type WorkflowStateCategory,
} from '@nosdesk/core/types/workflow'
import { cycleHealth, cycleHealthPresentation } from '@/utils/cycleHealth'
import type { CycleProgress } from '@/composables/useCycleStats'
import StatusPill from '@/components/common/StatusPill.vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import Icon from '@/components/common/Icon.vue'
import CycleBurnupChart from './CycleBurnupChart.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = withDefaults(
  defineProps<{
    cycle: {
      uuid: string
      name: string
      state: 'planned' | 'active' | 'completed'
      start_at?: string | null
      end_at?: string | null
    }
    stats: CycleProgress
    /** Daily series for the chart (full variant, live dated cycles). */
    burnup?: BurnupSeries | null
    /** `full` = chart + category breakdown; `dense` = headline only
     *  (the detail board already shows the work). */
    variant?: 'full' | 'dense'
    /** When set, the headline links there (the cycle's board). */
    to?: string
  }>(),
  { variant: 'full', burnup: null, to: undefined },
)

const isFrozen = computed(() => props.cycle.state === 'completed')

const completionPct = computed(() => {
  if (props.stats.total === 0) return 0
  return Math.round((props.stats.completed / props.stats.total) * 100)
})

const daysRemaining = computed<number | null>(() => {
  if (isFrozen.value || !props.cycle.end_at) return null
  const ms = new Date(props.cycle.end_at).getTime() - Date.now()
  return Math.max(0, Math.ceil(ms / 86_400_000))
})

const dateRange = computed<string | null>(() => {
  if (!props.cycle.start_at && !props.cycle.end_at) return null
  const s = props.cycle.start_at ? formatDate(props.cycle.start_at) : '…'
  const e = props.cycle.end_at ? formatDate(props.cycle.end_at) : '…'
  return `${s} → ${e}`
})

const health = computed(() => {
  if (isFrozen.value) return null
  const h = cycleHealth({
    total: props.stats.total,
    completed: props.stats.completed,
    startAt: props.cycle.start_at ?? null,
    endAt: props.cycle.end_at ?? null,
  })
  const { tone, labelKey } = cycleHealthPresentation(h)
  return { tone, label: t(labelKey) }
})

/** Mid-cycle scope creep: frozen snapshots carry it; live cycles
 *  derive it from the burnup's start/final scope. */
const scopeAdded = computed(() => {
  if (props.stats.scope_added != null) return props.stats.scope_added
  if (props.burnup) return Math.max(0, props.burnup.final_scope - props.burnup.start_scope)
  return 0
})

const showBurnup = computed(
  () =>
    props.variant === 'full' &&
    !isFrozen.value &&
    !!props.cycle.start_at &&
    !!props.cycle.end_at &&
    !!props.burnup,
)

const sortedCategories = computed<[string, number][]>(() => {
  const order = WORKFLOW_CATEGORIES as readonly string[]
  const rank = (c: string) => {
    const i = order.indexOf(c)
    return i === -1 ? order.length : i
  }
  return Object.entries(props.stats.by_category).sort(([a], [b]) => rank(a) - rank(b))
})

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
  return props.stats.total > 0 ? Math.round((count / props.stats.total) * 100) : 0
}
</script>

<template>
  <div class="rounded-md border border-subtle bg-surface p-4">
    <!-- Clickable top summary: a link to the cycle's board when `to`
         is set, static otherwise. Chart + breakdown below stay
         non-navigating. -->
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
        <div class="min-w-0">
          <h3
            class="text-sm font-semibold text-primary truncate"
            :class="to ? 'transition-colors group-hover:text-accent' : ''"
          >{{ cycle.name }}</h3>
          <p v-if="dateRange" class="text-[11px] text-tertiary tabular-nums mt-0.5">
            {{ dateRange }}
          </p>
        </div>
        <div class="flex items-center gap-1.5 shrink-0">
          <span
            v-if="isFrozen"
            class="text-[11px] uppercase tracking-wide font-semibold text-tertiary"
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

      <!-- Headline numbers -->
      <div class="mt-3 flex items-baseline gap-4">
        <div>
          <div class="text-2xl font-semibold text-primary tabular-nums">
            {{ stats.completed }}<span class="text-tertiary">/{{ stats.total }}</span>
          </div>
          <div class="text-[11px] uppercase tracking-wide text-tertiary">{{ t('tickets-cycle-burndown-tickets-done') }}</div>
        </div>
        <div>
          <div class="text-2xl font-semibold text-primary tabular-nums">{{ completionPct }}%</div>
          <div class="text-[11px] uppercase tracking-wide text-tertiary">{{ t('tickets-cycle-burndown-complete') }}</div>
        </div>
        <div v-if="daysRemaining != null">
          <div class="text-2xl font-semibold text-primary tabular-nums">{{ daysRemaining }}</div>
          <div class="text-[11px] uppercase tracking-wide text-tertiary">
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
    </component>

    <div class="mt-3 flex flex-col gap-3">
      <!-- Cycle signals: scope creep + carryover, promoted from fine
           print so they read alongside the headline. -->
      <div
        v-if="scopeAdded > 0 || (isFrozen && (stats.carried_over ?? 0) > 0)"
        class="flex flex-wrap items-center gap-2"
      >
        <span
          v-if="scopeAdded > 0"
          class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium bg-status-warning-muted text-status-warning"
        >
          <Icon name="add" size="xs" />
          {{ t('tickets-cycle-scope-added', { count: scopeAdded }) }}
        </span>
        <span
          v-if="isFrozen && (stats.carried_over ?? 0) > 0"
          class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium bg-surface-hover text-secondary"
        >
          <Icon name="move" size="xs" />
          {{ t('tickets-cycle-burndown-carried-over', { count: stats.carried_over ?? 0 }) }}
        </span>
      </div>

      <!-- Burnup chart (full variant, live cycles with dates only) -->
      <CycleBurnupChart v-if="showBurnup && burnup" :series="burnup" />

      <!-- By category: colour-coded dot + proportion bar + count -->
      <div v-if="variant === 'full' && sortedCategories.length" class="flex flex-col gap-1.5">
        <div
          v-for="[cat, count] in sortedCategories"
          :key="cat"
          class="flex items-center gap-2 text-xs"
        >
          <StatusIndicator :category="(cat as WorkflowStateCategory)" size="sm" />
          <span class="text-secondary w-20 shrink-0 truncate">{{ getCategoryLabel(cat as WorkflowStateCategory) }}</span>
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
      <p v-if="variant === 'full' && isFrozen && stats.frozen_at" class="text-[11px] text-tertiary italic">
        {{ t('tickets-cycle-burndown-snapshot-frozen', { date: formatDateTime(stats.frozen_at) }) }}
      </p>
    </div>
  </div>
</template>
