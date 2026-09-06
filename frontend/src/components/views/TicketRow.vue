<script setup lang="ts">
/**
 * Single ticket row inside the desktop tickets table. Extracted
 * from TicketsTable.vue to eliminate the ~230 LOC duplication
 * between the flat-list and grouped-list paths — they used to
 * inline the same cell-rendering switch twice, which silently
 * caused a column-rendering tweak in one path to skip the other
 * (the recent priority-helper refactor missed an import here for
 * exactly that reason).
 *
 * Presentational. Holds no state of its own; takes the card and
 * the column definitions, emits click / open. The parent decides
 * whether a click should select (split-view) or navigate
 * (single-pane); double-click always signals open in split-view.
 *
 * v-memo lives at the parent's <TicketRow> invocation, not
 * inside this component, because v-memo only works on the same
 * element as the v-for (per Vue's performance guide). The
 * parent's `rowMemoKey(card)` array stays the source of truth
 * for which field changes trigger a re-render of which row.
 */
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserCell from '@/components/views/UserCell.vue'
import WorkflowStateGlyph from '@/components/views/WorkflowStateGlyph.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import { priorityForBadge, priorityLabel } from '@/utils/priorityHelpers'
import { deriveSlaState } from '@/composables/useSlaState'
import {
  formatCompactRelativeTime,
  formatCompactDate,
  formatDateTime,
} from '@nosdesk/core/utils/dateUtils'
import type { ListColumn } from '@nosdesk/core/sync/views/ticketColumns'
import type { CardData } from '@nosdesk/core/sync/views/types'

const fluent = useFluent()

const props = defineProps<{
  card: CardData
  visibleColumns: ListColumn[]
  rowClass: string
  cellPadding: string
  colStyle: (col: ListColumn) => Record<string, string>
  /** True when this row is the currently-selected ticket in
   * split-view mode. Drives the selected-row visual treatment.
   * Parent computes the boolean rather than passing the
   * selected id so this component never re-renders just because
   * a sibling row got selected. */
  selected?: boolean
  /** True when this row is in the bulk-selection set. Drives
   * a separate visual treatment (left accent stripe + tinted
   * background) so it reads as distinct from the split-view
   * "preview is open on this row" highlight. */
  bulkSelected?: boolean
  /** True when *any* row is bulk-selected. Pins the leading
   * cell into checkbox mode (instead of showing the workflow
   * glyph) so the user can keep adding to the selection without
   * having to hover each row to find the checkbox. */
  bulkActive?: boolean
}>()

const emit = defineEmits<{
  (e: 'click', id: number): void
  /** Double-click opens the full ticket (split-view mode). */
  (e: 'open', id: number): void
  (e: 'contextmenu', id: number, event: MouseEvent): void
  /** Toggle bulk-selection for this row. The parent handles
   * range-select via shiftKey; we just forward the modifier so
   * the parent knows whether to extend or single-toggle. */
  (e: 'toggle-bulk', id: number, shiftKey: boolean): void
}>()

function onContextMenu(event: MouseEvent): void {
  event.preventDefault()
  emit('contextmenu', props.card.id, event)
}

function onLeadingClick(event: MouseEvent): void {
  // The leading cell's checkbox handles its own click via the
  // input element. Keeping the cell as a separate click target
  // means the user can also click the dot-glyph area to toggle
  // selection — discoverable when no items are selected yet.
  event.stopPropagation()
  emit('toggle-bulk', props.card.id, event.shiftKey)
}

function relativeTime(iso: string): string {
  return formatCompactRelativeTime(iso)
}

function shortDate(iso: string | null | undefined): string {
  return formatCompactDate(iso) || '-'
}

function slaToneClass(card: CardData): string {
  return deriveSlaState(card)?.toneClass ?? 'text-tertiary'
}

function slaLabel(card: CardData): string {
  return deriveSlaState(card)?.compactLabel ?? '-'
}

function slaTitle(card: CardData): string {
  if (!card.sla) return ''
  if (card.sla.breached) return fluent.$t('views-ticket-row-sla-breached')
  if (card.sla.paused) return fluent.$t('views-ticket-row-sla-paused')
  return fluent.$t('views-ticket-row-sla-on-track')
}

/** Single-letter RRULE frequency code for the dense Recur
 * column. Matches the abbreviated treatment used in the
 * surrounding columns (1w, 3h, etc.) — the column is narrow
 * and a one-character glyph reads as a chip without crowding. */
function recurrenceLabel(rule: string | null | undefined): string {
  if (!rule) return ''
  const freq = rule
    .split(';')
    .map((p) => p.trim())
    .find((p) => p.toUpperCase().startsWith('FREQ='))
    ?.split('=')[1]
    ?.toUpperCase()
  if (freq === 'DAILY') return 'D'
  if (freq === 'WEEKLY') return 'W'
  if (freq === 'MONTHLY') return 'M'
  if (freq === 'YEARLY') return 'Y'
  return '↻'
}
</script>

<template>
  <tr
    class="cursor-pointer transition-colors group relative"
    :class="[
      rowClass,
      bulkSelected
        ? 'bg-accent/10 hover:bg-accent/15'
        : selected
          ? 'bg-accent/5 hover:bg-accent/10'
          : 'hover:bg-surface-hover',
    ]"
    @click="emit('click', card.id)"
    @dblclick="emit('open', card.id)"
    @contextmenu="onContextMenu"
  >
    <!--
      Leading state cell. Always-present 24px-wide indicator that
      encodes the ticket's workflow-state CATEGORY as a glyph
      (not just a hue). Lives outside the column-customisation
      v-for because (a) it's non-removable — the row's identity
      anchor across all column configs — and (b) the user can
      hide the labelled Status column without losing the at-a-
      glance state cue. Linear's "icon-only" mode pattern.

      The glyph encodes meaning in shape (dashed ring → triage,
      empty ring → backlog, half-pie → active, three-quarter pie
      → in review, filled disc + check → done, filled disc + X
      → cancelled). Hue still carries the workspace-configured
      colour — admins keep their colour control. The shape adds
      a parallel channel so:
        - colour-blind users can read state without colour;
        - users new to the workspace don't need to memorise the
          colour-to-state mapping;
        - the glyph forms a visual family across all six
          categories rather than six interchangeable dots.

      The earlier design layered a 3px SLA tone stripe on the
      cell's left edge. It was removed because the title cell
      already carries the same signal as a trailing "SLA" tag
      (line below) and the SLA column does it again when the
      column is enabled — three indicators for the same fact
      crowded the leading edge. The stripe was the weakest of
      the three (peripheral, low contrast, easy to miss) so it
      went.
    -->
    <td
      class="col-state border-b border-subtle/40 align-middle p-0 cursor-pointer"
      style="width: 24px; min-width: 24px; max-width: 24px"
      @click="onLeadingClick"
    >
      <!-- Two visual modes:
           - Default: workflow-state glyph (the row's identity
             anchor across all column configs).
           - Bulk-active OR row-hover OR this row already
             bulk-selected: a checkbox swap. Bulk-active pins
             checkbox visibility across all rows so a user
             building a selection doesn't have to hover-track
             each row. The hover swap on a single row gives the
             "I can also select this" hint when no selection
             exists yet. The bulk-selected case keeps the
             checkbox visible after the cursor moves away.

           Both elements are stacked via display swap rather
           than absolute positioning so the cell's intrinsic
           width stays the same (the table is `table-fixed`
           and the column is locked at 24px). -->
      <span
        v-if="!bulkActive && !bulkSelected"
        class="flex items-center justify-center h-full group-hover:hidden"
      >
        <WorkflowStateGlyph
          :category="card.workflow_state.category"
          :color="card.workflow_state.color"
          :name="card.workflow_state.name"
        />
      </span>
      <span
        :class="[
          'flex items-center justify-center h-full',
          bulkActive || bulkSelected ? '' : 'hidden group-hover:flex',
        ]"
      >
        <Checkbox
          :model-value="!!bulkSelected"
          size="sm"
          :aria-label="$t('views-ticket-row-select-aria', { id: card.id })"
          @change="(e: Event) => onLeadingClick(e as unknown as MouseEvent)"
        />
      </span>
    </td>

    <td
      v-for="col in visibleColumns"
      :key="col.id"
      class="border-b border-subtle/40 align-middle relative"
      :class="[
        `col-${col.id}`,
        cellPadding,
        col.align === 'center' && 'text-center',
        col.align === 'right' && 'text-right',
      ]"
      :style="colStyle(col)"
    >
      <template v-if="col.id === 'id'">
        <span class="text-tertiary font-mono text-2xs tabular-nums">#{{ card.id }}</span>
      </template>

      <template v-else-if="col.id === 'title'">
        <!-- Title cell is title. Workflow state lives in the
             Status column, priority in the Priority column —
             duplicating them as inline leading indicators created
             visual redundancy AND misaligned the title text from
             row to row (and from the header label) depending on
             which optionals were present. Trailing position keeps
             the rare recurrence + SLA signals visible without
             disturbing the primary scan target. -->
        <div class="flex items-center gap-2 min-w-0">
          <span
            class="truncate min-w-0 font-medium text-primary flex-1"
            :title="card.title"
          >{{ card.title }}</span>
          <span
            v-if="card.recurrence_rule"
            class="text-tertiary text-xs leading-none shrink-0"
            :title="$t('views-ticket-row-recurring-tooltip')"
          >↻</span>
          <span
            v-if="card.sla?.breached"
            class="text-3xs font-semibold uppercase tracking-wide text-rose-600 dark:text-rose-400 shrink-0"
            :title="$t('views-ticket-row-sla-breached-tooltip')"
          >{{ $t('views-ticket-row-sla-badge') }}</span>
          <span
            v-if="card.spam_suspected"
            class="text-3xs font-medium uppercase tracking-wide px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-700 dark:text-amber-400 shrink-0"
            :title="$t('views-ticket-row-spam-tooltip')"
          >{{ $t('views-ticket-row-spam-badge') }}</span>
        </div>
      </template>

      <template v-else-if="col.id === 'workflow_state'">
        <!-- Same glyph the leading state cell uses, downsized to
             match the row text. Reusing the component keeps the
             two surfaces in lockstep visually — the column is the
             "labelled" version of what the leading cell says. -->
        <span class="inline-flex items-center gap-1.5 text-xs text-secondary">
          <WorkflowStateGlyph
            :category="card.workflow_state.category"
            :color="card.workflow_state.color"
            :name="card.workflow_state.name"
            :size="12"
          />
          <span class="truncate">{{ card.workflow_state.name }}</span>
        </span>
      </template>

      <template v-else-if="col.id === 'priority'">
        <span class="inline-flex items-center gap-1.5 text-xs text-secondary min-w-0">
          <PriorityIndicator
            v-if="priorityForBadge(card.priority)"
            :priority="priorityForBadge(card.priority)!"
            size="xs"
          />
          <span
            v-else
            class="inline-flex w-2.5 h-2.5 items-center justify-center shrink-0"
            aria-hidden="true"
          >
            <span class="w-2 h-2 rounded-full border border-tertiary" />
          </span>
          <span class="truncate">{{ priorityLabel(card.priority) }}</span>
        </span>
      </template>

      <template v-else-if="col.id === 'assignee'">
        <UserCell :uuid="card.assignee_uuid" />
      </template>

      <template v-else-if="col.id === 'requester'">
        <UserCell :uuid="card.requester_uuid" />
      </template>

      <template v-else-if="col.id === 'category'">
        <span
          v-if="card.category_id != null"
          class="text-2xs text-secondary bg-surface-hover rounded px-1.5 py-0.5"
        >#{{ card.category_id }}</span>
        <span v-else class="text-xs text-tertiary">-</span>
      </template>

      <template v-else-if="col.id === 'cycle'">
        <span
          v-if="card.cycle_id != null"
          class="text-2xs text-accent bg-accent/10 rounded px-1.5 py-0.5"
          :title="$t('views-ticket-row-cycle-tooltip')"
        >{{ $t('views-ticket-row-cycle-label', { id: card.cycle_id }) }}</span>
        <span v-else class="text-xs text-tertiary">-</span>
      </template>

      <template v-else-if="col.id === 'due_date'">
        <span
          class="text-2xs tabular-nums"
          :class="card.due_date ? 'text-secondary' : 'text-tertiary'"
          :title="card.due_date ? formatDateTime(card.due_date) : $t('views-ticket-row-no-due-date')"
        >{{ shortDate(card.due_date) }}</span>
      </template>

      <template v-else-if="col.id === 'last_activity'">
        <span
          class="text-2xs text-tertiary tabular-nums"
          :title="formatDateTime(card.last_activity_at)"
        >{{ relativeTime(card.last_activity_at) }}</span>
      </template>

      <template v-else-if="col.id === 'created_at'">
        <span
          class="text-2xs text-tertiary tabular-nums"
          :title="formatDateTime(card.created_at)"
        >{{ relativeTime(card.created_at) }}</span>
      </template>

      <template v-else-if="col.id === 'sla'">
        <span
          v-if="card.sla"
          class="inline-flex items-center gap-1 text-2xs tabular-nums transition-colors duration-200"
          :class="slaToneClass(card)"
          :title="slaTitle(card)"
        >
          <Icon name="clock" class="w-3 h-3" />
          {{ slaLabel(card) }}
        </span>
        <span v-else class="text-xs text-tertiary">-</span>
      </template>

      <template v-else-if="col.id === 'kb_gap'">
        <span
          v-if="card.kb_gap_signal && card.kb_gap_signal !== 'none'"
          class="text-3xs font-semibold uppercase tracking-wide rounded px-1.5 py-0.5"
          :class="card.kb_gap_signal === 'strong'
            ? 'bg-amber-500/20 text-amber-700 dark:text-amber-300'
            : 'bg-surface-hover text-secondary'"
          :title="$t('views-ticket-row-kb-gap-tooltip', { signal: card.kb_gap_signal })"
        >{{ $t('views-ticket-row-kb-badge') }}</span>
        <span v-else class="text-xs text-tertiary">-</span>
      </template>

      <template v-else-if="col.id === 'devices'">
        <span
          v-if="card.affected_devices && card.affected_devices.count > 0"
          class="text-2xs text-secondary tabular-nums inline-flex items-center gap-1"
          :title="card.affected_devices.first?.name ?? $t('views-ticket-row-devices-count', { count: card.affected_devices.count })"
        >
          <Icon name="device" class="w-3 h-3" />
          {{ card.affected_devices.count }}
        </span>
        <span v-else class="text-xs text-tertiary">-</span>
      </template>

      <template v-else-if="col.id === 'recurrence'">
        <span
          v-if="card.recurrence_rule"
          class="text-3xs font-medium rounded px-1.5 py-0.5 bg-violet-500/15 text-violet-700 dark:text-violet-300"
          :title="card.recurrence_rule"
        >{{ recurrenceLabel(card.recurrence_rule) }}</span>
        <span v-else class="text-xs text-tertiary">-</span>
      </template>
    </td>
  </tr>
</template>
