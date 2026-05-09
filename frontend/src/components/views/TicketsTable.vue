<script setup lang="ts">
/**
 * Dense data table for the tickets list. Shell only.
 *
 * Presentational. The parent owns sort state, column layout,
 * density, grouping, and the cards array. This component
 * renders the `<table>`, the resizable / sortable headers, and
 * delegates per-row rendering to `<TicketRow>` so the flat and
 * grouped paths share one source of truth for cell markup.
 *
 * Visual hierarchy (informed by Linear / Plain / Height):
 *
 * - Each row has a 3px leading "tone strip" that encodes SLA
 *   urgency (red breached, amber at-risk). Reads as a peripheral
 *   urgency indicator without burning a column. Lives inside
 *   <TicketRow>.
 * - Title is `font-medium text-primary`; metadata cells are
 *   `text-tertiary` and a step smaller. Strong title weight is
 *   what lets a tech scan a queue of 30+ tickets.
 * - One signal, one home: workflow state lives in the Status
 *   column, priority in the Priority column. Title cell has no
 *   leading indicators (avoids visual redundancy and keeps the
 *   title text aligned with the header label).
 *
 * SSE-optimised: each `<TicketRow>` is wrapped in v-memo at the
 * v-for site here against the subset of CardData fields the row
 * actually renders, so an SSE burst that touches N tickets
 * re-renders N rows, not the whole visible list.
 *
 * `table-layout: fixed` makes the inline column widths
 * authoritative — cells respect them even when content overflows,
 * keeping the resize / reorder interactions predictable. Title
 * column flexes via `colStyle` with a 280px min-width floor;
 * lower-priority columns drop out via container queries (see the
 * <style> block) before that floor is reached.
 */
import { computed } from 'vue'
import Icon from '@/components/common/Icon.vue'
import TicketRow from '@/components/views/TicketRow.vue'
import { rowMemoKey, type ListColumn } from '@/sync/views/ticketColumns'
import type { useColumnLayout } from '@/composables/useColumnLayout'
import type { CardData } from '@/sync/views/types'
import type { GroupBucket } from '@/composables/useTicketsGrouping'

const props = defineProps<{
  cards: CardData[]
  visibleColumns: ListColumn[]
  rowClass: string
  cellPadding: string
  sortField: string
  sortDir: 'asc' | 'desc'
  layout: ReturnType<typeof useColumnLayout>
  colStyle: (col: ListColumn) => Record<string, string>
  /** When non-empty, the body renders bucket headers + cards
   * grouped under each. When empty, the body is a flat list. */
  buckets: GroupBucket[]
  isCollapsed: (key: string) => boolean
  /** Currently-selected ticket id (split-view mode). Drives the
   * "selected row" visual treatment — distinct from hover so
   * users can see the row that's powering the preview pane. */
  selectedId?: number | null
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'select', id: number): void
  (e: 'toggle-sort', field: string): void
  (e: 'toggle-bucket', key: string): void
}>()

/** When split-view is on, single-click selects (preview).
 * When off, single-click opens (full route navigation).
 * The shell decides which by binding `selectedId` —
 * presence of the prop signals split-view mode. */
const isSplitMode = computed<boolean>(
  () => props.selectedId !== undefined,
)

function onRowClick(id: number): void {
  if (isSplitMode.value) emit('select', id)
  else emit('open', id)
}

const grouped = computed<boolean>(() => props.buckets.length > 0)
</script>

<template>
  <!-- pr-2 gives the rightmost column ~8px of breathing room past
       its cellPadding, so the trailing "Updated" timestamps don't
       crowd the page edge. The container's overflow-auto still
       handles horizontal scroll on narrow viewports; the padding
       just sits inside that scroll area. -->
  <!-- The table is `min-w-full` (not `w-full`) so it can grow PAST
       the container's width when the sum of explicit column widths
       exceeds it — at which point the wrapper's `overflow-auto`
       triggers horizontal scroll. Without this, `table-fixed`
       honours the column `width` declarations against a width-
       capped table, which proportionally squeezes everything
       (including the title flex's 280px floor) and reads as a
       broken layout. The user is responsible for the layout when
       they enable many optional columns; we'd rather give them
       a horizontal scrollbar than a squashed title. -->
  <div class="tickets-table-container flex-1 min-h-0 overflow-auto pr-2">
    <table class="min-w-full text-sm border-separate border-spacing-0 table-fixed">
      <thead>
        <tr class="sticky top-0 z-10 bg-surface">
          <!-- Leading state-cell header: empty content, matches the
               24px-wide non-removable state cell rendered by every
               TicketRow. The dot itself is the column "label" — once
               users learn the colour-state mapping, the header text
               adds nothing. -->
          <th
            class="col-state border-b border-subtle bg-surface select-none p-0"
            style="width: 24px; min-width: 24px; max-width: 24px"
            aria-hidden="true"
          />
          <th
            v-for="col in visibleColumns"
            :key="col.id"
            :draggable="layout.isReorderable(col.id)"
            class="relative text-left text-[10px] font-semibold text-tertiary uppercase tracking-wider border-b border-subtle bg-surface select-none p-0"
            :class="[
              `col-${col.id}`,
              col.align === 'center' && 'text-center',
              col.align === 'right' && 'text-right',
              layout.isReorderable(col.id) && 'cursor-grab',
              layout.dragSourceId.value === col.id && 'opacity-50',
              layout.dragTargetId.value === col.id && 'bg-accent/10',
            ]"
            :style="colStyle(col)"
            @dragstart="layout.onDragStart(col.id, $event)"
            @dragover="layout.onDragOver(col.id, $event)"
            @dragleave="layout.onDragLeave(col.id)"
            @drop="layout.onDrop(col.id, $event)"
            @dragend="layout.onDragEnd"
          >
            <!--
              Sortable column: the entire cell area is the click
              target. The button is a block-level element that
              fills the cell (the th drops its own padding via
              p-0; padding moves to the button so the click target
              matches the visual cell). This way "click anywhere
              on the Title header" toggles sort, not just the
              ~40px of label text. Drag-to-reorder still fires
              on the th itself; mousedown that becomes a drag
              wins over the click before the button's @click can
              fire.
            -->
            <button
              v-if="col.sortKey"
              type="button"
              class="flex items-center gap-1 w-full h-full text-left hover:bg-surface-hover/60 hover:text-primary transition-colors"
              :class="[
                cellPadding,
                col.align === 'center' && 'justify-center text-center',
                col.align === 'right' && 'justify-end text-right',
              ]"
              :aria-sort="
                sortField === col.sortKey
                  ? (sortDir === 'asc' ? 'ascending' : 'descending')
                  : 'none'
              "
              @click="emit('toggle-sort', col.sortKey!)"
            >
              <span>{{ col.label }}</span>
              <span v-if="sortField === col.sortKey" class="text-[10px] leading-none" aria-hidden="true">
                {{ sortDir === 'asc' ? '↑' : '↓' }}
              </span>
            </button>
            <!-- Non-sortable columns get the same cell-filling
                 div so visual cell padding stays consistent. -->
            <div
              v-else
              class="flex items-center w-full h-full"
              :class="[
                cellPadding,
                col.align === 'center' && 'justify-center text-center',
                col.align === 'right' && 'justify-end text-right',
              ]"
            >
              {{ col.label }}
            </div>
            <span
              class="absolute top-0 right-0 h-full w-1.5 cursor-col-resize touch-none group/resize z-10"
              :class="layout.resizingId.value === col.id && 'bg-accent/50'"
              @pointerdown="layout.beginResize(col.id, $event)"
              @click.stop
            >
              <span
                class="absolute inset-y-1 right-0.5 w-px bg-transparent group-hover/resize:bg-accent/40 transition-colors"
              />
            </span>
          </th>
        </tr>
      </thead>
      <tbody>
        <!-- Flat (ungrouped) list -->
        <template v-if="!grouped">
          <TicketRow
            v-for="card in cards"
            :key="card.id"
            v-memo="[...rowMemoKey(card), visibleColumns.length, cellPadding, rowClass, selectedId === card.id]"
            :card="card"
            :visible-columns="visibleColumns"
            :row-class="rowClass"
            :cell-padding="cellPadding"
            :col-style="colStyle"
            :selected="selectedId === card.id"
            @click="onRowClick"
          />
        </template>

        <!-- Grouped list. Each bucket renders a sticky header row
             with chevron toggle + label + count, then the bucket's
             cards (or nothing when collapsed). -->
        <template v-else>
          <template v-for="bucket in buckets" :key="bucket.key">
            <tr
              class="bg-surface/50 border-b border-subtle cursor-pointer select-none hover:bg-surface-hover transition-colors"
              @click="emit('toggle-bucket', bucket.key)"
            >
              <td
                :colspan="visibleColumns.length"
                class="px-3 py-1.5"
              >
                <div class="flex items-center gap-2">
                  <Icon
                    name="chevronDown"
                    class="w-3 h-3 text-tertiary transition-transform"
                    :class="isCollapsed(bucket.key) && '-rotate-90'"
                  />
                  <span class="text-xs font-semibold text-primary">{{ bucket.label }}</span>
                  <span class="text-[11px] text-tertiary tabular-nums">{{ bucket.cards.length }}</span>
                </div>
              </td>
            </tr>
            <template v-if="!isCollapsed(bucket.key)">
              <TicketRow
                v-for="card in bucket.cards"
                :key="`${bucket.key}:${card.id}`"
                v-memo="[...rowMemoKey(card), visibleColumns.length, cellPadding, rowClass, selectedId === card.id]"
                :card="card"
                :visible-columns="visibleColumns"
                :row-class="rowClass"
                :cell-padding="cellPadding"
                :col-style="colStyle"
                :selected="selectedId === card.id"
                @click="onRowClick"
              />
            </template>
          </template>
        </template>
      </tbody>
    </table>
  </div>
</template>

<style>
/*
 * Priority+ progressive column hiding via CSS container queries.
 *
 * The container is the table wrapper (.tickets-table-container)
 * — `container-type: inline-size` makes its width queryable,
 * `container-name: ticket-table` lets the @container rules
 * target it specifically (avoids accidental matches from any
 * future ancestor container).
 *
 * The breakpoints are derived from the default visible column
 * widths plus title's 280px floor:
 *   id (64) + title-floor (280) + workflow_state (140) +
 *   priority (88) + assignee (140) + last_activity (96) +
 *   ~6 cells of padding ≈ 870px before things get tight.
 *
 * Hide order is reverse priority — least-essential first,
 * matching Filament Group's priority+ pattern. Title and
 * workflow_state always stay visible (workflow_state is the
 * dot in the title cell anyway). The mobile card list takes
 * over below 768px, so these rules only fire in the
 * tablet / small-desktop range.
 *
 * NOT scoped: Vue's scoped-style hash would mangle the
 * dynamically-generated `col-${id}` class selectors. The class
 * names here are namespaced enough (`tickets-table-container`,
 * `col-*`) that a global rule is safe. Container queries have
 * 95%+ browser support in 2026 — within the comfort zone for a
 * B2B helpdesk audience.
 */
.tickets-table-container {
  container-type: inline-size;
  container-name: ticket-table;
}

@container ticket-table (max-width: 1100px) {
  .col-last_activity { display: none; }
}
@container ticket-table (max-width: 960px) {
  .col-assignee { display: none; }
}
@container ticket-table (max-width: 820px) {
  /* Priority is also encoded as the inline `!` next to the
     title, so dropping the column doesn't lose information. */
  .col-priority { display: none; }
}
</style>
