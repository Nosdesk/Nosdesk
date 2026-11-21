<script setup lang="ts">
/**
 * The row loop of the mobile ticket card list, one instance per
 * group. Its own component so each group gets its own v-memo cache
 * (see BucketRows.vue for why a v-memo inside an outer v-for
 * corrupts itself).
 *
 * The due chip reads the day as written ("Today", "Tomorrow",
 * "Fri 27") and turns the danger colour once the ticket is past
 * its due day and still open. `today` is part of the memo key, so
 * the chips move on at midnight.
 */
import UserCell from '@/components/views/UserCell.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import { useFluent } from 'fluent-vue'
import { paletteForColor } from '@nosdesk/core/utils/workflowColors'
import { inlinePriorityClass } from '@/utils/priorityHelpers'
import { rowSlaToneClass } from '@/utils/priorityHelpers'
import { formatCompactRelativeTime } from '@nosdesk/core/utils/dateUtils'
import { rowMemoKey } from '@nosdesk/core/sync/views/ticketColumns'
import { TERMINAL_CATEGORIES } from '@nosdesk/core/types/workflow'
import {
  dueChipLabel,
  dueDay,
  formatDay,
  type Day,
} from '@nosdesk/core/utils/ticketDates'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = defineProps<{
  cards: CardData[]
  today: Day
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'contextmenu', id: number, event: MouseEvent): void
}>()

const fluent = useFluent()

function dueChip(card: CardData): { label: string; overdue: boolean; title: string } | null {
  const day = dueDay(card)
  if (!day) return null
  const overdue = day < props.today && !TERMINAL_CATEGORIES.has(card.workflow_state.category)
  const date = formatDay(day)
  return {
    label: dueChipLabel(day, props.today, {
      today: fluent.$t('tickets-due-today'),
      tomorrow: fluent.$t('tickets-due-tomorrow'),
    }),
    overdue,
    title: overdue
      ? `${fluent.$t('tickets-due-overdue')}: ${date}`
      : fluent.$t('ticket-detail-scheduling-due-prefix', { date }),
  }
}

function onRowClick(id: number): void {
  emit('open', id)
}

function onContextMenu(id: number, event: MouseEvent): void {
  event.preventDefault()
  emit('contextmenu', id, event)
}
</script>

<template>
  <ul class="flex flex-col">
    <li
      v-for="card in cards"
      :key="card.id"
      v-memo="[...rowMemoKey(card), today]"
      class="relative border-b border-subtle last:border-b-0"
    >
      <!-- SLA tone strip on the leading edge. Empty when the
           row has no urgency signal so we don't burn a strip
           on every row. Matches the table's row stripe so the
           visual vocabulary is consistent across breakpoints. -->
      <span
        v-if="rowSlaToneClass(card)"
        class="absolute left-0 top-0 bottom-0 w-[3px]"
        :class="rowSlaToneClass(card)"
        aria-hidden="true"
      />

      <button
        type="button"
        class="w-full text-left flex flex-col gap-1 px-3 py-3 hover:bg-surface-hover active:bg-surface-alt transition-colors"
        @click="onRowClick(card.id)"
        @contextmenu="onContextMenu(card.id, $event)"
      >
        <!-- Top line: id + title. Title takes whatever width
             remains; truncates on overflow. -->
        <div class="flex items-center gap-2 min-w-0">
          <span class="text-2xs font-mono tabular-nums text-tertiary shrink-0">#{{ card.number }}</span>
          <span
            v-if="inlinePriorityClass(card.priority)"
            class="text-2xs leading-none font-bold shrink-0"
            :class="inlinePriorityClass(card.priority)!"
            :title="`Priority: ${card.priority}`"
          >!</span>
          <span
            v-if="card.recurrence_rule"
            class="text-tertiary text-xs leading-none shrink-0"
            :title="$t('ticket-list-recurring-title')"
          >↻</span>
          <span
            class="truncate font-medium text-sm text-primary"
            :title="card.title"
          >{{ card.title }}</span>
          <span
            v-if="card.approval_state === 'pending'"
            class="text-3xs font-medium uppercase tracking-wide px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-700 dark:text-amber-400 shrink-0"
            :title="$t('views-ticket-row-approval-tooltip')"
          >{{ $t('views-ticket-row-approval-badge') }}</span>
          <span
            v-if="card.sla?.breached"
            class="ml-auto text-3xs font-semibold uppercase tracking-wide text-rose-600 dark:text-rose-400 shrink-0"
            :title="$t('ticket-list-sla-breached-title')"
          >SLA</span>
        </div>

        <!-- Detail line: workflow state dot + name, assignee
             avatar (if any), last activity, due date.
             flex-wrap so nothing gets cut on narrow phones. -->
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-tertiary">
          <span class="inline-flex items-center gap-1.5 shrink-0">
            <span
              class="inline-block w-2 h-2 rounded-full"
              :class="paletteForColor(card.workflow_state.color).solid"
              aria-hidden="true"
            />
            <span class="text-secondary">{{ card.workflow_state.name }}</span>
          </span>

          <span
            v-if="card.assignee_uuid"
            class="inline-flex items-center gap-1 shrink-0"
          >
            <UserCell :uuid="card.assignee_uuid" />
          </span>

          <span class="shrink-0 text-tertiary tabular-nums">
            {{ formatCompactRelativeTime(card.last_activity_at) }}
          </span>

          <StatusPill
            v-if="dueChip(card)"
            class="shrink-0 tabular-nums"
            :label="dueChip(card)!.label"
            :tone="dueChip(card)!.overdue ? 'critical' : 'neutral'"
            :dot="dueChip(card)!.overdue"
            :title="dueChip(card)!.title"
          />
        </div>
      </button>
    </li>
  </ul>
</template>
