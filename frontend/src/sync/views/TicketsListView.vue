<script setup lang="ts">
/**
 * Tickets list — workspace-wide table fed by the sync engine.
 *
 * Shell only. The route component wires the subscription, derives
 * the card list from the sync pool, and composes the state
 * composables (view resolution, filters, summary, grouping, sort,
 * columns, density) with the child components (header, table,
 * calendar fork).
 *
 * Card pipeline:
 *   1. allCards   — full denormalised set from the sync pool
 *   2. afterView  — view's structural filter applied
 *   3. afterChip  — header pill filters applied
 *   4. sorted     — final ordering (or grouped buckets)
 *
 * The header reads from `afterView` (not `sorted`) so its
 * derived option lists (assignee chips, etc.) don't shrink as
 * filters are added — chips would self-erase otherwise. Summary
 * stats also read from afterView, so "12 open" describes the
 * queue itself, not the temporarily filtered slice.
 *
 * Keyboard:
 *   /  — open AddFilterMenu pre-selected to the Title facet
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useCreateTicketAction } from '@/composables/useCreateTicketAction'
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useAuthStore } from '@/stores/auth'
import { useSavedViewsStore } from '@/stores/savedViews'
import { buildPredicate } from './filter'
import { toCardData } from './cardData'
import { ALL_ACTIVE_VIEW, MY_OPEN_VIEW } from './builtinViews'
import type { CardData, Priority } from './types'
import {
  calendarOverlaysService,
  type CalendarOverlayEntry,
} from '@/services/calendarOverlaysService'
import CalendarBoard, { type CalendarOverlay } from './CalendarBoard.vue'
import TicketsHeader from '@/components/views/TicketsHeader.vue'
import TicketsTable from '@/components/views/TicketsTable.vue'
import TicketsCardList from '@/components/views/TicketsCardList.vue'
import TicketPreviewPane from '@/components/views/TicketPreviewPane.vue'
import SavedViewEditorModal from '@/components/views/SavedViewEditorModal.vue'
import type { SavedView } from '@/services/savedViewsService'
import { useTicketsViewResolution } from '@/composables/useTicketsViewResolution'
import { useTicketsSort } from '@/composables/useTicketsSort'
import { useTicketsColumns } from '@/composables/useTicketsColumns'
import { useTicketsDensity } from '@/composables/useTicketsDensity'
import {
  useTicketsFilters,
  type FilterFacet,
  type SlaFilter,
} from '@/composables/useTicketsFilters'
import { useTicketsGrouping } from '@/composables/useTicketsGrouping'
import { useTicketsSummary } from '@/composables/useTicketsSummary'
import { useSplitView } from '@/composables/useSplitView'
import { useDragGesture } from '@/composables/useDragGesture'
import { useBulkSelection } from '@/composables/useBulkSelection'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import TicketsBulkBar from '@/components/views/TicketsBulkBar.vue'
import { onScopeDispose } from 'vue'
import { useTicketSelection } from '@/composables/useTicketSelection'
import { useWorkspaceCapabilities } from '@/composables/useWorkspaceCapabilities'
import { FACET_ORDER } from '@/components/views/filterFacets'
import { TICKET_COLUMNS } from '@/sync/views/ticketColumns'

const router = useRouter()
const ticketsStore = useSyncTicketsStore()
const authStore = useAuthStore()
const savedViewsStore = useSavedViewsStore()

onMounted(async () => {
  await subscribe('workspace:1')
  await savedViewsStore.ensureLoaded(null)
})

const { activeView, tabItems, savedItems, selectViewById } = useTicketsViewResolution()
const { sortField, sortDir, toggleSort, applySort } = useTicketsSort(activeView)
const {
  visibleColumnIds,
  visibleColumns,
  layoutDirty,
  canSaveLayoutToView,
  layout,
  toggleColumn,
  resetColumns,
  saveLayoutToView,
  colStyle,
} = useTicketsColumns(activeView)
const { density, setDensity, rowClass, cellPadding } = useTicketsDensity()
const filters = useTicketsFilters()
const workflowStatesStore = useWorkflowStatesStore()
const grouping = useTicketsGrouping(() => activeView.value.id)
const splitView = useSplitView()
const capabilities = useWorkspaceCapabilities()

// Filter facet list, gated by workspace capabilities. Currently
// this just hides 'sla' when no policies exist; future flags
// (eg. 'cycle' if cycles are disabled per-workspace) join the
// same filter chain.
const facetOrder = computed(() =>
  FACET_ORDER.filter((f) => f !== 'sla' || capabilities.slaEnabled.value),
)

// Columns the DisplayMenu's Properties picker offers. Same gating
// principle as facetOrder. We don't filter the active visible set
// here; useTicketsColumns owns that. The picker simply doesn't
// list disabled-feature columns so the user can't toggle them on
// only to see "—" in every row.
const availableColumns = computed(() =>
  TICKET_COLUMNS.filter((c) => c.id !== 'sla' || capabilities.slaEnabled.value),
)

// ---------------------------------------------------------------
// Card pipeline.
// ---------------------------------------------------------------
const allTickets = ticketsStore.all()

const allCards = computed<CardData[]>(() => {
  const out: CardData[] = []
  for (const t of allTickets.value) {
    const card = toCardData(t)
    if (card) out.push(card)
  }
  return out
})

/**
 * Smart fall-through: when the resolved view is `MY_OPEN` and
 * produces zero rows, but `ALL_ACTIVE` does have rows, render
 * ALL_ACTIVE's filter instead. The active-view picker / URL
 * doesn't change — this is purely a render-time deflection so an
 * agent with no assignments yet never lands on a blank screen
 * for an established workspace. NN/G empty-state guidance:
 * "communicate system status; never let a primary nav surface
 * read as broken." The banner below tells the user what swap
 * happened so the fall-through is transparent rather than
 * surprising.
 *
 * Predicate construction is unified through `buildViewPredicate`
 * so the three places that need a predicate (the two fall-through
 * candidates + the actual render filter) all read from one
 * source. The two built-in predicates stay as separate computeds
 * because fallThroughActive needs both at the same time; the
 * render predicate reuses them when the active view matches.
 */
function buildViewPredicate(view: { filter: typeof MY_OPEN_VIEW.filter }) {
  return buildPredicate(view.filter, {
    currentUserUuid: authStore.user?.uuid ?? null,
  })
}

const myOpenPredicate = computed(() => buildViewPredicate(MY_OPEN_VIEW))
const allActivePredicate = computed(() => buildViewPredicate(ALL_ACTIVE_VIEW))

const fallThroughActive = computed(() => {
  if (activeView.value.id !== MY_OPEN_VIEW.id) return false
  // .some() short-circuits on the first match instead of
  // materialising a full filtered array just to read its length.
  if (allCards.value.some(myOpenPredicate.value)) return false
  return allCards.value.some(allActivePredicate.value)
})

const afterViewFilter = computed<CardData[]>(() => {
  // Reuse the cached built-in predicates when the active view
  // matches one of them; build fresh for saved views and other
  // built-ins. Saves a buildPredicate() per render of the most
  // common case (MY_OPEN as default landing).
  const view = fallThroughActive.value ? ALL_ACTIVE_VIEW : activeView.value
  const predicate =
    view.id === MY_OPEN_VIEW.id ? myOpenPredicate.value
    : view.id === ALL_ACTIVE_VIEW.id ? allActivePredicate.value
    : buildViewPredicate(view)
  return allCards.value.filter(predicate)
})

const afterChipFilter = computed<CardData[]>(() =>
  afterViewFilter.value.filter(filters.predicate.value),
)

const sortedCards = applySort(afterChipFilter)
const buckets = grouping.buckets(sortedCards)
const { segments } = useTicketsSummary(afterViewFilter)

const selection = useTicketSelection(sortedCards)

// ---------------------------------------------------------------
// Bulk selection (multi-row checkbox model). Coexists with the
// single-row split-view selection above — `selection` drives the
// preview pane, `bulkSelection` drives the floating action bar.
//
// `cacheKey` is a stable string fingerprint of "what does the
// current view show". When it changes (user switches saved view,
// applies a new filter chip, changes sort), useBulkSelection
// clears its picks because the previous selection no longer
// matches the new query — keeping stale ids around would let a
// bulk action target rows the user can no longer see.
// ---------------------------------------------------------------
const bulkCacheKey = computed<string>(() =>
  JSON.stringify({
    view: activeView.value.id,
    sortField: sortField.value,
    sortDir: sortDir.value,
    title: filters.title.value,
    statuses: Array.from(filters.status.value),
    priorities: Array.from(filters.priority.value),
    assignees: Array.from(filters.assignee.value),
    sla: Array.from(filters.sla.value),
    cycles: Array.from(filters.cycle.value),
  }),
)
const bulkSelection = useBulkSelection<CardData>({
  items: sortedCards,
  cacheKey: bulkCacheKey,
  totalCount: computed(() => sortedCards.value.length),
})

// Esc clears the selection. Doesn't conflict with split-view's
// keyboard shortcuts because they're scoped to ArrowUp/Down/Enter.
function onKeydownClearBulk(e: KeyboardEvent): void {
  if (e.key !== 'Escape') return
  if (bulkSelection.selectedCount.value === 0) return
  const target = e.target as HTMLElement | null
  if (
    target &&
    (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)
  ) {
    return
  }
  bulkSelection.clear()
}
if (typeof window !== 'undefined') {
  window.addEventListener('keydown', onKeydownClearBulk)
  onScopeDispose(() => window.removeEventListener('keydown', onKeydownClearBulk))
}

// Bulk action dispatchers. Each delegates to the sync store's
// bulk variant; the store handles per-ticket optimistic updates
// + rollback. We close the bulk selection on success so the
// user gets the visual confirmation of "your action landed".
async function handleBulkSetStatus(stateId: number, ticketIds: number[]): Promise<void> {
  const target = workflowStatesStore.findById(stateId)
  if (!target) return
  await ticketsStore.bulkMoveToWorkflowState(ticketIds, target)
  bulkSelection.clear()
}
async function handleBulkSetPriority(priority: string, ticketIds: number[]): Promise<void> {
  await ticketsStore.bulkPatchKanbanFields(ticketIds, {
    priority: priority as CardData['priority'],
  })
  bulkSelection.clear()
}
async function handleBulkSetAssignee(uuid: string, ticketIds: number[]): Promise<void> {
  await ticketsStore.bulkPatchKanbanFields(ticketIds, { assignee_uuid: uuid })
  bulkSelection.clear()
}

// Viewport-aware split-view gate. Per the useSplitView doc
// comment, the composable doesn't enforce viewport-aware
// clamping — the consumer falls back to single-pane below the
// `md` breakpoint (768px) so phones don't try to render a
// 360px-min preview pane next to the table. The user-toggled
// `splitView.enabled` value persists; we just hide the layout
// when there isn't room for it.
const isWideViewport = ref<boolean>(true)
{
  const mql = typeof window !== 'undefined' ? window.matchMedia('(min-width: 768px)') : null
  if (mql) {
    isWideViewport.value = mql.matches
    const onChange = (e: MediaQueryListEvent) => {
      isWideViewport.value = e.matches
    }
    mql.addEventListener('change', onChange)
    onScopeDispose(() => mql.removeEventListener('change', onChange))
  }
}
const splitViewActive = computed(() => splitView.enabled.value && isWideViewport.value)

// Auto-select the first row whenever split-view turns on so the
// preview pane has something to render. Two distinct triggers
// share one effect:
//
//   1. splitViewActive flips on → try to select the first row.
//      No-op if sortedCards is still empty (initial mount, sync
//      subscription not yet resolved); the cards-arrive trigger
//      below picks it up when the data lands.
//   2. sortedCards changes → reconcile (drop selection if the
//      selected row vanished from the filter/sort) and, if
//      split-view is on, ensure first row is selected. Catches
//      the initial-mount race where splitViewActive fired with
//      empty cards and selectFirstIfNone was a no-op.
//
// selectFirstIfNone is idempotent (only acts when no selection),
// so calling it from both triggers is safe.
watch(
  splitViewActive,
  (on) => {
    if (on) selection.selectFirstIfNone()
    else selection.clearSelected()
  },
  { immediate: true },
)
watch(sortedCards, () => {
  selection.reconcile()
  if (splitViewActive.value) selection.selectFirstIfNone()
})

const isInitiallyLoading = computed(
  () => allTickets.value.length === 0 && allCards.value.length === 0,
)

function open(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

useCreateTicketAction()

// ---------------------------------------------------------------
// Filter mutation adapters. The header speaks in (facet, value)
// pairs; we route those into the typed Sets the composable
// owns, reassigning the ref so dependent computeds re-evaluate.
// ---------------------------------------------------------------
function toggleFilter(facet: FilterFacet, raw: string): void {
  if (facet === 'title') return // title uses set-text instead
  if (facet === 'status') {
    const next = new Set(filters.status.value)
    const id = Number(raw)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    filters.status.value = next
    return
  }
  if (facet === 'priority') {
    const next = new Set(filters.priority.value)
    const v = raw as Priority
    if (next.has(v)) next.delete(v)
    else next.add(v)
    filters.priority.value = next
    return
  }
  if (facet === 'assignee') {
    const next = new Set(filters.assignee.value)
    if (next.has(raw)) next.delete(raw)
    else next.add(raw)
    filters.assignee.value = next
    return
  }
  if (facet === 'sla') {
    const next = new Set(filters.sla.value)
    const v = raw as SlaFilter
    if (next.has(v)) next.delete(v)
    else next.add(v)
    filters.sla.value = next
    return
  }
  if (facet === 'cycle') {
    const next = new Set(filters.cycle.value)
    const id = Number(raw)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    filters.cycle.value = next
    return
  }
}

function setFilterText(facet: FilterFacet, value: string): void {
  if (facet === 'title') filters.title.value = value
}

function clearFilter(facet: FilterFacet): void {
  filters.clearFacet(facet)
}

// ---------------------------------------------------------------
// `/` opens the AddFilterMenu pre-selected to the Title facet,
// keeping search inside the unified filter model. Skip when the
// user is already typing in an input — pressing slash inside a
// new ticket title shouldn't yank focus.
// ---------------------------------------------------------------
const headerRef = ref<InstanceType<typeof TicketsHeader> | null>(null)

function onKey(e: KeyboardEvent): void {
  const t = e.target as HTMLElement | null
  const tag = t?.tagName
  const inField = tag === 'INPUT' || tag === 'TEXTAREA' || t?.isContentEditable

  if (e.key === '/' && !inField) {
    e.preventDefault()
    headerRef.value?.openAddFilter('title')
    return
  }

  // Split-view keyboard nav. Arrow keys + Enter operate on the
  // selection when the preview pane is open and the user isn't
  // typing into an input.
  // Use the viewport-aware splitViewActive — keyboard nav for
  // selected-row arrow keys only makes sense when the preview pane
  // is actually visible.
  if (!splitViewActive.value || inField) return
  if (e.key === 'ArrowDown' || e.key === 'j') {
    e.preventDefault()
    selection.move(1)
    return
  }
  if (e.key === 'ArrowUp' || e.key === 'k') {
    e.preventDefault()
    selection.move(-1)
    return
  }
  if (e.key === 'Enter') {
    if (selection.selectedId.value != null) {
      e.preventDefault()
      open(selection.selectedId.value)
    }
    return
  }
  if (e.key === 'Escape') {
    selection.clearSelected()
    return
  }
}

onMounted(() => {
  document.addEventListener('keydown', onKey)
})
onUnmounted(() => {
  document.removeEventListener('keydown', onKey)
})

// ---------------------------------------------------------------
// Save-as-view + rename + archive flows. Kept here (not in the
// resolver composable) because they reach into router + window
// prompt — keeping them in the route component makes the
// composable testable without DOM globals.
// ---------------------------------------------------------------
const isSaving = ref(false)
const savedViewsRef = savedViewsStore.viewsForProject(null)

async function saveAsView(): Promise<void> {
  const userUuid = authStore.user?.uuid
  if (!userUuid) return
  const fallbackName = activeView.value.source === 'saved'
    ? `${activeView.value.name} copy`
    : activeView.value.name
  const name = window.prompt('Name this view', fallbackName)
  if (!name) return
  isSaving.value = true
  try {
    const created = await savedViewsStore.create({
      scope: 'private',
      scope_id: userUuid,
      name: name.trim(),
      shape: activeView.value.shape,
      filter: activeView.value.filter,
    })
    if (created) {
      router.push({ query: { view: created.uuid } })
    }
  } finally {
    isSaving.value = false
  }
}

// ---------------------------------------------------------------
// Saved-view editor modal. Replaces the earlier window.prompt /
// window.confirm pair with a single focused surface where rename
// and delete live together. Open state is the editing view itself
// (null = closed) so the modal naturally re-renders if the user
// opens a different view's editor without closing first.
// ---------------------------------------------------------------
const editingView = ref<SavedView | null>(null)

function openEditor(uuid: string): void {
  const view = savedViewsRef.value.find((v) => v.uuid === uuid)
  editingView.value = view ?? null
}

async function handleRename(uuid: string, name: string): Promise<boolean> {
  const result = await savedViewsStore.update(uuid, { name })
  return result !== null
}

async function handleDelete(uuid: string): Promise<boolean> {
  const ok = await savedViewsStore.deleteView(uuid)
  if (ok && activeView.value.uuid === uuid) {
    router.push({ query: { view: MY_OPEN_VIEW.id } })
  }
  return ok
}

// ---------------------------------------------------------------
// Calendar overlays — unchanged; CalendarBoard is one of the two
// render targets the active view selects between.
// ---------------------------------------------------------------
const overlayCache = ref<Map<string, CalendarOverlayEntry[]>>(new Map())
const calendarOverlays = ref<CalendarOverlay[]>([])

function entryToOverlay(e: CalendarOverlayEntry): CalendarOverlay {
  return {
    id: `${e.kind}:${e.device_id}:${e.date}`,
    date: e.date,
    kind: e.kind,
    label: e.label,
    href: `/devices/${e.device_id}`,
  }
}

async function loadOverlays(start: string, end: string): Promise<void> {
  const key = `${start}..${end}`
  const cached = overlayCache.value.get(key)
  if (cached) {
    calendarOverlays.value = cached.map(entryToOverlay)
    return
  }
  try {
    const rows = await calendarOverlaysService.list(start, end)
    overlayCache.value.set(key, rows)
    calendarOverlays.value = rows.map(entryToOverlay)
  } catch {
    calendarOverlays.value = []
  }
}

function onCalendarVisibleRange(range: { start: string; end: string }): void {
  void loadOverlays(range.start, range.end)
}

const slaOverlays = computed<CalendarOverlay[]>(() => {
  const out: CalendarOverlay[] = []
  for (const card of afterViewFilter.value) {
    const sla = card.sla
    if (!sla || sla.paused) continue
    const target = new Date(sla.target_at)
    if (Number.isNaN(target.getTime())) continue
    const y = target.getFullYear()
    const m = String(target.getMonth() + 1).padStart(2, '0')
    const d = String(target.getDate()).padStart(2, '0')
    out.push({
      id: `sla:${card.id}`,
      date: `${y}-${m}-${d}`,
      kind: 'sla_breach',
      label: sla.breached
        ? `SLA breached: ${card.title}`
        : `SLA target: ${card.title}`,
      href: `/tickets/${card.id}`,
    })
  }
  return out
})

const mergedCalendarOverlays = computed<CalendarOverlay[]>(() => [
  ...calendarOverlays.value,
  ...slaOverlays.value,
])

// ---------------------------------------------------------------
// Split-view resize. Uses the shared `useDragGesture`
// composable for the gesture mechanics (pointer capture, rAF
// coalescing, will-change, cleanup). Strategy is LIVE: the
// preview pane is one element, so writing the new pixel width
// each rAF tick is cheap. We skip `setPaneWidth` during the
// drag (it persists to localStorage on every call) and only
// commit through it on pointerup.
//
// `direction: -1` because the divider sits on the LEFT edge of
// a right-anchored pane: dragging left should grow the pane.
// ---------------------------------------------------------------
const paneDrag = useDragGesture()
function startPaneResize(event: PointerEvent): void {
  paneDrag.begin(event, {
    axis: 'x',
    direction: -1,
    startValue: splitView.paneWidth.value,
    clamp: (raw) =>
      Math.max(splitView.minPaneWidth, Math.min(splitView.maxPaneWidth, raw)),
    onUpdate: (value) => {
      // Direct ref mutation: skips the localStorage write that
      // setPaneWidth would do every frame.
      splitView.paneWidth.value = Math.round(value)
    },
    onCommit: (value) => {
      // setPaneWidth re-clamps + persists. Final source of truth.
      splitView.setPaneWidth(value)
    },
  })
}
</script>

<template>
  <div class="flex flex-col h-full bg-app">
    <TicketsHeader
      ref="headerRef"
      :tab-items="tabItems"
      :saved-items="savedItems"
      :active-view-id="activeView.id"
      :source-cards="afterViewFilter"
      :density="density"
      :group-by="grouping.groupBy.value"
      :visible-columns="visibleColumnIds"
      :can-save-layout-to-view="canSaveLayoutToView"
      :layout-dirty="layoutDirty"
      :summary-segments="segments"
      :active-facets="filters.activeFacets.value"
      :filter-title="filters.title.value"
      :filter-status="filters.status.value"
      :filter-priority="filters.priority.value"
      :filter-assignee="filters.assignee.value"
      :filter-sla="filters.sla.value"
      :filter-cycle="filters.cycle.value"
      :split-view-enabled="splitView.enabled.value"
      :facet-order="facetOrder"
      :available-columns="availableColumns"
      @select-view="selectViewById"
      @edit-view="openEditor"
      @save-as-view="saveAsView"
      @set-density="setDensity"
      @set-group-by="grouping.setGroupBy"
      @toggle-column="toggleColumn"
      @reset-layout="resetColumns"
      @save-layout-to-view="saveLayoutToView"
      @toggle-filter="toggleFilter"
      @clear-filter="clearFilter"
      @set-filter-text="setFilterText"
      @toggle-split-view="splitView.toggle"
    />

    <!-- Fall-through banner: My Open was empty, we transparently
         swapped to All Active so the surface isn't blank. Inline,
         dismissable-by-navigation (clicking another view clears
         the swap). -->
    <div
      v-if="fallThroughActive"
      class="px-4 py-2 text-xs text-secondary bg-surface-alt border-b border-default flex items-center gap-2"
    >
      <span class="font-medium text-primary">{{ $t('ticket-list-empty-no-assigned-message') }}</span>
      <span>{{ $t('ticket-list-empty-showing-all-active') }}</span>
    </div>

    <div
      v-if="isInitiallyLoading"
      class="flex-1 flex items-center justify-center text-tertiary text-sm"
    >
      Loading tickets…
    </div>

    <CalendarBoard
      v-else-if="activeView.shape.type === 'calendar'"
      class="flex-1 min-h-0"
      :cards="afterChipFilter"
      :date-field="activeView.shape.date_field"
      :overlays="mergedCalendarOverlays"
      :on-card-click="open"
      @visible-range="onCalendarVisibleRange"
    />

    <!-- Contextual empty states. Affirmation copy ("clear" / "caught
         up") on per-view empties so the surface reads as success not
         broken; "no tickets match" stays as the fallback for
         user-applied filters that produce no rows. NN/G empty-state
         guidance §3: communicate system status, never let a primary
         nav surface look broken. -->
    <div
      v-else-if="sortedCards.length === 0"
      class="flex-1 flex flex-col items-center justify-center text-tertiary text-sm gap-1"
    >
      <template v-if="filters.activeFacets.value.length > 0">
        <p class="font-medium text-primary">{{ $t('ticket-list-empty-no-match-title') }}</p>
        <p class="text-xs">{{ $t('ticket-list-empty-no-match-description') }}</p>
      </template>
      <template v-else-if="activeView.id === 'triage'">
        <p class="font-medium text-primary">{{ $t('ticket-list-empty-triage-clear-title') }}</p>
        <p class="text-xs">{{ $t('ticket-list-empty-triage-clear-description') }}</p>
      </template>
      <template v-else-if="activeView.id === MY_OPEN_VIEW.id">
        <p class="font-medium text-primary">{{ $t('ticket-list-empty-all-caught-up-title') }}</p>
        <p class="text-xs">{{ $t('ticket-list-empty-all-caught-up-description') }}</p>
      </template>
      <template v-else-if="activeView.id === ALL_ACTIVE_VIEW.id">
        <p class="font-medium text-primary">{{ $t('ticket-list-empty-no-active-title') }}</p>
        <p class="text-xs">{{ $t('ticket-list-empty-no-active-description') }}</p>
      </template>
      <template v-else>
        <p class="font-medium text-primary">{{ $t('ticket-list-empty-no-in-view-title') }}</p>
        <p class="text-xs">{{ $t('ticket-list-empty-no-in-view-description') }}</p>
      </template>
    </div>

    <!-- Mobile: card list. Stacks each ticket's attributes
         vertically so a phone shows ~5-6 facts per row without
         horizontal scroll. Same `sortedCards` source so sort /
         filter state matches the desktop table exactly. -->
    <TicketsCardList
      v-else-if="!isWideViewport"
      :cards="sortedCards"
      class="flex-1 min-h-0"
      @open="open"
    />

    <!-- Desktop: split-view layout. Table on the left, divider,
         preview on the right. The table becomes the flex-1
         element so it gobbles available space; the preview is a
         fixed-width column the user can resize. When split-view
         is off the table renders standalone (single-pane). -->
    <div
      v-else
      class="flex-1 flex min-h-0 min-w-0"
    >
      <TicketsTable
        :cards="sortedCards"
        :visible-columns="visibleColumns"
        :row-class="rowClass"
        :cell-padding="cellPadding"
        :sort-field="sortField"
        :sort-dir="sortDir"
        :layout="layout"
        :col-style="colStyle"
        :buckets="buckets"
        :is-collapsed="grouping.isCollapsed"
        :selected-id="splitViewActive ? selection.selectedId.value : undefined"
        :bulk-selection="bulkSelection"
        class="flex-1 min-w-0"
        @open="open"
        @select="selection.setSelected"
        @toggle-sort="toggleSort"
        @toggle-bucket="grouping.toggleCollapsed"
      />

      <!-- Split-view divider + preview pane. Mounted only while
           split-view is enabled so the layout collapses cleanly
           when the user toggles off. The Transition wraps both
           the divider and the pane so they enter / leave as a
           single unit (a slide-in from the right edge with a
           subtle fade). -->
      <Transition name="split-pane">
        <div
          v-if="splitViewActive"
          class="flex shrink-0"
        >
          <div
            class="w-1 cursor-col-resize bg-subtle hover:bg-accent/40 active:bg-accent/60 transition-colors shrink-0 touch-none"
            :title="`Drag to resize preview (${splitView.paneWidth.value}px)`"
            @pointerdown="startPaneResize"
          />
          <div
            class="shrink-0"
            :style="{ width: `${splitView.paneWidth.value}px` }"
          >
            <TicketPreviewPane
              :card="selection.selectedCard.value"
              @open="open"
              @close="selection.clearSelected"
            />
          </div>
        </div>
      </Transition>
    </div>

    <SavedViewEditorModal
      :view="editingView"
      @close="editingView = null"
      @rename="handleRename"
      @delete="handleDelete"
    />

    <!-- Floating bulk-action bar. Renders only when at least one
         row is checked (the component handles its own visibility
         via the selection count). Sits at the bottom-center of
         the viewport over the table — fixed positioning so it
         floats above the split-pane preview when active. -->
    <TicketsBulkBar
      :selected-ids="bulkSelection.selectedIds.value"
      :total-count="sortedCards.length"
      @clear="bulkSelection.clear"
      @set-status="handleBulkSetStatus"
      @set-priority="handleBulkSetPriority"
      @set-assignee="handleBulkSetAssignee"
    />
  </div>
</template>

<style scoped>
/* Split-view enter / leave. The pane slides in from the right
   edge with a brief fade; on exit it reverses with a snappier
   ease-in curve (the "appear graceful, dismiss snappy" rhythm
   that makes a panel feel like an intentional surface rather
   than a window flicker). The table snaps to its new width
   beside the pane — animating both flex children would require
   measured-width JS animation which isn't worth the complexity. */
.split-pane-enter-active {
  transition:
    transform 220ms cubic-bezier(0.16, 1, 0.3, 1),
    opacity 160ms ease-out;
}
.split-pane-leave-active {
  transition:
    transform 160ms cubic-bezier(0.4, 0, 1, 1),
    opacity 120ms ease-in;
}
.split-pane-enter-from,
.split-pane-leave-to {
  opacity: 0;
  transform: translateX(24px);
}

@media (prefers-reduced-motion: reduce) {
  .split-pane-enter-active,
  .split-pane-leave-active {
    transition: opacity 100ms linear;
  }
  .split-pane-enter-from,
  .split-pane-leave-to {
    transform: none;
  }
}
</style>
