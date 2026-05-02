<script setup lang="ts">
/**
 * Kanban view, sync-engine version. Reads cards from the pool,
 * dispatches drag-to-status writes through the sync queue.
 *
 * Phase 4 scope:
 * - Single-axis swimlanes by workflow_state.category.
 * - Pointer-event drag (single + multi-select).
 * - Click a card to open the detail (caller-supplied callback).
 * - Optimistic dispatch: pool flips immediately, server hears
 *   on the next push tick.
 *
 * Deferred to later commits:
 * - Two-axis swimlanes (assignee × status, etc.).
 * - SLA / KB-gap pills (need pre-computed CardData fields).
 * - Field-level presence indicators on atomic dropdowns.
 * - Keyboard parity (Option+Shift+Up/Down).
 */
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useSyncTicketsStore, type SyncTicket } from '@/sync/stores/tickets'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import {
  WORKFLOW_CATEGORIES,
  CATEGORY_LABELS,
  type WorkflowStateCategory,
  type WorkflowState,
} from '@/types/workflow'
import { paletteForColor } from '@/utils/workflowColors'
import { useDragDrop } from './drag'
import type { CardData } from './types'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'

const props = defineProps<{
  /** Cards to render. The parent route filters and orders these
   * before passing in; the view renders what it's given. */
  cards: readonly CardData[]
  /** Fires when the user clicks (not drags) a card. Parent route
   * decides what "open" means — usually router.push to detail. */
  onCardClick?: (cardId: number) => void
}>()

const ticketsStore = useSyncTicketsStore()
const workflowStatesStore = useWorkflowStatesStore()

// ---------------------------------------------------------------
// Selection (multi-select)
// ---------------------------------------------------------------

const selectedIds = ref<Set<number>>(new Set())

function toggleSelection(cardId: number, event: MouseEvent): void {
  if (!event.shiftKey && !event.metaKey && !event.ctrlKey) {
    // Plain click: replace selection with this single card.
    selectedIds.value = new Set([cardId])
    return
  }
  // Modifier-click: toggle membership without dropping the rest.
  const next = new Set(selectedIds.value)
  if (next.has(cardId)) next.delete(cardId)
  else next.add(cardId)
  selectedIds.value = next
}

function clearSelection(): void {
  selectedIds.value = new Set()
}

function isSelected(cardId: number): boolean {
  return selectedIds.value.has(cardId)
}

// ---------------------------------------------------------------
// Lanes — one column per workflow_state category that has at
// least one workflow state in it. Empty categories are hidden so
// a workspace with custom states doesn't show ghost columns.
// ---------------------------------------------------------------

interface Lane {
  id: WorkflowStateCategory
  label: string
  /** First workflow state in the category — the drop target. */
  defaultState: WorkflowState | null
  cards: CardData[]
}

const lanes = computed<Lane[]>(() => {
  const out: Lane[] = []
  const cardsByCategory = groupCardsByCategory(props.cards)
  for (const cat of WORKFLOW_CATEGORIES) {
    const states = workflowStatesStore.byCategory[cat]
    if (!states || states.length === 0) continue
    out.push({
      id: cat,
      label: CATEGORY_LABELS[cat],
      defaultState: states[0],
      cards: cardsByCategory.get(cat) ?? [],
    })
  }
  return out
})

function groupCardsByCategory(
  cards: readonly CardData[],
): Map<WorkflowStateCategory, CardData[]> {
  const grouped = new Map<WorkflowStateCategory, CardData[]>()
  for (const card of cards) {
    const cat = card.workflow_state.category
    let bucket = grouped.get(cat)
    if (!bucket) {
      bucket = []
      grouped.set(cat, bucket)
    }
    bucket.push(card)
  }
  return grouped
}

function laneCardCount(laneId: WorkflowStateCategory): number {
  return lanes.value.find((l) => l.id === laneId)?.cards.length ?? 0
}

// ---------------------------------------------------------------
// Drag-and-drop
// ---------------------------------------------------------------

function resolveLaneAt(clientX: number, clientY: number): string | null {
  const elements = document.elementsFromPoint(clientX, clientY)
  const laneEl = elements.find((el) => el.hasAttribute('data-lane-id'))
  return laneEl?.getAttribute('data-lane-id') ?? null
}

const { state: dragState, onPointerDown, isDraggedCard, isHoverLane } = useDragDrop({
  resolveLaneAt,
  selection: () => selectedIds.value,
  onClick: (cardId) => props.onCardClick?.(cardId),
  onDrop: ({ cardIds, targetLane }) => {
    const lane = lanes.value.find((l) => l.id === targetLane)
    if (!lane?.defaultState) return
    const target = lane.defaultState
    void ticketsStore.bulkMoveToWorkflowState(cardIds, {
      id: target.id,
      name: target.name,
      category: target.category,
      color: target.color,
    })
    // Dropping clears the selection so the user gets a clean state
    // for the next interaction. Multi-drag intent is "move these
    // five things" not "keep these five selected forever."
    clearSelection()
  },
})

function handleCardPointerDown(card: CardData, event: PointerEvent): void {
  // Modifier-clicks toggle selection but DON'T initiate a drag —
  // they're intent-to-select. A subsequent plain click on a
  // selected card initiates the drag for the whole group.
  if (event.shiftKey || event.metaKey || event.ctrlKey) {
    toggleSelection(card.id, event as unknown as MouseEvent)
    return
  }
  if (!isSelected(card.id) && selectedIds.value.size > 0) {
    // Click on an unselected card: replace the selection.
    selectedIds.value = new Set([card.id])
  }
  onPointerDown(card.id, event)
}

// ---------------------------------------------------------------
// Card visuals
// ---------------------------------------------------------------

const { defaultState: _defaultState } = storeToRefs(workflowStatesStore)
// _defaultState is referenced to keep the workflowStatesStore
// composable subscription alive — without at least one
// storeToRefs binding the store unsubscribes from reactivity in
// some Pinia configurations.
void _defaultState

function priorityLabel(p: CardData['priority']): string {
  return p
}

function ticketRow(cardId: number): SyncTicket | null {
  return ticketsStore.byId(cardId).value
}
</script>

<template>
  <div class="flex h-full">
    <!-- Lanes -->
    <div class="kanban-board flex gap-4 p-4 h-full overflow-x-auto" @click="clearSelection">
      <div
        v-for="lane in lanes"
        :key="lane.id"
        class="w-72 flex-shrink-0 flex flex-col bg-surface rounded-lg border border-default h-full min-h-[300px]"
        :class="{ 'ring-2 ring-accent/50': isHoverLane(lane.id) }"
        :data-lane-id="lane.id"
        @click.stop
      >
        <!-- Header -->
        <header class="flex items-center justify-between px-4 py-3 bg-surface-alt border-b border-subtle">
          <div class="flex items-center gap-3">
            <span
              v-if="lane.defaultState"
              class="inline-block w-2.5 h-2.5 rounded-full bg-current"
              :class="paletteForColor(lane.defaultState.color).solid"
              aria-hidden="true"
            />
            <h3 class="text-sm font-semibold text-primary">{{ lane.label }}</h3>
          </div>
          <span class="text-xs text-tertiary bg-surface-hover rounded-md px-2 py-1">
            {{ laneCardCount(lane.id) }}
          </span>
        </header>

        <!-- Cards -->
        <div class="flex-1 flex flex-col gap-2 p-2 overflow-y-auto">
          <article
            v-for="card in lane.cards"
            :key="card.id"
            class="bg-surface rounded-lg border border-default hover:border-strong p-3 cursor-grab select-none transition-colors"
            :class="{
              'ring-2 ring-accent': isSelected(card.id),
              'opacity-50 scale-95': isDraggedCard(card.id),
            }"
            @pointerdown.stop="handleCardPointerDown(card, $event)"
          >
            <!-- Title row -->
            <div class="flex items-start justify-between gap-2 mb-2">
              <h4 class="text-sm font-medium text-primary line-clamp-2 flex-1">
                {{ card.title }}
              </h4>
              <PriorityIndicator
                v-if="card.priority !== 'none'"
                :priority="(card.priority === 'urgent' ? 'high' : card.priority) as 'low' | 'medium' | 'high'"
                size="xs"
              />
            </div>

            <!-- Meta row -->
            <div class="flex items-center justify-between text-[11px] text-tertiary">
              <span class="font-mono">#{{ card.id }}</span>
              <UserAvatar
                v-if="card.assignee_uuid"
                :name="card.assignee_uuid"
                :avatar="null"
                size="xxs"
                :showName="false"
                :clickable="false"
              />
              <span v-else class="italic">unassigned</span>
            </div>

            <!-- Selection badge: visible only when this card is in
                 a multi-select set, so the user can confirm what
                 will move on the next drag. -->
            <span
              v-if="isSelected(card.id) && selectedIds.size > 1"
              class="absolute top-1 right-1 text-[10px] uppercase tracking-wide font-semibold text-accent bg-accent/10 rounded px-1.5 py-0.5"
            >
              {{ selectedIds.size }} selected
            </span>
          </article>

          <!-- Empty-lane drop hint -->
          <div
            v-if="lane.cards.length === 0"
            class="flex-1 flex items-center justify-center text-tertiary text-xs italic border-2 border-dashed border-subtle rounded-lg min-h-[80px]"
            :class="{ 'border-accent/50 bg-accent-muted': isHoverLane(lane.id) }"
          >
            Drop here
          </div>
        </div>
      </div>
    </div>

    <!-- Floating drag preview -->
    <div
      v-if="dragState.isDragging && dragState.dragPosition"
      class="fixed pointer-events-none z-50"
      :style="{
        left: dragState.dragPosition.x + 'px',
        top: dragState.dragPosition.y + 'px',
        transform: 'translate(-40%, -50%)',
      }"
    >
      <div class="bg-surface-alt rounded-md border border-accent shadow-lg px-2.5 py-2 max-w-[16rem]">
        <div class="text-xs font-medium text-primary line-clamp-2">
          {{ ticketRow(dragState.draggedCardIds[0])?.title ?? 'Moving cards' }}
        </div>
        <div
          v-if="dragState.draggedCardIds.length > 1"
          class="text-[10px] text-tertiary mt-0.5"
        >
          + {{ dragState.draggedCardIds.length - 1 }} more
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.kanban-board::after {
  content: '';
  flex-shrink: 0;
  width: 1px;
}
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
article {
  position: relative;
}
</style>
