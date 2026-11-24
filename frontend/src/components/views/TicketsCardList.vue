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
 *   activity timestamp, a due chip when there is a due date. Wraps
 *   on very narrow viewports so nothing gets cut.
 * - Grouped, each group gets the table's header row (chevron,
 *   label, count) and its own row loop (`TicketsCardRows`).
 *
 * Whole row is the tap target. Click opens; selection (split-
 * view) is desktop-only and not surfaced here — phones don't
 * have room for a side-by-side preview pane.
 *
 * Memo against the same field subset as the table so SSE
 * updates only re-render the affected rows.
 *
 * Under the Manual sort, ungrouped, a held row lifts and drags into an
 * order, as the table's rows do; the drop writes ranks.
 */
import Icon from '@/components/common/Icon.vue'
import TicketsCardRows from '@/components/views/TicketsCardRows.vue'
import { useToday } from '@/composables/useToday'
import type { TicketGroupBucket } from '@/composables/useTicketsGrouping'
import { computed, ref } from 'vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import { useRowReorder } from '@/sync/views/useRowReorder'
import { ranksForDrop } from '@nosdesk/core/sync/views/planOrder'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = withDefaults(defineProps<{
  cards: CardData[]
  /** Group buckets, when the list is grouped; empty renders flat. */
  buckets?: TicketGroupBucket[]
  isCollapsed?: (key: string) => boolean
  /** The sort is Manual: an ungrouped list's rows can be dragged. */
  manual?: boolean
}>(), {
  buckets: () => [],
  isCollapsed: () => false,
  manual: false,
})

// The card list owns the mobile scroll container; exposed so the
// parent view can bind pull-to-refresh to it.
const rootEl = ref<HTMLElement | null>(null)
defineExpose({ rootEl })

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'contextmenu', id: number, event: MouseEvent): void
  (e: 'toggle-bucket', key: string): void
  (e: 'patch', id: number, patch: Partial<CardData>): void
}>()

const draggable = computed<boolean>(() => props.manual && props.buckets.length === 0)

const reorder = useRowReorder({
  root: () => rootEl.value,
  onDrop: ({ id, slot }) => {
    for (const write of ranksForDrop(props.cards, id, slot)) emit('patch', write.id, { rank: write.rank })
  },
  onClick: (id) => emit('open', id),
})

/** A hold that turns into a drag must not also open the row menu. */
function onContextMenu(id: number, event: MouseEvent): void {
  if (draggable.value && (reorder.settling.value || reorder.lastPointerType() !== 'mouse')) return
  emit('contextmenu', id, event)
}

/** The last hand-placed row, when unplaced rows follow it. */
const lastPlacedId = computed<number | null>(() => {
  if (!draggable.value) return null
  const index = props.cards.findIndex((c) => c.rank == null)
  return index > 0 ? props.cards[index - 1].id : null
})

const dragged = computed(() =>
  reorder.draggedId.value != null ? props.cards.find((c) => c.id === reorder.draggedId.value) : undefined,
)

const today = useToday()

/** One headerless section when flat, else one per bucket. */
const sections = computed(() =>
  props.buckets.length === 0
    ? [{ key: 'all', label: null, cards: props.cards, collapsed: false }]
    : props.buckets.map((b) => ({
        key: b.key,
        label: b.label as string | null,
        cards: b.cards,
        collapsed: props.isCollapsed(b.key),
      })),
)
</script>

<template>
  <div ref="rootEl" class="flex-1 min-h-0 overflow-auto">
    <template v-for="section in sections" :key="section.key">
      <!-- Group header, as the table draws it: chevron, label,
           count. Absent when the list isn't grouped. -->
      <button
        v-if="section.label !== null"
        type="button"
        class="w-full flex items-center gap-2 px-3 py-1.5 bg-surface/50 border-b border-subtle text-left select-none hover:bg-surface-hover transition-colors"
        :aria-expanded="!section.collapsed"
        @click="emit('toggle-bucket', section.key)"
      >
        <Icon
          name="chevronDown"
          class="w-3 h-3 text-tertiary transition-transform"
          :class="section.collapsed && '-rotate-90'"
        />
        <span class="text-xs font-semibold text-primary">{{ section.label }}</span>
        <span class="text-2xs text-tertiary tabular-nums">{{ section.cards.length }}</span>
      </button>
      <TicketsCardRows
        v-if="!section.collapsed"
        :cards="section.cards"
        :today="today"
        :draggable="draggable"
        :dragged-id="reorder.draggedId.value"
        :last-placed-id="lastPlacedId"
        :data-lane="draggable ? 'manual' : undefined"
        @open="(id) => emit('open', id)"
        @contextmenu="onContextMenu"
        @row-down="reorder.onRowDown"
        @patch="(id, patch) => emit('patch', id, patch)"
      />
    </template>

    <Teleport to="body">
      <div
        v-if="reorder.line.value"
        class="pointer-events-none fixed z-overlay h-0.5 -translate-y-px rounded-full bg-accent"
        data-drop-line
        :style="{
          top: `${reorder.line.value.top}px`,
          left: `${reorder.line.value.left}px`,
          width: `${reorder.line.value.width}px`,
        }"
      />
    </Teleport>
    <TicketDragPreview
      v-if="dragged && reorder.state.dragPosition"
      :ticket="{
        id: dragged.id,
        number: dragged.number,
        title: dragged.title,
        category: dragged.workflow_state.category,
        assigneeUuid: dragged.assignee_uuid,
        priority: dragged.priority,
      }"
      :position="reorder.state.dragPosition"
    />

    <!-- Empty state lives in the parent (TicketsListView) so the
         contextual copy ("All caught up", "Triage is clear" etc.)
         stays consistent across the table and card surfaces. -->
  </div>
</template>
