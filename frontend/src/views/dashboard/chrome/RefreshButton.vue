<script setup lang="ts">
/**
 * R-key refresh button with "Updated HH:MM" indicator
 * (docs/dashboard-and-analytics-plan.md decision 9).
 *
 * Live SSE is restricted to Row 1 KPIs only (Wave 6). Time-series,
 * leaderboards, heatmaps, and the rest show "Updated HH:MM" with
 * this manual-refresh button so the data-freshness story stays
 * honest. Wave 1 ships the UI + emit; consumers wire the actual
 * refetch by listening to the `refresh` event on the page or by
 * binding the `R` key shortcut (also Wave 6).
 *
 * Timestamp source: server time of the most recent successful
 * fetch, formatted via formatDistanceToNow so the display is
 * timezone-safe.
 */
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import { formatDistanceToNow } from 'date-fns'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  /** ISO-8601 timestamp the page last refreshed. `null` shows
   *  "Just loaded". */
  updatedAt: string | null
}>()

const emit = defineEmits<{
  (e: 'refresh'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

// Tick the relative-time display every 30s so "5 min ago" stays
// approximately current without paying the ticking-clock cost every
// frame. The interval is cheap (single Vue ref write); the format
// only churns minute-by-minute anyway.
const now = ref(Date.now())
let intervalId: number | undefined
onMounted(() => {
  intervalId = window.setInterval(() => {
    now.value = Date.now()
  }, 30_000)
})
onBeforeUnmount(() => {
  if (intervalId !== undefined) window.clearInterval(intervalId)
})

const relativeTime = computed<string>(() => {
  if (!props.updatedAt) return t('dashboard-refresh-just-loaded')
  // Read `now` so the computed re-runs on every interval tick.
  void now.value
  try {
    return formatDistanceToNow(new Date(props.updatedAt), { addSuffix: true })
  } catch {
    return t('dashboard-refresh-unknown')
  }
})
</script>

<template>
  <button
    type="button"
    class="inline-flex items-center gap-1.5 rounded-md border border-default bg-surface px-2 py-1 text-xs text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
    :title="t('dashboard-refresh-tooltip')"
    @click="emit('refresh')"
  >
    <Icon name="refresh" class="w-3.5 h-3.5" />
    <span>{{ t('dashboard-refresh-updated-prefix') }} {{ relativeTime }}</span>
  </button>
</template>
