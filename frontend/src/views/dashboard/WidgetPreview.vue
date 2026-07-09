<!--
Static thumbnail for the add-widget picker: a small, themed SVG that
mirrors the *layout* of a widget (a big metric + delta + sparkline, a
metric rail, the net-flow area, labelled bars, a contribution heatmap,
ticket rows, status rows) without rendering the real component or
fetching data. Illustrative only — `aria-hidden`; the picker cell
carries the accessible title. The numbers are placeholder decoration.
-->
<script setup lang="ts">
import type { WidgetPreviewKind } from './widgets'

defineProps<{ kind: WidgetPreviewKind }>()

// Deterministic per-cell heatmap intensities (no Math.random so the
// thumbnail is stable across renders).
const HEAT: number[] = Array.from({ length: 7 * 16 }, (_, i) => {
  const v = ((i * 37) % 11) / 10 // 0..1 pseudo-spread
  return 0.12 + v * 0.7
})
</script>

<template>
  <svg
    viewBox="0 0 120 68"
    class="h-full w-full"
    preserveAspectRatio="xMidYMid meet"
    aria-hidden="true"
    font-family="inherit"
  >
    <!-- KPI tile: label, big number, delta chip, sparkline. -->
    <template v-if="kind === 'kpi'">
      <rect x="12" y="10" width="30" height="4" rx="2" class="text-tertiary" fill="currentColor" opacity="0.5" />
      <text x="12" y="40" font-size="24" font-weight="700" class="text-primary" fill="currentColor">128</text>
      <text x="60" y="24" font-size="9" font-weight="600" class="text-status-success" fill="currentColor">▲ 8%</text>
      <path
        d="M12,60 L28,53 L44,56 L60,47 L76,51 L92,44 L108,49 L108,62 L12,62 Z"
        class="text-accent"
        fill="currentColor"
        opacity="0.16"
      />
      <polyline
        points="12,60 28,53 44,56 60,47 76,51 92,44 108,49"
        fill="none"
        class="text-accent"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </template>

    <!-- Metric rail: divided columns, each a number + sparkline + label. -->
    <template v-else-if="kind === 'kpi-rail'">
      <line x1="42" y1="10" x2="42" y2="58" class="text-default" stroke="currentColor" stroke-width="1" opacity="0.3" />
      <line x1="80" y1="10" x2="80" y2="58" class="text-default" stroke="currentColor" stroke-width="1" opacity="0.3" />
      <g v-for="(col, i) in [{ x: 10, n: '42' }, { x: 48, n: '37' }, { x: 86, n: '12' }]" :key="i">
        <text :x="col.x" y="28" font-size="15" font-weight="700" class="text-primary" fill="currentColor">{{ col.n }}</text>
        <polyline
          :points="`${col.x},46 ${col.x + 8},41 ${col.x + 16},44 ${col.x + 24},36`"
          fill="none"
          :class="i === 1 ? 'text-status-success' : 'text-accent'"
          stroke="currentColor"
          stroke-width="1.75"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        <rect :x="col.x" y="52" width="22" height="4" rx="2" class="text-tertiary" fill="currentColor" opacity="0.5" />
      </g>
    </template>

    <!-- Net-flow: diverging area around a centre axis. -->
    <template v-else-if="kind === 'area'">
      <line x1="8" y1="36" x2="112" y2="36" class="text-default" stroke="currentColor" stroke-width="1" opacity="0.5" />
      <path d="M8,36 C22,18 34,15 50,26 C58,31 66,34 72,36 L8,36 Z" class="text-accent" fill="currentColor" opacity="0.22" />
      <path d="M72,36 C84,45 92,51 104,48 C108,47 110,46 112,46 L112,36 Z" class="text-status-success" fill="currentColor" opacity="0.22" />
      <path
        d="M8,36 C22,18 34,15 50,26 C58,31 66,34 72,36 C84,45 92,51 104,48 C108,47 110,46 112,46"
        fill="none"
        class="text-secondary"
        stroke="currentColor"
        stroke-width="1.75"
        opacity="0.75"
      />
    </template>

    <!-- Horizontal bars: label · track+fill · value. -->
    <template v-else-if="kind === 'bars'">
      <g v-for="(row, i) in [{ w: 58, o: 0.85 }, { w: 42, o: 0.6 }, { w: 30, o: 0.45 }, { w: 18, o: 0.35 }]" :key="i">
        <rect x="8" :y="14 + i * 12" width="20" height="6" rx="2" class="text-secondary" fill="currentColor" opacity="0.4" />
        <rect x="34" :y="14 + i * 12" width="62" height="6" rx="3" class="text-default" fill="currentColor" opacity="0.18" />
        <rect x="34" :y="14 + i * 12" :width="row.w" height="6" rx="3" class="text-accent" fill="currentColor" :opacity="row.o" />
        <rect x="100" :y="14 + i * 12" width="12" height="6" rx="2" class="text-tertiary" fill="currentColor" opacity="0.4" />
      </g>
    </template>

    <!-- Contribution heatmap: 7 day rows × 16 week columns. -->
    <template v-else-if="kind === 'heatmap'">
      <template v-for="r in 7" :key="r">
        <rect
          v-for="c in 16"
          :key="c"
          :x="10 + (c - 1) * 6.6"
          :y="8 + (r - 1) * 7.4"
          width="5.2"
          height="5.2"
          rx="1.2"
          class="text-accent"
          fill="currentColor"
          :opacity="HEAT[(r - 1) * 16 + (c - 1)]"
        />
      </template>
    </template>

    <!-- Ticket rows: priority marker, title line, meta line, status pill. -->
    <template v-else-if="kind === 'list'">
      <g v-for="(row, i) in [{ y: 11, p: 'text-status-error' }, { y: 31, p: 'text-accent' }, { y: 51, p: 'text-tertiary' }]" :key="i">
        <rect x="8" :y="row.y" width="3" height="13" rx="1.5" :class="row.p" fill="currentColor" opacity="0.9" />
        <rect x="18" :y="row.y" width="66" height="5" rx="2.5" class="text-secondary" fill="currentColor" opacity="0.45" />
        <rect x="18" :y="row.y + 8" width="42" height="4" rx="2" class="text-tertiary" fill="currentColor" opacity="0.45" />
        <rect x="96" :y="row.y + 1" width="16" height="7" rx="3.5" class="text-default" fill="currentColor" opacity="0.22" />
      </g>
    </template>

    <!-- Status rows: coloured health dot, name line, meta line. -->
    <template v-else-if="kind === 'status'">
      <g v-for="(row, i) in [{ y: 11, s: 'text-status-success' }, { y: 31, s: 'text-status-error' }, { y: 51, s: 'text-tertiary' }]" :key="i">
        <circle cx="13" :cy="row.y + 5" r="3.5" :class="row.s" fill="currentColor" />
        <rect x="24" :y="row.y" width="58" height="5" rx="2.5" class="text-secondary" fill="currentColor" opacity="0.45" />
        <rect x="24" :y="row.y + 8" width="40" height="4" rx="2" class="text-tertiary" fill="currentColor" opacity="0.45" />
      </g>
    </template>
  </svg>
</template>
