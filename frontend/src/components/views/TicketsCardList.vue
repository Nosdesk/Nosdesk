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
 */
import Icon from '@/components/common/Icon.vue'
import TicketsCardRows from '@/components/views/TicketsCardRows.vue'
import { useToday } from '@/composables/useToday'
import type { TicketGroupBucket } from '@/composables/useTicketsGrouping'
import { computed, ref } from 'vue'
import type { CardData } from '@nosdesk/core/sync/views/types'

const props = withDefaults(defineProps<{
  cards: CardData[]
  /** Group buckets, when the list is grouped; empty renders flat. */
  buckets?: TicketGroupBucket[]
  isCollapsed?: (key: string) => boolean
}>(), {
  buckets: () => [],
  isCollapsed: () => false,
})

// The card list owns the mobile scroll container; exposed so the
// parent view can bind pull-to-refresh to it.
const rootEl = ref<HTMLElement | null>(null)
defineExpose({ rootEl })

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'contextmenu', id: number, event: MouseEvent): void
  (e: 'toggle-bucket', key: string): void
}>()

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
        @open="(id) => emit('open', id)"
        @contextmenu="(id, event) => emit('contextmenu', id, event)"
      />
    </template>

    <!-- Empty state lives in the parent (TicketsListView) so the
         contextual copy ("All caught up", "Triage is clear" etc.)
         stays consistent across the table and card surfaces. -->
  </div>
</template>
