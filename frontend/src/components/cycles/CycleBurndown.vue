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
import { useFluent } from 'fluent-vue'
import { cyclesService, type CycleStats, type BurnupSeries } from '@/services/cyclesService'
import type { Cycle } from '@/services/cyclesService'
import { formatDateTime } from '@/utils/dateUtils'
import { WORKFLOW_CATEGORIES } from '@/types/workflow'
import CycleBurnupChart from './CycleBurnupChart.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{ cycle: Cycle }>()

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
</script>

<template>
  <div class="rounded-md border border-subtle bg-app p-4">
    <header class="flex items-baseline justify-between mb-3">
      <h3 class="text-sm font-semibold text-primary">{{ cycle.name }}</h3>
      <span
        class="text-[10px] uppercase tracking-wide font-semibold"
        :class="isFrozen ? 'text-tertiary' : 'text-accent'"
      >{{ isFrozen ? t('tickets-cycle-burndown-frozen') : t('tickets-cycle-burndown-live') }}</span>
    </header>

    <div v-if="isLoading" class="text-xs text-tertiary italic">{{ t('tickets-cycle-burndown-loading') }}</div>
    <div v-else-if="error" class="text-xs text-status-error">{{ error }}</div>

    <div v-else-if="stats" class="flex flex-col gap-3">
      <!-- Headline numbers -->
      <div class="flex items-baseline gap-4">
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
      <div class="h-1.5 rounded-full bg-surface-hover overflow-hidden">
        <div
          class="h-full bg-accent transition-all"
          :style="{ width: `${completionPct}%` }"
        />
      </div>

      <!-- Burnup chart (live cycles with start + end dates only) -->
      <CycleBurnupChart v-if="showBurnup && burnup" :series="burnup" />

      <!-- By category -->
      <div v-if="sortedCategories.length" class="flex flex-col gap-1">
        <div
          v-for="[cat, count] in sortedCategories"
          :key="cat"
          class="flex items-center justify-between text-xs"
        >
          <span class="text-secondary">{{ categoryLabels[cat] ?? cat }}</span>
          <span class="text-tertiary tabular-nums">{{ count }}</span>
        </div>
      </div>

      <!-- Scope added after start (creep); live and frozen both carry it -->
      <p
        v-if="(stats.scope_added ?? 0) > 0"
        class="text-xs text-tertiary"
      >
        {{ t('tickets-cycle-scope-added', { count: stats.scope_added ?? 0 }) }}
      </p>

      <!-- Carried-over count (frozen snapshots only) -->
      <p
        v-if="isFrozen && (stats.carried_over ?? 0) > 0"
        class="text-xs text-tertiary"
      >
        {{ t('tickets-cycle-burndown-carried-over', { count: stats.carried_over ?? 0 }) }}
      </p>

      <!-- Frozen-snapshot timestamp -->
      <p v-if="isFrozen" class="text-[10px] text-tertiary italic">
        {{ t('tickets-cycle-burndown-snapshot-frozen', { date: formatDateTime(stats.frozen_at) }) }}
      </p>
    </div>
  </div>
</template>
