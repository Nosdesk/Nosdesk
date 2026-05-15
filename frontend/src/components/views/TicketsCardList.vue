<script setup lang="ts">
/**
 * Mobile card list for the tickets surface.
 *
 * The desktop table is dense in horizontal direction — many
 * columns side by side. On a phone the same approach degrades
 * to a horizontally-scrolled wall of cells where the user only
 * sees 1-2 columns at a time, defeating the point of a list.
 *
 * This component flips the geometry: each ticket becomes a card
 * row with attributes stacked vertically. Models the legacy V1
 * mobile pattern (since deleted in 78d71ae) plus Linear / Front
 * / email-app conventions:
 *
 *   ┌──┬─────────────────────────────────────────┐
 *   │S │ #1234  This is the ticket title…        │
 *   │L │ ◯ In Progress · Alice · 3h ago · ! High │
 *   │A │                                         │
 *   └──┴─────────────────────────────────────────┘
 *
 * - Leading 3px tone strip encodes SLA urgency (matches the
 *   table's row stripe so the visual vocabulary stays consistent
 *   across breakpoints).
 * - Top line: id + title (truncated). Title carries the visual
 *   weight that lets a tech scan a 30-row queue.
 * - Detail line: workflow state dot + name, assignee, last
 *   activity timestamp, priority. Wraps on very narrow viewports
 *   so nothing gets cut.
 *
 * Whole row is the tap target. Click opens; selection (split-
 * view) is desktop-only and not surfaced here — phones don't
 * have room for a side-by-side preview pane.
 *
 * Memo against the same field subset as the table so SSE
 * updates only re-render the affected rows.
 */
import UserCell from '@/components/views/UserCell.vue'
import { paletteForColor } from '@/utils/workflowColors'
import { inlinePriorityClass } from '@/utils/priorityHelpers'
import { rowSlaToneClass } from '@/utils/priorityHelpers'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
import { rowMemoKey } from '@/sync/views/ticketColumns'
import type { CardData } from '@/sync/views/types'

defineProps<{
  cards: CardData[]
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
}>()

function onRowClick(id: number): void {
  emit('open', id)
}
</script>

<template>
  <div class="flex-1 min-h-0 overflow-auto">
    <ul class="flex flex-col">
      <li
        v-for="card in cards"
        :key="card.id"
        v-memo="rowMemoKey(card)"
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
        >
          <!-- Top line: id + title. Title takes whatever width
               remains; truncates on overflow. -->
          <div class="flex items-center gap-2 min-w-0">
            <span class="text-[11px] font-mono tabular-nums text-tertiary shrink-0">#{{ card.id }}</span>
            <span
              v-if="inlinePriorityClass(card.priority)"
              class="text-[11px] leading-none font-bold shrink-0"
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
              v-if="card.sla?.breached"
              class="ml-auto text-[10px] font-semibold uppercase tracking-wide text-rose-600 dark:text-rose-400 shrink-0"
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
          </div>
        </button>
      </li>
    </ul>

    <!-- Empty state lives in the parent (TicketsListView) so the
         contextual copy ("All caught up", "Triage is clear" etc.)
         stays consistent across the table and card surfaces. -->
  </div>
</template>
