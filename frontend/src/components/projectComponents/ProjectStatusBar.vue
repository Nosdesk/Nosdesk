<script setup lang="ts">
/**
 * Stacked status-breakdown bar for a project. Reads the coarse
 * buckets from a ProjectRollup and paints them as proportional
 * segments: done (closed) anchors the left like a progress fill,
 * then in-progress, then open. An empty track renders when the
 * project has no tickets.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'

const props = defineProps<{
  open: number
  inProgress: number
  closed: number
  total: number
}>()

const fluent = useFluent()

const segments = computed(() => {
  const pct = (n: number) => (props.total > 0 ? (n / props.total) * 100 : 0)
  return [
    { key: 'closed', pct: pct(props.closed), cls: 'bg-status-closed' },
    { key: 'in-progress', pct: pct(props.inProgress), cls: 'bg-status-in-progress' },
    { key: 'open', pct: pct(props.open), cls: 'bg-status-open' },
  ]
})

const summary = computed(() =>
  fluent.$t('projects-status-summary', {
    done: props.closed,
    doing: props.inProgress,
    open: props.open,
    total: props.total,
  }),
)
</script>

<template>
  <div
    class="flex h-1.5 w-full overflow-hidden rounded-full bg-surface-alt"
    role="img"
    :aria-label="summary"
    :title="summary"
  >
    <div
      v-for="seg in segments"
      v-show="seg.pct > 0"
      :key="seg.key"
      class="h-full"
      :class="seg.cls"
      :style="{ width: `${seg.pct}%` }"
    />
  </div>
</template>
