<script setup lang="ts">
/**
 * Compact active-cycle readout for a project card: cycle name,
 * count-based progress (completed / total + a thin bar), and the
 * relative end date. Renders nothing when the project has no active
 * cycle. The whole row is informational; the card itself owns the
 * click into the project.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { formatDate, formatRelativeTime } from '@/utils/dateUtils'
import { cycleHealth, cycleHealthPresentation } from '@/utils/cycleHealth'
import Icon from '@/components/common/Icon.vue'
import type { ActiveCycleSummary } from '@/composables/useActiveCycleSummaries'

const props = withDefaults(
  defineProps<{
    summary: ActiveCycleSummary | null
    /** Hide the inner progress bar, for dense rows where the
     *  project's own status bar already conveys progress. */
    compact?: boolean
  }>(),
  { compact: false },
)

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const pct = computed(() => {
  const s = props.summary
  if (!s || s.tickets === 0) return 0
  return Math.round((s.completed / s.tickets) * 100)
})

// On-track / at-risk / behind dot. Supplementary to the count + bar
// (which carry the real signal), with a title so the meaning is
// reachable without relying on colour alone.
const health = computed(() => {
  const s = props.summary
  if (!s) return null
  const h = cycleHealth({
    total: s.tickets,
    completed: s.completed,
    startAt: s.cycle.start_at,
    endAt: s.cycle.end_at,
  })
  const { tone, labelKey } = cycleHealthPresentation(h)
  return { tone, label: t(labelKey) }
})
const healthDotClass = computed(() => {
  switch (health.value?.tone) {
    case 'positive':
      return 'bg-status-success'
    case 'caution':
      return 'bg-status-warning'
    case 'critical':
      return 'bg-status-error'
    default:
      return 'bg-strong'
  }
})
</script>

<template>
  <div v-if="summary" class="flex items-center gap-2 text-xs" @click.stop>
    <span class="inline-flex items-center gap-1.5 min-w-0 text-secondary">
      <span
        class="w-1.5 h-1.5 rounded-full shrink-0"
        :class="healthDotClass"
        :title="health?.label"
      />
      <Icon name="refresh" size="xs" class="shrink-0 text-tertiary" />
      <span class="truncate">{{ summary.cycle.name }}</span>
    </span>

    <span class="shrink-0 tabular-nums text-tertiary">
      {{ summary.completed }}/{{ summary.tickets }}
    </span>

    <div v-if="!compact" class="flex-1 min-w-8 h-1.5 rounded-full bg-surface-alt overflow-hidden">
      <div
        class="h-full rounded-full bg-accent transition-all motion-reduce:transition-none"
        :style="{ width: `${pct}%` }"
      />
    </div>

    <span
      v-if="summary.cycle.end_at"
      class="shrink-0 text-tertiary"
      :title="formatDate(summary.cycle.end_at)"
    >
      {{ formatRelativeTime(summary.cycle.end_at) }}
    </span>
  </div>
</template>
