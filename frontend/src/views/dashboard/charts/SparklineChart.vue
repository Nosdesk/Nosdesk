<!--
Compact sparkline using the same rendering vocabulary as LineChart:
monotone curve, accent gradient fill, optional prior overlay, and
endpoint marker. Used by ticket-volume KPI columns and legacy KPI
tiles so mini charts read as the same family as "Tickets over time".
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { useElementSize } from '@/composables/useElementSize'
import { buildSparklinePaths } from './seriesPath'

let gradientUidCounter = 0
function nextGradientUid(): number {
  gradientUidCounter += 1
  return gradientUidCounter
}

const props = withDefaults(
  defineProps<{
    values?: number[] | null
    priorValues?: number[] | null
    /** Fixed pixel height when `fluid` is false. */
    height?: number
    /** Fill the flex parent instead of a fixed pixel strip. */
    fluid?: boolean
  }>(),
  {
    height: 36,
    fluid: false,
  },
)

const gradientId = `spark-area-${nextGradientUid()}`
const containerRef = ref<HTMLElement | null>(null)
const { width, height: observedHeight } = useElementSize(containerRef)

const chartHeight = computed(() => (props.fluid ? observedHeight.value : props.height))

const chart = computed(() => {
  const values = props.values
  if (!values?.length || width.value <= 0 || chartHeight.value <= 0) return null
  return buildSparklinePaths({
    values,
    priorValues: props.priorValues,
    width: width.value,
    height: chartHeight.value,
  })
})
</script>

<template>
  <div
    ref="containerRef"
    :class="[
      'w-full min-h-0',
      fluid ? 'flex-1 h-full min-h-2' : 'shrink-0',
    ]"
    :style="fluid ? undefined : { height: `${height}px` }"
    aria-hidden="true"
  >
    <svg
      v-if="chart && width > 0 && chartHeight > 0"
      :viewBox="`0 0 ${width} ${chartHeight}`"
      class="block w-full h-full overflow-visible"
    >
      <defs>
        <linearGradient :id="gradientId" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--color-accent)" stop-opacity="0.22" />
          <stop offset="100%" stop-color="var(--color-accent)" stop-opacity="0" />
        </linearGradient>
      </defs>

      <path
        v-if="chart.priorPath"
        :d="chart.priorPath"
        fill="none"
        stroke="currentColor"
        stroke-width="1.25"
        stroke-dasharray="3,2"
        stroke-linejoin="round"
        stroke-linecap="round"
        class="text-tertiary"
        opacity="0.7"
      />

      <path v-if="chart.areaPath" :d="chart.areaPath" :fill="`url(#${gradientId})`" stroke="none" />

      <path
        v-if="chart.linePath"
        :d="chart.linePath"
        fill="none"
        stroke="currentColor"
        stroke-width="1.75"
        stroke-linejoin="round"
        stroke-linecap="round"
        class="text-accent"
      />

      <circle
        v-if="chart.last"
        :cx="chart.last.x"
        :cy="chart.last.y"
        r="3.5"
        class="text-surface"
        fill="currentColor"
      />
      <circle
        v-if="chart.last"
        :cx="chart.last.x"
        :cy="chart.last.y"
        r="2"
        class="text-accent"
        fill="currentColor"
      />
    </svg>
  </div>
</template>
