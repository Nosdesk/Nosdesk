<script setup lang="ts">
/**
 * The row loop of the mobile ticket card list, one instance per
 * group. Its own component so each group gets its own v-memo cache
 * (see BucketRows.vue for why a v-memo inside an outer v-for
 * corrupts itself).
 *
 * The due chip reads the day as written ("Today", "Tomorrow",
 * "Fri 27") and turns the danger colour once the ticket is past
 * its due day and still open, and carries the due time when there is
 * one ("Today 2:30 pm"). `today` is part of the memo key, so the chips
 * move on at midnight.
 *
 * The size chip sits at the top line's right edge, outside the row's
 * button (a button can't hold another). Under the Manual sort a held
 * row can be dragged; the list owns the drag (`TicketsCardList`).
 */
import UserCell from '@/components/views/UserCell.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import SizeChip from '@/components/planning/SizeChip.vue'
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
import { formatWallTime } from '@nosdesk/core/utils/ticketPlan'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = defineProps<{
  cards: CardData[]
  today: Day
  /** Rows can be dragged into an order (Manual sort, ungrouped). */
  draggable?: boolean
  /** The row being dragged, dimmed in place until the drop. */
  draggedId?: number | null
  /** The last hand-placed row: its bottom edge is the rule above the
   *  rows still in priority order. */
  lastPlacedId?: number | null
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'contextmenu', id: number, event: MouseEvent): void
  (e: 'row-down', id: number, event: PointerEvent): void
  (e: 'patch', id: number, patch: Partial<CardData>): void
}>()

const fluent = useFluent()

function dueChip(card: CardData): { label: string; overdue: boolean; title: string } | null {
  const day = dueDay(card)
  if (!day) return null
  const overdue = day < props.today && !TERMINAL_CATEGORIES.has(card.workflow_state.category)
  const date = formatDay(day)
  const word = dueChipLabel(day, props.today, {
    today: fluent.$t('tickets-due-today'),
    tomorrow: fluent.$t('tickets-due-tomorrow'),
  })
  return {
    label: card.due_time ? `${word} ${formatWallTime(card.due_time)}` : word,
    overdue,
    title: overdue
      ? `${fluent.$t('tickets-due-overdue')}: ${date}`
      : fluent.$t('ticket-detail-scheduling-due-prefix', { date }),
  }
}

function onRowClick(id: number, event: MouseEvent): void {
  // Under Manual the list's drag helper tells a pointer's click from a
  // drag; a click with no pointer (keyboard, screen reader) still opens.
  if (!props.draggable || event.detail === 0) emit('open', id)
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
      v-memo="[...rowMemoKey(card), today, draggable, draggedId === card.id, lastPlacedId === card.id]"
      :data-row="card.id"
      class="plan-row relative border-b last:border-b-0"
      :class="[
        lastPlacedId === card.id ? 'border-strong' : 'border-subtle',
        draggable && 'plan-row-hold',
        draggedId === card.id && 'opacity-40',
      ]"
      @pointerdown="draggable && emit('row-down', card.id, $event)"
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
        data-row-press
        class="w-full text-left flex flex-col gap-1 px-3 py-3 hover:bg-surface-hover active:bg-surface-alt transition-colors"
        @click="onRowClick(card.id, $event)"
        @contextmenu="onContextMenu(card.id, $event)"
      >
        <!-- Top line: id + title. Title takes whatever width
             remains and wraps rather than truncating, so a larger text
             size costs a line, not the end of the title. Its right end
             leaves room for the size chip, which sits outside the button. -->
        <div class="flex items-baseline gap-2 min-w-0 pr-7">
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
          <span class="min-w-0 flex-1 font-medium text-sm leading-snug text-primary [overflow-wrap:anywhere]">{{ card.title }}</span>
        </div>

        <!-- Detail line: workflow state dot + name, the SLA and
             approval chips, assignee avatar (if any), last activity, due
             date. The chips live here, not beside the title, so the title
             keeps the whole top line to wrap in.
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
            v-if="card.sla?.breached"
            class="text-3xs font-semibold uppercase tracking-wide text-status-error shrink-0"
            :title="$t('ticket-list-sla-breached-title')"
          >SLA</span>
          <span
            v-if="card.approval_state === 'pending'"
            class="text-3xs font-medium uppercase tracking-wide px-1.5 py-0.5 rounded bg-status-warning-muted text-status-warning shrink-0"
            :title="$t('views-ticket-row-approval-tooltip')"
          >{{ $t('views-ticket-row-approval-badge') }}</span>

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
      <span class="absolute top-3 right-3 flex">
        <SizeChip
          :minutes="card.estimate_minutes"
          :number="card.number"
          @update="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
        />
      </span>
    </li>
  </ul>
</template>
