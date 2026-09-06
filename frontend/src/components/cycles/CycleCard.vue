<script setup lang="ts">
/**
 * Enriched cycle card for the cycles list, mirroring ProjectCard's design
 * language (bordered surface card, hover affordance, title + state, a
 * progress bar with done/total, and footer actions) so the cycles page
 * reads as a peer of the projects list rather than a thin row list.
 *
 * The card opens the cycle's board; the footer actions stop propagation.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { cycleHealth, cycleHealthPresentation } from '@/utils/cycleHealth'
import { formatDate } from '@nosdesk/core/utils/dateUtils'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import Icon from '@/components/common/Icon.vue'

/** Structural slice, so both the REST DTO and the pool row fit. */
interface CardCycle {
  uuid: string
  name: string
  state: 'planned' | 'active' | 'completed'
  start_at?: string | null
  end_at?: string | null
}

const props = defineProps<{ cycle: CardCycle; completed: number; total: number }>()

const emit = defineEmits<{
  (e: 'open'): void
  (e: 'promote'): void
  (e: 'complete'): void
  (e: 'archive'): void
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const pct = computed(() => (props.total > 0 ? Math.round((props.completed / props.total) * 100) : 0))

function stateLabel(state: string): string {
  switch (state) {
    case 'active':
      return t('project-cycles-state-active')
    case 'planned':
      return t('project-cycles-state-planned')
    case 'completed':
      return t('project-cycles-state-completed')
    default:
      return state
  }
}
function stateTone(state: string): StatusPillTone {
  return state === 'active' ? 'accent' : 'neutral'
}

// On-track / at-risk / behind, shown only for the in-flight (active)
// cycle, where pace is meaningful.
const health = computed(() => {
  if (props.cycle.state !== 'active') return null
  const h = cycleHealth({
    total: props.total,
    completed: props.completed,
    startAt: props.cycle.start_at ?? null,
    endAt: props.cycle.end_at ?? null,
  })
  const { tone, labelKey } = cycleHealthPresentation(h)
  return { tone, label: t(labelKey) }
})

function fmt(iso: string | null | undefined): string {
  return iso ? formatDate(iso) : t('project-cycles-date-missing')
}
const dateRange = computed(() => `${fmt(props.cycle.start_at)} → ${fmt(props.cycle.end_at)}`)
</script>

<template>
  <div
    class="group flex flex-col gap-3 bg-surface border border-default rounded-lg p-4 cursor-pointer transition-colors hover:border-strong"
    :class="{ 'opacity-75': cycle.state === 'completed' }"
    @click="emit('open')"
  >
    <!-- Title + state -->
    <div class="flex items-start justify-between gap-2">
      <button
        type="button"
        class="flex-1 min-w-0 text-left text-base font-medium text-primary truncate rounded transition-colors group-hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click.stop="emit('open')"
      >
        {{ cycle.name }}
      </button>
      <StatusPill :tone="stateTone(cycle.state)" :label="stateLabel(cycle.state)" class="shrink-0" />
    </div>

    <!-- Date range + health -->
    <div class="flex items-center gap-1.5 text-xs text-tertiary">
      <Icon name="refresh" size="xs" class="shrink-0" />
      <span class="tabular-nums truncate">{{ dateRange }}</span>
      <StatusPill v-if="health" :tone="health.tone" :label="health.label" class="ml-auto shrink-0" />
    </div>

    <!-- Progress -->
    <div class="flex items-center gap-2">
      <div class="flex-1 h-1.5 rounded-full bg-surface-hover overflow-hidden">
        <div
          class="h-full rounded-full bg-accent transition-all motion-reduce:transition-none"
          :style="{ width: `${pct}%` }"
        />
      </div>
      <span class="shrink-0 text-xs tabular-nums text-tertiary">{{ completed }}/{{ total }}</span>
    </div>

    <!-- Footer actions -->
    <div class="flex items-center gap-1 mt-auto pt-1" @click.stop>
      <button
        v-if="cycle.state === 'planned'"
        type="button"
        class="text-2xs text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click="emit('promote')"
      >
        {{ t('project-cycles-action-promote') }}
      </button>
      <button
        v-if="cycle.state === 'active'"
        type="button"
        class="text-2xs text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click="emit('complete')"
      >
        {{ t('project-cycles-action-complete') }}
      </button>
      <button
        v-if="cycle.state !== 'completed'"
        type="button"
        class="text-2xs text-tertiary hover:text-status-error px-2 py-1 rounded hover:bg-surface-hover ml-auto focus:outline-none focus-visible:ring-2 focus-visible:ring-status-error"
        @click="emit('archive')"
      >
        {{ t('project-cycles-action-archive') }}
      </button>
    </div>
  </div>
</template>
