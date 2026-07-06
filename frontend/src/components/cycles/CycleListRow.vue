<script setup lang="ts">
/**
 * Quiet list row for upcoming / completed cycles: state pill, name,
 * date range, a small progress bar, and inline lifecycle actions.
 * Deliberately lighter than the active-cycle hero (Linear's cycles
 * list idiom): the secondary sections should not compete with it.
 *
 * Responsive against its CONTAINER, not the viewport: these rows
 * live in a column whose width doesn't track the window (60/40 grid
 * at lg). Only the name truncates; every other cell is shrink-0 and
 * appears in order of worth as the container widens (fraction
 * always, dates at @md, bar at @lg, wider bar at @2xl), so nothing
 * ever overflows the card.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'

const props = defineProps<{
  cycle: {
    uuid: string
    name: string
    state: 'planned' | 'active' | 'completed'
    start_at?: string | null
    end_at?: string | null
  }
  completed: number
  total: number
}>()

const emit = defineEmits<{
  (e: 'open'): void
  (e: 'promote'): void
  (e: 'complete'): void
  (e: 'archive'): void
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const pct = computed(() =>
  props.total > 0 ? Math.round((props.completed / props.total) * 100) : 0,
)

const dateRange = computed(() => {
  if (!props.cycle.start_at && !props.cycle.end_at) return null
  const s = props.cycle.start_at ? formatCompactDate(props.cycle.start_at) : '…'
  const e = props.cycle.end_at ? formatCompactDate(props.cycle.end_at) : '…'
  return `${s} → ${e}`
})

const stateLabel = computed(() => {
  switch (props.cycle.state) {
    case 'active':
      return t('project-cycles-state-active')
    case 'planned':
      return t('project-cycles-state-planned')
    default:
      return t('project-cycles-state-completed')
  }
})
const stateTone = computed<StatusPillTone>(() =>
  props.cycle.state === 'active' ? 'accent' : 'neutral',
)
</script>

<template>
  <div
    class="group flex items-center gap-2.5 pl-3 pr-2 py-2 rounded-md border border-transparent hover:border-subtle hover:bg-surface-hover transition-colors motion-reduce:transition-none cursor-pointer"
    role="button"
    tabindex="0"
    @click="emit('open')"
    @keydown.enter="emit('open')"
  >
    <!-- No pill on completed rows: they only ever render under the
         "Completed" section header, so the pill just repeated it. -->
    <StatusPill
      v-if="cycle.state !== 'completed'"
      :tone="stateTone"
      :label="stateLabel"
      class="shrink-0"
    />
    <span
      class="flex-1 min-w-0 truncate text-sm font-medium text-primary group-hover:text-accent transition-colors"
      :title="cycle.name"
    >
      {{ cycle.name }}
    </span>

    <span
      v-if="dateRange"
      class="hidden @md:inline shrink-0 whitespace-nowrap text-xs text-tertiary tabular-nums"
    >{{ dateRange }}</span>

    <div class="hidden @lg:block shrink-0 h-1.5 w-16 @2xl:w-24 rounded-full bg-surface-hover overflow-hidden">
      <div class="h-full rounded-full bg-accent" :style="{ width: `${pct}%` }" />
    </div>
    <span class="shrink-0 whitespace-nowrap text-xs tabular-nums text-tertiary">
      {{ completed }}/{{ total }}
    </span>

    <div
      v-if="cycle.state !== 'completed'"
      class="flex items-center gap-0.5 shrink-0"
      @click.stop
    >
      <button
        v-if="cycle.state === 'planned'"
        type="button"
        class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-alt focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click="emit('promote')"
      >{{ t('project-cycles-action-promote') }}</button>
      <button
        v-if="cycle.state === 'active'"
        type="button"
        class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-alt focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click="emit('complete')"
      >{{ t('project-cycles-action-complete') }}</button>
      <button
        type="button"
        class="text-[11px] text-tertiary hover:text-status-error px-2 py-1 rounded hover:bg-surface-alt focus:outline-none focus-visible:ring-2 focus-visible:ring-status-error"
        @click="emit('archive')"
      >{{ t('project-cycles-action-archive') }}</button>
    </div>
  </div>
</template>
