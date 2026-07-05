<script setup lang="ts">
/**
 * Hover-card content for a gantt bar: the "scent, then reveal"
 * layer. Everything here is supplementary (the bar itself carries
 * an aria-label with the essentials); click still opens the ticket.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { format } from 'date-fns'
import type { CardData } from '@nosdesk/core/sync/views/types'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import UserAvatar from '@/components/UserAvatar.vue'

const props = defineProps<{
  card: CardData
  start: Date
  end: Date
  /** Resolved cycle name, when the ticket belongs to one. */
  cycleName?: string | null
  /** Whether the bar can be rescheduled (shows the drag hint). */
  resizable?: boolean
}>()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const TONE_BY_CATEGORY: Record<WorkflowStateCategory, StatusPillTone> = {
  triage: 'info',
  backlog: 'info',
  active: 'accent',
  in_review: 'accent',
  done: 'positive',
  cancelled: 'neutral',
  merged: 'neutral',
}
const statusTone = computed(() => TONE_BY_CATEGORY[props.card.workflow_state.category])

const dateRange = computed(
  () => `${format(props.start, 'MMM d')} → ${format(props.end, 'MMM d')}`,
)

const blocks = computed(() => props.card.relation_counts?.blocks ?? 0)
const blockedBy = computed(() => props.card.relation_counts?.blocked_by ?? 0)
</script>

<template>
  <div class="w-64 p-3 flex flex-col gap-2 text-left">
    <div class="flex items-center justify-between gap-2">
      <span class="font-mono text-[11px] text-tertiary">#{{ card.id }}</span>
      <StatusPill :label="card.workflow_state.name" :tone="statusTone" size="xs" />
    </div>

    <p class="text-sm font-medium text-primary leading-snug line-clamp-2">
      {{ card.title }}
    </p>

    <div class="flex flex-col gap-1.5 text-xs text-secondary">
      <div class="flex items-center justify-between gap-2">
        <span class="text-tertiary">{{ t('gantt-hover-dates') }}</span>
        <span class="tabular-nums">{{ dateRange }}</span>
      </div>
      <div v-if="card.assignee_uuid" class="flex items-center justify-between gap-2">
        <span class="text-tertiary">{{ t('gantt-hover-assignee') }}</span>
        <UserAvatar :uuid="card.assignee_uuid" size="xxs" :clickable="false" />
      </div>
      <div v-if="cycleName" class="flex items-center justify-between gap-2">
        <span class="text-tertiary">{{ t('gantt-hover-cycle') }}</span>
        <span class="truncate">{{ cycleName }}</span>
      </div>
      <div v-if="blocks > 0 || blockedBy > 0" class="flex items-center justify-between gap-2">
        <span class="text-tertiary">{{ t('gantt-hover-dependencies') }}</span>
        <span class="tabular-nums">
          <template v-if="blocks > 0">{{ t('gantt-hover-blocks', { count: blocks }) }}</template>
          <template v-if="blocks > 0 && blockedBy > 0"> &middot; </template>
          <template v-if="blockedBy > 0">{{ t('gantt-hover-blocked-by', { count: blockedBy }) }}</template>
        </span>
      </div>
    </div>

    <p v-if="resizable" class="text-[11px] text-tertiary border-t border-subtle/60 pt-1.5">
      {{ t('gantt-hover-reschedule-hint') }}
    </p>
  </div>
</template>
