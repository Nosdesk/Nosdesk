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
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useAuthStore } from '@/stores/auth'
import { useSavedViewsStore } from '@/stores/savedViews'
import { buildPredicate } from './filter'
import { toCardData } from './cardData'
import { MY_QUEUE_VIEW } from './builtinViews'
import type { CardData, Priority } from './types'
import {
  calendarOverlaysService,
  type CalendarOverlayEntry,
} from '@/services/calendarOverlaysService'
import CalendarBoard, { type CalendarOverlay } from './CalendarBoard.vue'
import TicketsHeader from '@/components/views/TicketsHeader.vue'
import TicketsTable from '@/components/views/TicketsTable.vue'
import TicketPreviewPane from '@/components/views/TicketPreviewPane.vue'
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

const { activeView, switcherItems, selectViewById } = useTicketsViewResolution()
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

const afterViewFilter = computed<CardData[]>(() => {
  const predicate = buildPredicate(activeView.value.filter, {
    currentUserUuid: authStore.user?.uuid ?? null,
  })
  return allCards.value.filter(predicate)
})

const afterChipFilter = computed<CardData[]>(() =>
  afterViewFilter.value.filter(filters.predicate.value),
)

const sortedCards = applySort(afterChipFilter)
const buckets = grouping.buckets(sortedCards)
const { segments } = useTicketsSummary(afterViewFilter)

const selection = useTicketSelection(sortedCards)

// Auto-select the first row whenever split-view turns on so the
// preview pane has something to render. Reconcile when the
// visible card set shifts (filter / sort change drops the
// selection if its row vanished).
watch(
  () => splitView.enabled.value,
  (on) => {
    if (on) selection.selectFirstIfNone()
    else selection.clearSelected()
  },
  { immediate: true },
)
watch(sortedCards, () => selection.reconcile())

const isInitiallyLoading = computed(
  () => allTickets.value.length === 0 && allCards.value.length === 0,
)

function open(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

function newTicket(): void {
  router.push('/tickets/new')
}

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
  if (!splitView.enabled.value || inField) return
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

async function renameById(uuid: string): Promise<void> {
  const view = savedViewsRef.value.find((v) => v.uuid === uuid)
  if (!view) return
  const next = window.prompt('Rename view', view.name)
  if (!next || next.trim() === view.name) return
  await savedViewsStore.update(uuid, { name: next.trim() })
}

async function archiveById(uuid: string): Promise<void> {
  const view = savedViewsRef.value.find((v) => v.uuid === uuid)
  if (!view) return
  if (!window.confirm(`Archive "${view.name}"?`)) return
  const ok = await savedViewsStore.archive(uuid)
  if (ok && activeView.value.uuid === uuid) {
    router.push({ query: { view: MY_QUEUE_VIEW.id } })
  }
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
// Split-view resize. Pointer-capture loop on the divider; drag
// left grows the right pane, drag right shrinks it. Persisted
// per-user via useSplitView.setPaneWidth which clamps to
// min/max so the table stays readable at any extreme.
// ---------------------------------------------------------------
function startPaneResize(e: PointerEvent): void {
  e.preventDefault()
  const startX = e.clientX
  const startWidth = splitView.paneWidth.value
  const target = e.currentTarget as HTMLElement
  target.setPointerCapture(e.pointerId)

  const onMove = (ev: PointerEvent) => {
    const delta = startX - ev.clientX  // dragging left => positive
    splitView.setPaneWidth(startWidth + delta)
  }
  const onUp = (ev: PointerEvent) => {
    target.releasePointerCapture?.(ev.pointerId)
    target.removeEventListener('pointermove', onMove)
    target.removeEventListener('pointerup', onUp)
    target.removeEventListener('pointercancel', onUp)
  }
  target.addEventListener('pointermove', onMove)
  target.addEventListener('pointerup', onUp)
  target.addEventListener('pointercancel', onUp)
}
</script>

<template>
  <div class="flex flex-col h-full bg-app">
    <TicketsHeader
      ref="headerRef"
      :switcher-items="switcherItems"
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
      @rename-view="renameById"
      @archive-view="archiveById"
      @save-as-view="saveAsView"
      @set-density="setDensity"
      @set-group-by="grouping.setGroupBy"
      @toggle-column="toggleColumn"
      @reset-layout="resetColumns"
      @save-layout-to-view="saveLayoutToView"
      @new-ticket="newTicket"
      @toggle-filter="toggleFilter"
      @clear-filter="clearFilter"
      @set-filter-text="setFilterText"
      @toggle-split-view="splitView.toggle"
    />

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

    <div
      v-else-if="sortedCards.length === 0"
      class="flex-1 flex flex-col items-center justify-center text-tertiary text-sm"
    >
      <p class="mb-1 font-medium">No tickets match.</p>
      <p class="text-xs">Pick a different view or remove some filters.</p>
    </div>

    <!-- Split-view layout: table on the left, divider, preview
         on the right. The table becomes the flex-1 element so
         it gobbles available space; the preview is a fixed-width
         column the user can resize. When split-view is off the
         table renders standalone (single-pane). -->
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
        :selected-id="splitView.enabled.value ? selection.selectedId.value : undefined"
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
          v-if="splitView.enabled.value"
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
