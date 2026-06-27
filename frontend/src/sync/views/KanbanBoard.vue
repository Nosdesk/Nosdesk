<script setup lang="ts">
/**
 * Kanban view, sync-engine version. Reads cards from the pool,
 * dispatches drag-to-status writes through the sync queue.
 *
 * Scope:
 * - Single-axis swimlanes by workflow_state.category (default).
 * - Optional two-axis: secondary becomes sub-lanes inside each
 *   column (assignee or priority for now). Drop targets carry
 *   both axes so the dispatch flips workflow state and the
 *   secondary field in one optimistic transaction.
 * - Pointer-event drag (single + multi-select) on board cards.
 * - HTML5 drag from the recent-tickets sidebar onto lanes.
 * - Click a card to open the detail (caller-supplied callback).
 *
 * Deferred to later commits:
 * - SLA / KB-gap pills (need pre-computed CardData fields).
 * - Field-level presence indicators on atomic dropdowns.
 *
 * Keyboard parity (Option+Shift+Arrow): the selected card moves
 * one column over (Left/Right) or, when the secondary axis is
 * on, one sub-lane up/down. Mirrors the drag dispatch path; both
 * end up in `dispatchMove`.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useFluent } from 'fluent-vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

import { useSyncTicketsStore, type SyncTicket } from '@/sync/stores/tickets'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import {
  WORKFLOW_CATEGORIES,
  getCategoryLabel,
  type WorkflowStateCategory,
  type WorkflowState,
} from '@nosdesk/core/types/workflow'
import { paletteForColor } from '@nosdesk/core/utils/workflowColors'
import { formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { useDragDrop } from './drag'
import type { CardData } from './types'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import Icon from '@/components/common/Icon.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import type { DraggableTicket } from '@/composables/useTicketDrag'
import {
  isTicketDragEvent,
  parseTicketDragTransfer,
  shouldSuppressTicketDrop,
  onDragEdgeScrollTick,
  useTicketDrag,
} from '@/composables/useTicketDrag'
import { useProjectTicketLink } from '@/composables/useProjectTicketLink'

/** Sub-lane keys are derived from the secondary axis value:
 *   - assignee_uuid: the uuid string, or '__none__' if null
 *   - priority: the priority literal
 * The lane id passed to the drag layer is `<category>::<secondary>`
 * so the existing `data-lane-id` resolution works without changes.
 */
type SecondaryAxis = 'assignee_uuid' | 'priority'
const SECONDARY_NONE = '__none__'
const LANE_SEP = '::'

const props = withDefaults(defineProps<{
  /** Cards to render. The parent route filters and orders these
   * before passing in; the view renders what it's given. */
  cards: readonly CardData[]
  /** Fires when the user clicks (not drags) a card. Parent route
   * decides what "open" means — usually router.push to detail. */
  onCardClick?: (cardId: number) => void
  /** Optional secondary axis. When set, columns split into
   * sub-lanes by the field's value; dropping into a sub-lane
   * patches the field in addition to moving workflow state. */
  secondaryGroupBy?: SecondaryAxis | null
  /** Optional column quick-add. When provided, each column header
   * shows a `+` that opens an inline title input; submitting calls
   * this with the column's default workflow state. The board hides
   * the affordance entirely when this is omitted. */
  onQuickAdd?: (workflowStateId: number, title: string) => Promise<void> | void
  /** When set, HTML5 drops from the recent-tickets sidebar link the
   * ticket to this project before moving it into the target column. */
  projectId?: number
}>(), {
  onCardClick: undefined,
  secondaryGroupBy: null,
  onQuickAdd: undefined,
  projectId: undefined,
})

const ticketsStore = useSyncTicketsStore()
const workflowStatesStore = useWorkflowStatesStore()
const { linkToProject } = useProjectTicketLink()

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

interface SubLane {
  /** Drop-target id: `<category>::<secondary>`. */
  id: string
  /** Secondary axis bucket key (uuid, priority literal, or
   * SECONDARY_NONE). Carried so the drop handler can patch the
   * right field without re-parsing the id. */
  secondaryKey: string
  label: string
  cards: CardData[]
}

interface Lane {
  id: WorkflowStateCategory
  label: string
  /** First workflow state in the category — the drop target. */
  defaultState: WorkflowState | null
  /** Either a single sublane (secondary axis off) or one sublane
   * per secondary value present in this column's cards. */
  sublanes: SubLane[]
  totalCards: number
}

const lanes = computed<Lane[]>(() => {
  const out: Lane[] = []
  const cardsByCategory = groupCardsByCategory(props.cards)
  const secondary = props.secondaryGroupBy
  for (const cat of WORKFLOW_CATEGORIES) {
    const states = workflowStatesStore.byCategory[cat]
    if (!states || states.length === 0) continue
    const cards = cardsByCategory.get(cat) ?? []
    const sublanes = secondary
      ? buildSubLanes(cat, cards, secondary)
      : [{
          id: `${cat}${LANE_SEP}${SECONDARY_NONE}`,
          secondaryKey: SECONDARY_NONE,
          label: '',
          cards,
        }]
    out.push({
      id: cat,
      label: getCategoryLabel(cat),
      defaultState: states[0],
      sublanes,
      totalCards: cards.length,
    })
  }
  return out
})

function buildSubLanes(
  cat: WorkflowStateCategory,
  cards: CardData[],
  axis: SecondaryAxis,
): SubLane[] {
  const buckets = new Map<string, CardData[]>()
  for (const card of cards) {
    const key = secondaryKey(card, axis)
    let bucket = buckets.get(key)
    if (!bucket) {
      bucket = []
      buckets.set(key, bucket)
    }
    bucket.push(card)
  }
  // Always render an empty unassigned/none sub-lane so the user can
  // drop a card into it to clear the field. Without this seeding
  // there'd be no drop target for "make this unassigned."
  if (!buckets.has(SECONDARY_NONE)) buckets.set(SECONDARY_NONE, [])
  const sorted = Array.from(buckets.entries()).sort(([a], [b]) => {
    if (a === SECONDARY_NONE) return 1
    if (b === SECONDARY_NONE) return -1
    return a.localeCompare(b)
  })
  return sorted.map(([key, bucketCards]) => ({
    id: `${cat}${LANE_SEP}${key}`,
    secondaryKey: key,
    label: secondaryLabel(key, axis),
    cards: bucketCards,
  }))
}

function secondaryKey(card: CardData, axis: SecondaryAxis): string {
  if (axis === 'assignee_uuid') return card.assignee_uuid ?? SECONDARY_NONE
  return card.priority ?? SECONDARY_NONE
}

function secondaryLabel(key: string, axis: SecondaryAxis): string {
  if (key === SECONDARY_NONE) {
    return axis === 'assignee_uuid'
      ? t('filter-assignee-unassigned')
      : t('priority-none')
  }
  if (axis === 'assignee_uuid') return key.slice(0, 8)
  return key
}

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

// ---------------------------------------------------------------
// Drag-and-drop
// ---------------------------------------------------------------

function resolveLaneAt(clientX: number, clientY: number): string | null {
  const elements = document.elementsFromPoint(clientX, clientY)
  const laneEl = elements.find((el) => el.hasAttribute('data-lane-id'))
  if (laneEl) return laneEl.getAttribute('data-lane-id')

  // Drops on the column header / chrome still target the column's
  // default sub-lane when cards haven't scrolled under the pointer.
  const colEl = elements.find((el) => el.hasAttribute('data-column-id'))
  const columnId = colEl?.getAttribute('data-column-id')
  if (columnId) {
    const lane = lanes.value.find((l) => l.id === columnId)
    return lane?.sublanes[0]?.id ?? null
  }
  return null
}

/** Single dispatch path used by both pointer drops and keyboard
 * shortcuts. Takes the target lane id (`<category>::<secondary>`
 * format) and the cards to move; flips workflow state and, when
 * the secondary axis is on, patches the secondary field.
 */
function dispatchMove(cardIds: number[], targetLaneId: string): void {
  const [categoryId, secondaryKey] = parseLaneId(targetLaneId)
  const lane = lanes.value.find((l) => l.id === categoryId)
  if (!lane?.defaultState) return
  const target = lane.defaultState
  void ticketsStore.bulkMoveToWorkflowState(cardIds, {
    id: target.id,
    name: target.name,
    category: target.category,
    color: target.color,
  })
  if (props.secondaryGroupBy && secondaryKey !== SECONDARY_NONE) {
    const patch: { assignee_uuid?: string | null; priority?: CardData['priority'] } = {}
    if (props.secondaryGroupBy === 'assignee_uuid') {
      patch.assignee_uuid = secondaryKey
    } else {
      patch.priority = secondaryKey as CardData['priority']
    }
    for (const id of cardIds) void ticketsStore.patchKanbanFields(id, patch)
  } else if (props.secondaryGroupBy && secondaryKey === SECONDARY_NONE) {
    // Drop into the "none" sub-lane clears the secondary field.
    // Priority's "no priority" is the literal 'none', whereas
    // assignee uses null — keep them distinguishable here.
    const patch = props.secondaryGroupBy === 'assignee_uuid'
      ? { assignee_uuid: null }
      : { priority: 'none' as const }
    for (const id of cardIds) void ticketsStore.patchKanbanFields(id, patch)
  }
}

const cardsById = computed(() => {
  const map = new Map<number, CardData>()
  for (const card of props.cards) map.set(card.id, card)
  return map
})

function cardToDragPreview(card: CardData): DraggableTicket {
  const priority = card.priority
  return {
    id: card.id,
    title: card.title,
    category: card.workflow_state.category,
    assigneeUuid: card.assignee_uuid ?? null,
    priority: priority === 'urgent' ? 'high' : priority,
  }
}

const { state: dragState, onPointerDown, isDraggedCard, isHoverLane } = useDragDrop({
  resolveLaneAt,
  selection: () => selectedIds.value,
  onClick: (cardId) => props.onCardClick?.(cardId),
  onDrop: ({ cardIds, targetLane }) => {
    dispatchMove(cardIds, targetLane)
    // Dropping clears the selection so the user gets a clean state
    // for the next interaction. Multi-drag intent is "move these
    // five things" not "keep these five selected forever."
    clearSelection()
  },
})

const kanbanDragPreview = computed(() => {
  if (!dragState.isDragging || !dragState.dragPosition) return null
  const leadId = dragState.draggedCardIds[0]
  if (leadId == null) return null
  const card = cardsById.value.get(leadId)
  if (!card) return null
  return {
    ticket: cardToDragPreview(card),
    position: dragState.dragPosition,
    extraCount: Math.max(0, dragState.draggedCardIds.length - 1),
  }
})

const { endDrag: endExternalTicketDrag } = useTicketDrag()
const externalDragHoverLane = ref<string | null>(null)

function isLaneHovered(laneId: string): boolean {
  return isHoverLane(laneId) || externalDragHoverLane.value === laneId
}

function onExternalDragOver(event: DragEvent): void {
  if (!isTicketDragEvent(event)) return
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  externalDragHoverLane.value = resolveLaneAt(event.clientX, event.clientY)
}

function onExternalDrop(event: DragEvent): void {
  if (!isTicketDragEvent(event)) return
  event.preventDefault()
  event.stopPropagation()

  if (shouldSuppressTicketDrop()) {
    externalDragHoverLane.value = null
    endExternalTicketDrag()
    return
  }

  void (async () => {
    const ticketId = parseTicketDragTransfer(event.dataTransfer)
    const laneId = resolveLaneAt(event.clientX, event.clientY) ?? externalDragHoverLane.value
    externalDragHoverLane.value = null

    if (ticketId == null || !laneId) {
      endExternalTicketDrag()
      return
    }

    if (props.projectId != null) {
      const linked = await linkToProject(props.projectId, ticketId)
      if (!linked) {
        endExternalTicketDrag()
        return
      }
    }

    await ticketsStore.ensureInPool(ticketId)
    dispatchMove([ticketId], laneId)
    endExternalTicketDrag()
  })()
}

function onExternalDragEnd(): void {
  externalDragHoverLane.value = null
  endExternalTicketDrag()
}

function parseLaneId(laneId: string): [WorkflowStateCategory, string] {
  const idx = laneId.indexOf(LANE_SEP)
  if (idx < 0) return [laneId as WorkflowStateCategory, SECONDARY_NONE]
  return [
    laneId.slice(0, idx) as WorkflowStateCategory,
    laneId.slice(idx + LANE_SEP.length),
  ]
}

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
// Keyboard parity: Option/Alt+Shift+Arrow moves the selected
// cards. Left/Right walks columns; Up/Down walks sub-lanes inside
// the current column when the secondary axis is on (otherwise
// they're no-ops). Mirrors Linear's drag-keyboard pairing.
// ---------------------------------------------------------------

function findCardLane(cardId: number): { laneIdx: number; sublaneIdx: number } | null {
  for (let li = 0; li < lanes.value.length; li++) {
    const lane = lanes.value[li]
    for (let si = 0; si < lane.sublanes.length; si++) {
      if (lane.sublanes[si].cards.some((c) => c.id === cardId)) {
        return { laneIdx: li, sublaneIdx: si }
      }
    }
  }
  return null
}

function moveSelectionByKey(direction: 'left' | 'right' | 'up' | 'down'): void {
  if (selectedIds.value.size === 0) return
  // Anchor on the first selected card so the group moves as a
  // unit. Mixed-lane selections all land in the same target lane.
  const anchorId = selectedIds.value.values().next().value as number
  const pos = findCardLane(anchorId)
  if (!pos) return
  let targetLane = pos.laneIdx
  let targetSub = pos.sublaneIdx
  if (direction === 'left') targetLane = Math.max(0, pos.laneIdx - 1)
  if (direction === 'right') targetLane = Math.min(lanes.value.length - 1, pos.laneIdx + 1)
  if (direction === 'up' || direction === 'down') {
    if (!props.secondaryGroupBy) return
    const subCount = lanes.value[pos.laneIdx].sublanes.length
    targetSub = direction === 'up'
      ? Math.max(0, pos.sublaneIdx - 1)
      : Math.min(subCount - 1, pos.sublaneIdx + 1)
  }
  if (targetLane === pos.laneIdx && targetSub === pos.sublaneIdx) return
  const dest = lanes.value[targetLane]?.sublanes[targetSub]
  if (!dest) return
  dispatchMove(Array.from(selectedIds.value), dest.id)
  // Keyboard moves keep the selection so the user can chain
  // moves; Linear ships this behaviour and it lets you chord
  // Option-Shift-Right twice in a row to skip a column.
}

function isModalActive(): boolean {
  // If the user is typing in an input or editing in a content-
  // editable region, never hijack the keys.
  const el = document.activeElement as HTMLElement | null
  if (!el) return false
  const tag = el.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  return el.isContentEditable === true
}

function onKeyDown(event: KeyboardEvent): void {
  // Shift + Alt (Option on macOS) is the gesture. Avoids colliding
  // with browser back/forward (Cmd+Arrow) and word-wise text nav
  // (Option+Arrow alone).
  if (!event.altKey || !event.shiftKey) return
  if (isModalActive()) return
  let direction: 'left' | 'right' | 'up' | 'down' | null = null
  if (event.key === 'ArrowLeft') direction = 'left'
  else if (event.key === 'ArrowRight') direction = 'right'
  else if (event.key === 'ArrowUp') direction = 'up'
  else if (event.key === 'ArrowDown') direction = 'down'
  if (!direction) return
  event.preventDefault()
  moveSelectionByKey(direction)
}

onMounted(() => {
  window.addEventListener('keydown', onKeyDown)
  document.addEventListener('dragend', onExternalDragEnd)
  onDragEdgeScrollTick((clientX, clientY) => {
    externalDragHoverLane.value = resolveLaneAt(clientX, clientY)
  })
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeyDown)
  document.removeEventListener('dragend', onExternalDragEnd)
  onDragEdgeScrollTick(null)
})

// ---------------------------------------------------------------
// Card visuals
// ---------------------------------------------------------------

const { defaultState: _defaultState } = storeToRefs(workflowStatesStore)
// _defaultState is referenced to keep the workflowStatesStore
// composable subscription alive — without at least one
// storeToRefs binding the store unsubscribes from reactivity in
// some Pinia configurations.
void _defaultState

// ---------------------------------------------------------------
// Column composer (only active when props.onQuickAdd is set)
//
// One input per column does two things: type a title and create a
// new ticket, or search the workspace pool and pull an existing
// ticket into this column. Add-existing only applies when the board
// is project-scoped (projectId set) — it reuses the same link-then-
// move path as the sidebar drag (`onExternalDrop`).
// ---------------------------------------------------------------

/** The lane category whose inline input is currently open, or null
 * when no column is in composer mode. */
const quickAddCategory = ref<WorkflowStateCategory | null>(null)
const quickAddTitle = ref('')
/** Highlighted option in the composer dropdown. Index 0 is always
 * the "create" row; 1..n map to `quickAddMatches`. */
const quickAddHighlight = ref(0)

/** Ids already on this board, so the existing-search never offers a
 * ticket that's already in the project. */
const boardTicketIds = computed(() => new Set(props.cards.map((c) => c.id)))
const allTickets = ticketsStore.all()

/** Existing tickets to offer in the composer dropdown, excluding ones
 * already on the board. Pool-backed, so results are instant — no fetch.
 * Only meaningful when the board is project-scoped (nothing to link to
 * otherwise) and a composer is open. With no query typed, surfaces the
 * most recently active tickets so the list is useful the moment the
 * composer opens; once the user types, switches to title/id matching. */
const quickAddMatches = computed<SyncTicket[]>(() => {
  if (props.projectId == null || quickAddCategory.value == null) return []
  const q = quickAddTitle.value.trim().toLowerCase()
  const seen = boardTicketIds.value
  if (!q) {
    return [...allTickets.value]
      .filter((tkt) => !seen.has(tkt.id))
      .sort((a, b) => b.last_activity_at.localeCompare(a.last_activity_at))
      .slice(0, 6)
  }
  const out: SyncTicket[] = []
  for (const tkt of allTickets.value) {
    if (seen.has(tkt.id)) continue
    if (String(tkt.id) === q || tkt.title.toLowerCase().includes(q)) {
      out.push(tkt)
      if (out.length >= 6) break
    }
  }
  return out
})

/** Whether the create row should show: a non-empty title and a
 * column with a default state to create into. */
const quickAddCanCreate = computed(
  () => quickAddTitle.value.trim().length > 0,
)

/** Total navigable options = create row (when valid) + matches. */
const quickAddOptionCount = computed(
  () => (quickAddCanCreate.value ? 1 : 0) + quickAddMatches.value.length,
)

const quickAddOpen = computed(
  () => quickAddCanCreate.value || quickAddMatches.value.length > 0,
)

// Typing resets the cursor to the create row so Enter creates by
// default; arrows then move down into the existing matches. With the
// field empty the dropdown shows recent tickets as a convenience, but
// nothing is pre-highlighted (-1) so a blind Enter never links the top
// row — the user clicks or arrows down to pick one.
watch(quickAddTitle, () => {
  quickAddHighlight.value = quickAddTitle.value.trim() ? 0 : -1
})

function openQuickAdd(cat: WorkflowStateCategory): void {
  quickAddCategory.value = cat
  quickAddTitle.value = ''
  quickAddHighlight.value = -1
}

function closeQuickAdd(): void {
  quickAddCategory.value = null
  quickAddTitle.value = ''
  quickAddHighlight.value = 0
}

/** Function-ref: focus the input the moment it mounts so the user
 * can type immediately after clicking `+`. */
function focusQuickAdd(el: Element | null): void {
  if (el instanceof HTMLInputElement) el.focus()
}

/** Move the dropdown highlight; wraps around the option list. */
function quickAddNav(delta: number): void {
  const count = quickAddOptionCount.value
  if (count === 0) return
  // The create row sits at index 0 when present; when it's absent
  // (no title typed but matches exist — not currently reachable, but
  // guarded anyway) navigation still stays in range.
  quickAddHighlight.value = (quickAddHighlight.value + delta + count) % count
}

/** Create a new ticket in the column's default state. Clears the
 * field but keeps the composer open for rapid successive adds. The
 * card lands optimistically via the parent's onQuickAdd handler and
 * is reconciled by the sync frame. */
async function submitQuickAdd(lane: Lane): Promise<void> {
  const title = quickAddTitle.value.trim()
  if (!title || !lane.defaultState) return
  quickAddTitle.value = ''
  quickAddHighlight.value = 0
  await props.onQuickAdd?.(lane.defaultState.id, title)
}

/** Link an existing ticket into this project and drop it in the
 * column. Mirrors the sidebar-drag path (`onExternalDrop`): optimistic
 * link, ensure the row is in the pool, then move to the column state. */
async function addExisting(lane: Lane, ticketId: number): Promise<void> {
  if (!lane.defaultState) return
  quickAddTitle.value = ''
  quickAddHighlight.value = 0
  if (props.projectId != null) {
    const linked = await linkToProject(props.projectId, ticketId)
    if (!linked) return
  }
  await ticketsStore.ensureInPool(ticketId)
  const s = lane.defaultState
  void ticketsStore.moveToWorkflowState(ticketId, {
    id: s.id,
    name: s.name,
    category: s.category,
    color: s.color,
  })
}

/** Enter / click commits the highlighted option. */
async function commitQuickAdd(lane: Lane): Promise<void> {
  const matchIndex = quickAddCanCreate.value
    ? quickAddHighlight.value - 1
    : quickAddHighlight.value
  const match = quickAddMatches.value[matchIndex]
  if (match) {
    await addExisting(lane, match.id)
  } else {
    await submitQuickAdd(lane)
  }
}

/** True when the given match index is the highlighted row. */
function isMatchHighlighted(i: number): boolean {
  const idx = quickAddCanCreate.value ? quickAddHighlight.value - 1 : quickAddHighlight.value
  return idx === i
}

const isCreateHighlighted = computed(
  () => quickAddCanCreate.value && quickAddHighlight.value === 0,
)

/** Esc closes; blur closes only when nothing has been typed, so a
 * blur mid-type (e.g. a transient focus shift) doesn't discard the
 * column the user is working in. Dropdown rows use mousedown.prevent
 * so a click commits without first blurring the input shut. */
function onQuickAddBlur(): void {
  if (!quickAddTitle.value.trim()) closeQuickAdd()
}

/** Card-level "is there anything noteworthy?" check that drives
 * the optional pills row. SLA + recurrence ride the title row
 * (icon + corner indicator respectively); KB-gap and assets get
 * the pill row when they're noteworthy enough to surface. */
function hasPills(card: CardData): boolean {
  const hasGap = !!card.kb_gap_signal && card.kb_gap_signal !== 'none'
  const hasDevices = !!card.affected_devices && card.affected_devices.count > 0
  return hasGap || hasDevices
}

/** SLA tone for the small clock icon next to the priority dot.
 * Only red and amber surface visually; green / paused stay
 * neutral so the eye picks out at-risk cards at a glance. */
function slaIconTone(card: CardData): string {
  const sla = card.sla
  if (!sla) return ''
  if (sla.breached) return 'text-rose-600 dark:text-rose-400'
  if (sla.pill_color === 'amber') return 'text-amber-600 dark:text-amber-400'
  return 'text-tertiary'
}

function slaTooltip(card: CardData): string {
  const sla = card.sla
  if (!sla) return ''
  const target = formatDateTime(sla.target_at)
  if (sla.breached) return `SLA breached (target ${target})`
  if (sla.paused) return `SLA paused (target ${target})`
  const remaining = sla.seconds_remaining ?? 0
  let formatted = ''
  if (remaining < 3600) formatted = `${Math.ceil(remaining / 60)}m`
  else if (remaining < 86_400) formatted = `${Math.ceil(remaining / 3600)}h`
  else formatted = `${Math.ceil(remaining / 86_400)}d`
  return `SLA: ${formatted} until ${target}`
}

function kbGapClass(signal: 'weak' | 'strong'): string {
  // Strong is a louder amber so the eye picks it out at a glance;
  // weak is a quieter slate that still reads as a flag without
  // colliding with the urgent priority indicator.
  return signal === 'strong'
    ? 'bg-amber-500/20 text-amber-700 dark:text-amber-300'
    : 'bg-surface-hover text-secondary'
}

function affectedDevicesTooltip(card: CardData): string {
  const summary = card.affected_devices
  if (!summary) return ''
  const first = summary.first?.name ?? 'device'
  if (summary.count === 1) return first
  return `${first} +${summary.count - 1} more`
}
</script>

<template>
  <div class="flex flex-1 flex-col min-h-0 min-w-0 overflow-hidden">
    <!-- Single scroll surface for the whole board (horizontal + vertical).
         Lanes grow with their cards — no nested column scrollports. Column
         headers use position:sticky against this element so they pin while
         the board scrolls. -->
    <div
      class="kanban-board flex flex-1 min-h-0 min-w-0 overflow-x-auto overflow-y-auto max-md:snap-x max-md:snap-mandatory"
      @click="clearSelection"
      @dragover="onExternalDragOver"
      @drop="onExternalDrop"
    >
      <div class="kanban-board-inner flex min-h-full min-w-min gap-3 p-3 items-start max-md:scroll-px-3">
      <div
        v-for="lane in lanes"
        :key="lane.id"
        :data-column-id="lane.id"
        class="kanban-lane w-72 shrink-0 flex flex-col bg-surface rounded-lg border border-default min-h-[22rem] max-md:snap-start"
        @click.stop
      >
        <!-- Top radius pad — scrolls away; keeps rounded corners without
             clipping sticky descendants. -->
        <div class="kanban-lane-cap kanban-lane-cap-top shrink-0" aria-hidden="true" />
        <header
          class="kanban-lane-header sticky z-20 flex items-center justify-between gap-1.5 px-2 bg-surface-alt shrink-0"
        >
          <div class="flex items-center gap-1.5 min-w-0">
            <span
              v-if="lane.defaultState"
              class="inline-block w-1.5 h-1.5 rounded-full bg-current shrink-0"
              :class="paletteForColor(lane.defaultState.color).solid"
              aria-hidden="true"
            />
            <h3 class="text-xs font-semibold text-primary truncate leading-none">
              {{ lane.label }}
            </h3>
          </div>
          <div class="flex items-center gap-1 shrink-0">
            <span class="text-[10px] text-tertiary tabular-nums leading-none">{{ lane.totalCards }}</span>
          </div>
        </header>
        <!-- Bottom pad — scrolls away with the top pad. -->
        <div class="kanban-lane-cap kanban-lane-cap-bottom shrink-0" aria-hidden="true" />
        <!-- Sticky hairline: pins below the title row once padding has scrolled off. -->
        <div class="kanban-lane-hairline sticky z-20 shrink-0" aria-hidden="true" />

        <div class="flex flex-col">
        <!-- Sub-lanes (one when secondary axis is off, many when on) -->
        <section
          v-for="sublane in lane.sublanes"
          :key="sublane.id"
          class="flex flex-col transition-colors"
          :class="{ 'bg-accent-muted/40': isLaneHovered(sublane.id) }"
          :data-lane-id="sublane.id"
        >
          <header
            v-if="secondaryGroupBy"
            class="kanban-sublane-header flex items-center justify-between px-2 py-0.5 text-[10px] uppercase tracking-wide font-semibold text-tertiary bg-surface border-b border-subtle sticky z-10 shrink-0"
          >
            <span class="truncate">{{ sublane.label }}</span>
            <span class="tabular-nums">{{ sublane.cards.length }}</span>
          </header>

          <div class="flex flex-col gap-1.5 p-1.5">
              <!-- Insertion line: a drop into a non-empty lane bumps
                   last_activity_at to NOW so the card lands at the
                   top of the lane. The line points there honestly
                   instead of pretending to support arbitrary in-lane
                   reorder, which the data model doesn't yet. -->
              <div
                v-if="isLaneHovered(sublane.id) && sublane.cards.length > 0"
                class="h-0.5 -my-1 rounded-full bg-accent shadow-[0_0_0_2px_var(--surface-app)] insertion-line"
                aria-hidden="true"
              />
              <article
                v-for="card in sublane.cards"
                :key="card.id"
                class="bg-surface rounded-lg border border-default hover:border-strong p-2 cursor-grab select-none transition-colors"
                :class="{
                  'ring-2 ring-accent': isSelected(card.id),
                  'opacity-50 scale-95': isDraggedCard(card.id),
                }"
                @pointerdown.stop="handleCardPointerDown(card, $event)"
              >
                <!-- Title row carries the title + the small,
                     always-relevant indicators (priority, SLA
                     status, recurrence). SLA is icon-only with a
                     tooltip — the colour does the talking; full
                     countdown text lives in the detail view. -->
                <div class="flex items-start justify-between gap-1.5 mb-1">
                  <h4 class="text-[13px] font-medium text-primary line-clamp-2 flex-1 inline-flex items-baseline gap-1">
                    <span
                      v-if="card.recurrence_rule"
                      class="text-tertiary text-xs shrink-0"
                      :title="t('kanban-recurring-tooltip')"
                      :aria-label="t('kanban-recurring-aria')"
                    >↻</span>
                    <span class="flex-1">{{ card.title }}</span>
                  </h4>
                  <div class="flex items-center gap-1.5 shrink-0">
                    <span
                      v-if="card.sla"
                      :class="slaIconTone(card)"
                      :title="slaTooltip(card)"
                    >
                      <Icon
                        name="clock"
                        size="xs"
                        :label="t('kanban-sla-aria')"
                      />
                    </span>
                    <PriorityIndicator
                      v-if="card.priority !== 'none'"
                      :priority="(card.priority === 'urgent' ? 'high' : card.priority) as 'low' | 'medium' | 'high'"
                      size="xs"
                    />
                  </div>
                </div>

                <!-- Pills row: only shown when knowledge-gap or
                     asset-link counts are noteworthy. Skipped
                     entirely on the common case so the layout
                     stays tight. -->
                <div
                  v-if="hasPills(card)"
                  class="flex items-center gap-1 mb-1"
                >
                  <span
                    v-if="card.kb_gap_signal && card.kb_gap_signal !== 'none'"
                    class="text-[10px] font-medium rounded px-1.5 py-0.5 inline-flex items-center gap-1"
                    :class="kbGapClass(card.kb_gap_signal)"
                    :title="`${card.kb_gap_signal} knowledge gap signal`"
                  >
                    <span aria-hidden="true">?</span>
                    KB
                  </span>
                  <span
                    v-if="card.affected_devices && card.affected_devices.count > 0"
                    class="text-[10px] font-medium rounded px-1.5 py-0.5 bg-surface-hover text-secondary inline-flex items-center gap-1"
                    :title="affectedDevicesTooltip(card)"
                  >
                    <span aria-hidden="true">▢</span>
                    {{ card.affected_devices.count }}
                  </span>
                </div>

                <!-- Meta row -->
                <div class="flex items-center justify-between text-[11px] text-tertiary">
                  <span class="font-mono">#{{ card.id }}</span>
                  <UserAvatar
                    v-if="card.assignee_uuid"
                    :uuid="card.assignee_uuid"
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

              <!-- Empty-sublane drop target. Stays quiet when idle so
                   the column's "+ Add ticket" footer reads as the
                   primary affordance; only reveals the "Drop here"
                   accent while a card is dragged over it. -->
              <div
                v-if="sublane.cards.length === 0"
                class="flex items-center justify-center text-[11px] italic border-2 rounded-lg min-h-12 transition-colors"
                :class="
                  isLaneHovered(sublane.id)
                    ? 'border-accent bg-accent-muted text-accent font-medium'
                    : 'border-dashed border-subtle text-transparent'
                "
              >
                {{ isLaneHovered(sublane.id) ? t('kanban-drop-here') : '' }}
              </div>
            </div>
          </section>

          <!-- Add-ticket footer: a persistent, labelled affordance at
               the foot of every column — the primary way into the
               composer. Click to type a title (creates a ticket in
               this column) or, on a project board, search to pull an
               existing ticket in. With nothing typed the dropdown
               lists the most recently active tickets. It opens upward
               so it never spills past a tall column, and stays open
               after each add for rapid successive entry. -->
          <div
            v-if="onQuickAdd && lane.defaultState"
            class="relative px-1.5 pb-1.5 pt-0.5 shrink-0"
          >
            <template v-if="quickAddCategory === lane.id">
              <input
                :ref="(el) => focusQuickAdd(el as Element | null)"
                v-model="quickAddTitle"
                type="text"
                class="w-full text-[13px] rounded-md border border-default bg-surface px-2 py-1 text-primary placeholder:text-tertiary focus:outline-none focus:ring-2 focus:ring-accent"
                :placeholder="projectId != null ? t('kanban-composer-placeholder') : t('kanban-quick-add-placeholder')"
                @keydown.enter.prevent="commitQuickAdd(lane)"
                @keydown.down.prevent="quickAddNav(1)"
                @keydown.up.prevent="quickAddNav(-1)"
                @keydown.esc.prevent="closeQuickAdd"
                @blur="onQuickAddBlur"
              />

              <!-- Suggestions: create row + existing-ticket matches.
                   mousedown.prevent keeps the input focused so the click
                   commits instead of blurring the composer shut first. -->
              <div
                v-if="quickAddOpen"
                class="absolute left-1.5 right-1.5 bottom-full z-30 mb-1 rounded-md border border-default bg-surface shadow-lg overflow-hidden"
              >
                <button
                  v-if="quickAddCanCreate"
                  type="button"
                  class="w-full flex items-center gap-1.5 px-2 py-1.5 text-left text-[13px] text-primary"
                  :class="isCreateHighlighted ? 'bg-accent-muted' : 'hover:bg-surface-hover'"
                  @mousedown.prevent="submitQuickAdd(lane)"
                >
                  <span class="text-tertiary shrink-0">+</span>
                  <span class="truncate">{{ t('kanban-composer-create', { title: quickAddTitle.trim() }) }}</span>
                </button>

                <div
                  v-if="quickAddMatches.length > 0"
                  class="px-2 pt-1 pb-0.5 text-[10px] uppercase tracking-wide text-tertiary border-t border-subtle"
                >
                  {{ quickAddTitle.trim() ? t('kanban-composer-existing') : t('kanban-composer-recent') }}
                </div>
                <button
                  v-for="(match, i) in quickAddMatches"
                  :key="match.id"
                  type="button"
                  class="w-full flex items-center gap-1.5 px-2 py-1.5 text-left text-[13px]"
                  :class="isMatchHighlighted(i) ? 'bg-accent-muted' : 'hover:bg-surface-hover'"
                  @mousedown.prevent="addExisting(lane, match.id)"
                >
                  <span class="text-tertiary tabular-nums shrink-0">#{{ match.id }}</span>
                  <span class="truncate text-primary">{{ match.title }}</span>
                </button>
              </div>
            </template>

            <button
              v-else
              type="button"
              class="w-full flex items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-[13px] text-tertiary hover:text-primary hover:bg-surface-hover transition-colors"
              :aria-label="t('kanban-quick-add-aria', { column: lane.label })"
              @click.stop="openQuickAdd(lane.id)"
            >
              <span class="text-sm leading-none">+</span>
              <span>{{ t('kanban-add-ticket') }}</span>
            </button>
          </div>
        </div>
      </div>
      </div>
    </div>

    <!-- Floating drag preview -->
    <TicketDragPreview
      v-if="kanbanDragPreview"
      :ticket="kanbanDragPreview.ticket"
      :position="kanbanDragPreview.position"
      :extra-count="kanbanDragPreview.extraCount"
    />
  </div>
</template>

<style scoped>
.kanban-lane {
  /* Shared sticky offsets for the title bar + hairline + sub-lane headers. */
  --kanban-lane-header-h: 1.75rem;
  --kanban-lane-cap: 0.375rem;
  min-height: 22rem;
}
.kanban-lane-cap {
  background: var(--color-surface-alt);
  padding-inline: 0.5rem;
}
.kanban-lane-cap-top {
  padding-top: var(--kanban-lane-cap);
  border-top-left-radius: 0.5rem;
  border-top-right-radius: 0.5rem;
}
.kanban-lane-cap-bottom {
  padding-bottom: var(--kanban-lane-cap);
}
.kanban-lane-header {
  position: sticky;
  top: 0;
  height: var(--kanban-lane-header-h);
}
.kanban-lane-hairline {
  position: sticky;
  top: var(--kanban-lane-header-h);
  height: 1px;
  background: var(--color-border-default);
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.04);
}
.kanban-sublane-header {
  position: sticky;
  top: calc(var(--kanban-lane-header-h) + 1px);
}
.kanban-board-inner::after {
  content: '';
  flex-shrink: 0;
  width: 1px;
}
/* Pulse the insertion line so it reads as "live" and not just a
   thin border. 0.6s is fast enough to feel responsive but slow
   enough not to strobe. */
.insertion-line {
  animation: insertion-pulse 0.6s ease-in-out infinite alternate;
}
@keyframes insertion-pulse {
  from { opacity: 0.55; transform: scaleY(1); }
  to   { opacity: 1;    transform: scaleY(1.4); }
}
@media (prefers-reduced-motion: reduce) {
  .insertion-line { animation: none; opacity: 1; }
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
