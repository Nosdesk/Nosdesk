<script setup lang="ts">
/**
 * Compact count-based burnup chart for an active cycle. Plots three
 * series over the cycle timeline: total scope (how many tickets are
 * in the cycle), completed (how many have closed), and an ideal line
 * from zero to the final scope. Burnup rather than burndown so a
 * mid-cycle scope change shows as the scope line rising, keeping
 * "behind" visually distinct from "added work."
 *
 * Self-contained SVG, no chart library. The fixed viewBox scales to
 * the container via class="w-full h-auto".
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import type { BurnupSeries } from '@/services/cyclesService'
import { formatCompactDate } from '@/utils/dateUtils'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{ series: BurnupSeries }>()

// Plot geometry. viewBox is 640x200; the plot area is inset to leave
// room for the left axis labels and the bottom date labels.
const VB_W = 640
const VB_H = 200
const left = 36
const right = 8
const top = 8
const bottom = 22
const plotW = VB_W - left - right
const plotH = VB_H - top - bottom

const points = computed(() => props.series.points)
const hasData = computed(() => points.value.length > 0)
const n = computed(() => points.value.length)
const maxY = computed(() => Math.max(props.series.final_scope, 1))

function xFor(i: number): number {
  return left + (n.value <= 1 ? 0 : (i / (n.value - 1)) * plotW)
}
function yFor(v: number): number {
  return top + plotH - (v / maxY.value) * plotH
}

function polyline(pick: (p: { scope: number; completed: number }) => number): string {
  return points.value.map((p, i) => `${xFor(i)},${yFor(pick(p))}`).join(' ')
}

const scopeLine = computed(() => polyline((p) => p.scope))
const completedLine = computed(() => polyline((p) => p.completed))

// Ideal line: a straight path from (start, 0) to (end, final_scope).
const idealLine = computed(() => {
  const x1 = xFor(0)
  const x2 = xFor(n.value - 1)
  return `${x1},${yFor(0)} ${x2},${yFor(props.series.final_scope)}`
})

const firstDay = computed(() => (hasData.value ? formatCompactDate(points.value[0].day) : ''))
const lastDay = computed(() =>
  hasData.value ? formatCompactDate(points.value[points.value.length - 1].day) : '',
)
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center justify-between">
      <h4 class="text-xs font-semibold text-secondary uppercase tracking-wide">
        {{ t('cycle-burnup-title') }}
      </h4>
      <div v-if="hasData" class="flex items-center gap-3 text-[10px] text-tertiary">
        <span class="flex items-center gap-1">
          <span class="inline-block w-3 h-0.5 bg-accent" />
          {{ t('cycle-burnup-legend-completed') }}
        </span>
        <span class="flex items-center gap-1">
          <span class="inline-block w-3 h-0.5 bg-secondary" />
          {{ t('cycle-burnup-legend-scope') }}
        </span>
        <span class="flex items-center gap-1">
          <span class="inline-block w-3 h-0.5 border-t border-dashed border-tertiary" />
          {{ t('cycle-burnup-legend-ideal') }}
        </span>
      </div>
    </div>

    <p v-if="!hasData" class="text-xs text-tertiary italic">
      {{ t('cycle-burnup-needs-dates') }}
    </p>

    <svg
      v-else
      :viewBox="`0 0 ${VB_W} ${VB_H}`"
      class="w-full h-auto"
      role="img"
      :aria-label="t('cycle-burnup-title')"
    >
      <!-- Axes: left vertical + bottom baseline -->
      <line
        :x1="left"
        :y1="top"
        :x2="left"
        :y2="top + plotH"
        class="stroke-subtle"
        stroke-width="1"
      />
      <line
        :x1="left"
        :y1="top + plotH"
        :x2="VB_W - right"
        :y2="top + plotH"
        class="stroke-subtle"
        stroke-width="1"
      />

      <!-- Y tick labels at 0 and final_scope -->
      <text :x="left - 4" :y="yFor(0) + 3" text-anchor="end" class="fill-tertiary text-[10px]">0</text>
      <text
        :x="left - 4"
        :y="yFor(series.final_scope) + 3"
        text-anchor="end"
        class="fill-tertiary text-[10px]"
      >
        {{ series.final_scope }}
      </text>

      <!-- Ideal (dashed) -->
      <polyline
        :points="idealLine"
        fill="none"
        class="stroke-tertiary"
        stroke-width="1.5"
        stroke-dasharray="4 3"
      />
      <!-- Scope (muted) -->
      <polyline :points="scopeLine" fill="none" class="stroke-secondary" stroke-width="2" />
      <!-- Completed (accent) -->
      <polyline :points="completedLine" fill="none" class="stroke-accent" stroke-width="2" />

      <!-- X labels: first and last day -->
      <text :x="xFor(0)" :y="VB_H - 6" text-anchor="start" class="fill-tertiary text-[10px]">
        {{ firstDay }}
      </text>
      <text
        :x="xFor(n - 1)"
        :y="VB_H - 6"
        text-anchor="end"
        class="fill-tertiary text-[10px]"
      >
        {{ lastDay }}
      </text>
    </svg>
  </div>
</template>
