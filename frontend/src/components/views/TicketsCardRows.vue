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
import Icon from '@/components/common/Icon.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import SizeChip from '@/components/planning/SizeChip.vue'
import RowChip from '@/components/views/RowChip.vue'
import UserName from '@/components/views/UserName.vue'
import { useFluent } from 'fluent-vue'
import { paletteForColor } from '@nosdesk/core/utils/workflowColors'
import { priorityForBadge, priorityLabel, rowSlaToneClass } from '@/utils/priorityHelpers'
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
      class="plan-row relative flex flex-col gap-1 border-b px-4 py-3 last:border-b-0 hover:bg-surface-hover active:bg-surface-alt transition-colors"
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

      <!-- Two alignment lines: the title on the left gutter, priority
           and size on the right edge beside its first line. -->
      <div class="flex items-start gap-3">
        <!-- The title is the row's button, stretched over the whole
             card (`after:inset-0`); the size chip sits above it, so
             neither holds the other. It wraps and is never clamped:
             the people who turn the text up are the ones a clamp cuts
             first. The number runs on as its last word, tied to the
             word before it, so a wrapped title keeps its left edge. -->
        <button
          type="button"
          data-row-press
          class="min-w-0 flex-1 text-left text-sm leading-snug font-medium text-primary [overflow-wrap:anywhere] after:absolute after:inset-0 focus:outline-none focus-visible:after:ring-2 focus-visible:after:ring-accent focus-visible:after:ring-inset"
          @click="onRowClick(card.id, $event)"
          @contextmenu="onContextMenu(card.id, $event)"
        >{{ card.title }}<span class="font-mono text-2xs font-medium whitespace-nowrap text-secondary tabular-nums">&nbsp;#{{ card.number }}</span></button>
        <span class="relative z-10 flex h-[1.375em] shrink-0 items-center gap-2 text-sm">
          <span
            v-if="priorityForBadge(card.priority)"
            class="type-glyph"
            :title="priorityLabel(card.priority)"
          >
            <PriorityIndicator :priority="priorityForBadge(card.priority)!" size="xs" />
          </span>
          <SizeChip
            :minutes="card.estimate_minutes"
            :number="card.number"
            @update="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
          />
        </span>
      </div>

      <!-- Details: whole items that never break inside. When the line
           fills they move to the next one, most useful first: state,
           due, flags, then who has it with its last activity (one item,
           so a lone "1h" never takes a line). Text at the 12px floor is
           medium weight in the secondary colour; the tertiary grey
           starts at 13px. -->
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-tertiary">
        <span class="inline-flex items-center gap-[0.45em] whitespace-nowrap text-secondary">
          <span
            class="h-[0.6em] w-[0.6em] shrink-0 rounded-full"
            :class="paletteForColor(card.workflow_state.color).solid"
            aria-hidden="true"
          />
          {{ card.workflow_state.name }}
        </span>
        <RowChip
          v-if="dueChip(card)"
          :tone="dueChip(card)!.overdue ? 'critical' : 'neutral'"
          :dot="dueChip(card)!.overdue"
          :title="dueChip(card)!.title"
        >
          <span v-if="dueChip(card)!.overdue" class="sr-only">{{ $t('tickets-due-overdue') }}: </span>{{ dueChip(card)!.label }}
        </RowChip>
        <RowChip v-if="card.sla?.breached" tone="critical">{{ $t('ticket-list-sla-breached-title') }}</RowChip>
        <RowChip
          v-if="card.approval_state === 'pending'"
          tone="caution"
          :title="$t('views-ticket-row-approval-tooltip')"
        >{{ $t('views-ticket-row-approval-badge') }}</RowChip>
        <span class="inline-flex min-w-0 flex-wrap items-center gap-x-3">
          <UserName v-if="card.assignee_uuid" :uuid="card.assignee_uuid" />
          <span class="inline-flex items-center gap-[0.35em] whitespace-nowrap tabular-nums">
            <span v-if="card.recurrence_rule" class="type-glyph" :title="$t('ticket-list-recurring-title')">
              <Icon name="refresh" size="xs" />
              <span class="sr-only">{{ $t('ticket-list-recurring-title') }}</span>
            </span>
            {{ formatCompactRelativeTime(card.last_activity_at) }}
          </span>
        </span>
      </div>
    </li>
  </ul>
</template>
