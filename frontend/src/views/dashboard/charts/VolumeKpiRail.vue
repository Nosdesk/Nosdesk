<!--
Rich horizontal KPI rail for the ticket-volume widget. Each column
shows a drill-down link, headline number, optional compare delta,
mini sparkline, and label — more context than the bare KpiRail used
by SLA health.

Layout follows the same relative-height conventions as KpiRail: the
rail fills the shell body (`flex-1 min-h-0`) and each column distributes
space vertically so sparklines grow/shrink with the widget's row span
rather than relying on fixed pixel min-heights.
-->
<script setup lang="ts">
import type { RouteLocationRaw } from 'vue-router'
import SparklineChart from './SparklineChart.vue'

export interface VolumeKpi {
  id: string
  label: string
  value: number
  to: RouteLocationRaw
  description?: string
  deltaSign?: 'up' | 'down' | 'flat' | null
  deltaPctDisplay?: string | null
  sparkline?: number[] | null
  /** When true, show snapshot copy instead of a period delta. */
  snapshot?: boolean
  snapshotLabel?: string
}

defineProps<{
  kpis: VolumeKpi[]
}>()
</script>

<template>
  <div class="flex-1 flex min-h-0 divide-x divide-default">
    <router-link
      v-for="stat in kpis"
      :key="stat.id"
      :to="stat.to"
      :title="stat.description"
      class="group flex flex-1 flex-col min-h-0 min-w-0 px-2 py-2 sm:px-3 hover:bg-surface-hover transition-colors"
    >
      <div class="flex items-baseline gap-1.5 shrink-0 min-w-0">
        <span class="text-xl font-semibold tabular-nums leading-none text-primary group-hover:text-accent transition-colors">
          {{ stat.value }}
        </span>
        <span
          v-if="stat.snapshot && stat.snapshotLabel"
          class="text-3xs font-medium uppercase tracking-wide text-tertiary truncate"
        >
          {{ stat.snapshotLabel }}
        </span>
        <span
          v-else-if="stat.deltaSign && stat.deltaPctDisplay"
          :class="[
            'inline-flex items-center gap-0.5 text-3xs sm:text-2xs font-medium tabular-nums truncate',
            stat.deltaSign === 'up' ? 'text-status-success' : '',
            stat.deltaSign === 'down' ? 'text-status-error' : '',
            stat.deltaSign === 'flat' ? 'text-tertiary' : '',
          ]"
        >
          <span v-if="stat.deltaSign === 'up'" aria-hidden="true">▲</span>
          <span v-else-if="stat.deltaSign === 'down'" aria-hidden="true">▼</span>
          <span v-else aria-hidden="true">▬</span>
          <span>{{ stat.deltaPctDisplay }}</span>
        </span>
      </div>

      <!-- Sparkline: a bounded strip on the stacked layout (gives the rail an
           intrinsic height so it sizes to content and never collapses), but
           grows to fill the card on the xl lattice. -->
      <div class="h-10 xl:h-auto xl:flex-1 min-h-0 flex flex-col justify-end py-0.5">
        <SparklineChart
          v-if="stat.sparkline?.length"
          fluid
          :values="stat.sparkline"
        />
      </div>

      <span class="shrink-0 text-3xs font-medium uppercase tracking-wider text-tertiary truncate">
        {{ stat.label }}
      </span>
    </router-link>
  </div>
</template>
