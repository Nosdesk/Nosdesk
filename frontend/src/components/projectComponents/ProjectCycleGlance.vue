<script setup lang="ts">
/**
 * Compact active-cycle readout for a project card: cycle name,
 * count-based progress (completed / total + a thin bar), and the
 * relative end date. Renders nothing when the project has no active
 * cycle. The whole row is informational; the card itself owns the
 * click into the project.
 */
import { computed } from 'vue'
import { formatDate, formatRelativeTime } from '@/utils/dateUtils'
import type { ActiveCycleSummary } from '@/composables/useActiveCycleSummaries'

const props = defineProps<{ summary: ActiveCycleSummary | null }>()

const pct = computed(() => {
  const s = props.summary
  if (!s || s.tickets === 0) return 0
  return Math.round((s.completed / s.tickets) * 100)
})
</script>

<template>
  <div v-if="summary" class="flex items-center gap-2 text-xs" @click.stop>
    <span class="inline-flex items-center gap-1 min-w-0 text-secondary">
      <svg
        class="w-3.5 h-3.5 shrink-0 text-tertiary"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        aria-hidden="true"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"
        />
      </svg>
      <span class="truncate">{{ summary.cycle.name }}</span>
    </span>

    <span class="shrink-0 tabular-nums text-tertiary">
      {{ summary.completed }}/{{ summary.tickets }}
    </span>

    <div class="flex-1 min-w-8 h-1 rounded-full bg-surface-alt overflow-hidden">
      <div class="h-full rounded-full bg-accent transition-all" :style="{ width: `${pct}%` }" />
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
