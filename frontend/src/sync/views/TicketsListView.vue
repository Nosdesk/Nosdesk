<script setup lang="ts">
/**
 * Tickets list — workspace-wide table fed by the sync engine.
 *
 * Architecture:
 * - Left: TicketViewsSidebar — persistent view list, no popover
 *   hop. Linear's Issues sidebar pattern.
 * - Right toolbar: active view name + record count + density
 *   toggle + columns picker.
 * - Right body: dense table driven by ticketColumns.ts. The
 *   user controls column visibility via the picker; choice
 *   persists to localStorage per view, and editable saved views
 *   can promote the local layout into shape.visible_card_fields.
 *
 * SSE / live update path:
 * - Sync pool mutations flow through `useSyncTicketsStore` and
 *   trigger the cards / filteredCards / sortedCards computeds.
 * - Each <tr> is wrapped in v-memo against `rowMemoKey(card)` so
 *   only rows whose visible fields actually changed re-render.
 *   A burst of SSE events touching N tickets re-renders N rows,
 *   not the entire visible list.
 *
 * Loader-coordinated startup:
 * - ticketsListLoader prefetches saved views in parallel with
 *   the first page of tickets and primes savedViewsStore. By
 *   the time this view mounts, the workspace default is in the
 *   store, so activeView resolves correctly on frame one.
 *   A one-shot router.replace() canonicalises ?view=<id> for
 *   reload stability.
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { subscribe } from '@/sync/lifecycle'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useAuthStore } from '@/stores/auth'
import { useSavedViewsStore } from '@/stores/savedViews'
import { paletteForColor } from '@/utils/workflowColors'
import {
  BUILTIN_VIEWS,
  findBuiltinView,
  MY_QUEUE_VIEW,
  type BuiltInView,
} from './builtinViews'
import { buildPredicate } from './filter'
import { toCardData } from './cardData'
import {
  TICKET_COLUMNS,
  DEFAULT_VISIBLE_COLUMNS,
  rowMemoKey,
  type ColumnId,
  type ListColumn,
} from './ticketColumns'
import type {
  CalendarViewShape,
  CardData,
  FilterState,
  ListViewShape,
  ViewShape,
} from './types'
import type { SavedView } from '@/services/savedViewsService'
import {
  calendarOverlaysService,
  type CalendarOverlayEntry,
} from '@/services/calendarOverlaysService'
import CalendarBoard, { type CalendarOverlay } from './CalendarBoard.vue'
import TicketViewsSidebar, {
  type TicketViewItem,
} from '@/components/views/TicketViewsSidebar.vue'
import ColumnPickerMenu from '@/components/views/ColumnPickerMenu.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import Icon from '@/components/common/Icon.vue'

const router = useRouter()
const route = useRoute()
const ticketsStore = useSyncTicketsStore()
const authStore = useAuthStore()
const savedViewsStore = useSavedViewsStore()

interface ResolvedView {
  id: string
  name: string
  description: string
  shape: ListViewShape | CalendarViewShape
  filter: FilterState
  source: 'builtin' | 'saved'
  uuid?: string
}

// ---------------------------------------------------------------
// Subscription & startup. The loader has already populated the
// saved-views cache by the time we get here; ensureLoaded is a
// belt-and-braces fallback for direct mounts that bypassed the
// loader (tests, deep links from external apps).
// ---------------------------------------------------------------
onMounted(async () => {
  await subscribe('workspace:1')
  await savedViewsStore.ensureLoaded(null)
})

const savedViewsRef = savedViewsStore.viewsForProject(null)

const listSavedViews = computed<SavedView[]>(() =>
  savedViewsRef.value.filter((v) => {
    const t = (v.shape as ViewShape | null)?.type
    return t === 'list' || t === 'calendar'
  }),
)

function fromBuiltin(view: BuiltInView): ResolvedView {
  return {
    id: view.id,
    name: view.name,
    description: view.description,
    shape: view.shape,
    filter: view.filter,
    source: 'builtin',
  }
}

function fromSaved(view: SavedView): ResolvedView {
  return {
    id: view.uuid,
    name: view.name,
    description: view.scope === 'private' ? 'Private view' : 'Workspace view',
    shape: view.shape as ListViewShape | CalendarViewShape,
    filter: view.filter,
    source: 'saved',
    uuid: view.uuid,
  }
}

const workspaceDefaultView = computed<SavedView | null>(
  () =>
    savedViewsRef.value.find(
      (v) => v.scope === 'workspace' && v.is_default && v.archived_at == null,
    ) ?? null,
)

const activeView = computed<ResolvedView>(() => {
  const requested = (route.query.view as string | undefined) ?? ''
  const builtin = findBuiltinView(requested)
  if (builtin) return fromBuiltin(builtin)
  const saved = listSavedViews.value.find((v) => v.uuid === requested)
  if (saved) return fromSaved(saved)
  if (workspaceDefaultView.value) return fromSaved(workspaceDefaultView.value)
  return fromBuiltin(MY_QUEUE_VIEW)
})

onMounted(() => {
  if (!route.query.view) {
    router.replace({
      path: route.path,
      query: { ...route.query, view: activeView.value.id },
    })
  }
})

function selectViewById(id: string): void {
  router.push({ path: route.path, query: { ...route.query, view: id } })
}

// ---------------------------------------------------------------
// Sort state, seeded by the active view's default.
// ---------------------------------------------------------------
const sortField = ref<string>(activeView.value.shape.sort[0]?.field ?? 'last_activity_at')
const sortDir = ref<'asc' | 'desc'>(activeView.value.shape.sort[0]?.dir ?? 'desc')

watch(activeView, (next) => {
  const seed = next.shape.sort[0]
  if (seed) {
    sortField.value = seed.field
    sortDir.value = seed.dir
  }
})

function toggleSort(field: string): void {
  if (sortField.value === field) {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
  } else {
    sortField.value = field
    sortDir.value = 'asc'
  }
}

// ---------------------------------------------------------------
// Cards (denormalised at the boundary), filtered + sorted.
// ---------------------------------------------------------------
const allTickets = ticketsStore.all()

const cards = computed<CardData[]>(() => {
  const out: CardData[] = []
  for (const t of allTickets.value) {
    const card = toCardData(t)
    if (card) out.push(card)
  }
  return out
})

const filteredCards = computed<CardData[]>(() => {
  const predicate = buildPredicate(activeView.value.filter, {
    currentUserUuid: authStore.user?.uuid ?? null,
  })
  return cards.value.filter(predicate)
})

const sortedCards = computed<CardData[]>(() => {
  const field = sortField.value
  const dir = sortDir.value === 'asc' ? 1 : -1
  return [...filteredCards.value].sort((a, b) => {
    const av = readSortField(a, field)
    const bv = readSortField(b, field)
    if (av === bv) return 0
    if (av == null) return 1 * dir
    if (bv == null) return -1 * dir
    return av < bv ? -1 * dir : 1 * dir
  })
})

function readSortField(card: CardData, field: string): string | number | null {
  const parts = field.split('.')
  let cursor: unknown = card
  for (const part of parts) {
    if (cursor == null || typeof cursor !== 'object') return null
    cursor = (cursor as Record<string, unknown>)[part]
  }
  if (cursor == null) return null
  if (typeof cursor === 'string' || typeof cursor === 'number') return cursor
  return JSON.stringify(cursor)
}

const isInitiallyLoading = computed(
  () => allTickets.value.length === 0 && cards.value.length === 0,
)

function open(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

function priorityForBadge(p: CardData['priority']): 'low' | 'medium' | 'high' | null {
  if (p === 'urgent') return 'high'
  if (p === 'low' || p === 'medium' || p === 'high') return p
  return null
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime()
  const seconds = Math.max(1, Math.round((Date.now() - then) / 1000))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.round(hours / 24)
  if (days < 30) return `${days}d`
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function shortDate(iso: string | null | undefined): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function recurrenceLabel(rule: string): string {
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

function slaToneClass(card: CardData): string {
  const sla = card.sla
  if (!sla) return 'text-tertiary'
  if (sla.breached) return 'text-rose-600 dark:text-rose-400'
  if (sla.pill_color === 'amber') return 'text-amber-600 dark:text-amber-400'
  if (sla.pill_color === 'green') return 'text-emerald-600 dark:text-emerald-400'
  return 'text-tertiary'
}

function slaLabel(card: CardData): string {
  const sla = card.sla
  if (!sla) return '—'
  if (sla.breached) return 'Breached'
  if (sla.paused) return 'Paused'
  const remaining = sla.seconds_remaining ?? 0
  if (remaining < 3600) return `${Math.ceil(remaining / 60)}m`
  if (remaining < 86_400) return `${Math.ceil(remaining / 3600)}h`
  return `${Math.ceil(remaining / 86_400)}d`
}

// ---------------------------------------------------------------
// Density toggle (Pencil & Paper enterprise table guide). User-
// controlled, persisted to localStorage so the choice survives
// reloads.
// ---------------------------------------------------------------
type Density = 'compact' | 'cosy' | 'comfortable'

function loadDensity(): Density {
  if (typeof localStorage === 'undefined') return 'cosy'
  const v = localStorage.getItem('tickets-list-density')
  return v === 'compact' || v === 'comfortable' ? v : 'cosy'
}

const density = ref<Density>(loadDensity())

function setDensity(value: Density): void {
  density.value = value
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('tickets-list-density', value)
  }
}

const rowClass = computed<string>(() => {
  if (density.value === 'compact') return 'h-8'
  if (density.value === 'comfortable') return 'h-12'
  return 'h-10'
})

const cellPadding = computed<string>(() =>
  density.value === 'compact' ? 'px-3 py-1' : density.value === 'comfortable' ? 'px-3 py-2.5' : 'px-3 py-1.5',
)

// ---------------------------------------------------------------
// Column visibility — persisted per view.
//
// Order of precedence:
//   1. localStorage override (user toggled here this session)
//   2. shape.visible_card_fields on the active view (saved view
//      authoritative layout)
//   3. DEFAULT_VISIBLE_COLUMNS factory default
//
// `layoutDirty` reports whether the local choice differs from the
// view's canonical layout — drives the "Save to view" button.
// ---------------------------------------------------------------
function storageKeyFor(viewId: string): string {
  return `tickets-columns:${viewId}`
}

function loadColumns(viewId: string): ColumnId[] | null {
  if (typeof localStorage === 'undefined') return null
  const raw = localStorage.getItem(storageKeyFor(viewId))
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return null
    const valid: ColumnId[] = []
    for (const item of parsed) {
      if (TICKET_COLUMNS.some((c) => c.id === item)) valid.push(item as ColumnId)
    }
    return valid.length ? valid : null
  } catch {
    return null
  }
}

function persistColumns(viewId: string, ids: ColumnId[]): void {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(storageKeyFor(viewId), JSON.stringify(ids))
}

function clearColumns(viewId: string): void {
  if (typeof localStorage === 'undefined') return
  localStorage.removeItem(storageKeyFor(viewId))
}

const localOverride = ref<ColumnId[] | null>(loadColumns(activeView.value.id))

watch(activeView, (next) => {
  localOverride.value = loadColumns(next.id)
})

/** Map a CardData field name onto a ColumnId. The spec stores
 * `visible_card_fields` as `(keyof CardData)[]` for forward
 * compatibility, but we render through ColumnIds — the picker
 * works in column-space, not field-space. */
function mapFieldToColumnId(field: string): ColumnId | null {
  if (field === 'workflow_state') return 'workflow_state'
  if (field === 'priority') return 'priority'
  if (field === 'assignee_uuid') return 'assignee'
  if (field === 'requester_uuid') return 'requester'
  if (field === 'category_id') return 'category'
  if (field === 'cycle_id') return 'cycle'
  if (field === 'due_date') return 'due_date'
  if (field === 'last_activity_at') return 'last_activity'
  if (field === 'created_at') return 'created_at'
  if (field === 'sla') return 'sla'
  if (field === 'kb_gap_signal') return 'kb_gap'
  if (field === 'affected_devices') return 'devices'
  if (field === 'recurrence_rule') return 'recurrence'
  if (field === 'id' || field === 'title') return field as ColumnId
  return null
}

function columnIdToField(id: ColumnId): string | null {
  switch (id) {
    case 'workflow_state': return 'workflow_state'
    case 'priority': return 'priority'
    case 'assignee': return 'assignee_uuid'
    case 'requester': return 'requester_uuid'
    case 'category': return 'category_id'
    case 'cycle': return 'cycle_id'
    case 'due_date': return 'due_date'
    case 'last_activity': return 'last_activity_at'
    case 'created_at': return 'created_at'
    case 'sla': return 'sla'
    case 'kb_gap': return 'kb_gap_signal'
    case 'devices': return 'affected_devices'
    case 'recurrence': return 'recurrence_rule'
    case 'id': return 'id'
    case 'title': return 'title'
    default: return null
  }
}

const viewCanonicalColumns = computed<ColumnId[]>(() => {
  const fields = (activeView.value.shape as ListViewShape).visible_card_fields
  if (!fields || fields.length === 0) return [...DEFAULT_VISIBLE_COLUMNS]
  const out: ColumnId[] = []
  if (!fields.some((f) => String(f) === 'title')) out.push('title')
  for (const f of fields) {
    const id = mapFieldToColumnId(String(f))
    if (id && !out.includes(id)) out.push(id)
  }
  return out.length ? out : [...DEFAULT_VISIBLE_COLUMNS]
})

const visibleColumnIds = computed<ColumnId[]>(
  () => localOverride.value ?? viewCanonicalColumns.value,
)

const visibleColumns = computed<ListColumn[]>(() =>
  visibleColumnIds.value
    .map((id) => TICKET_COLUMNS.find((c) => c.id === id))
    .filter((c): c is ListColumn => Boolean(c)),
)

const layoutDirty = computed<boolean>(() => {
  if (!localOverride.value) return false
  const canonical = viewCanonicalColumns.value
  if (localOverride.value.length !== canonical.length) return true
  return localOverride.value.some((id, i) => id !== canonical[i])
})

const canSaveLayoutToView = computed<boolean>(
  () => activeView.value.source === 'saved' && !!activeView.value.uuid,
)

function toggleColumn(id: ColumnId): void {
  if (id === 'title') return
  const current = visibleColumnIds.value
  const next = current.includes(id)
    ? current.filter((c) => c !== id)
    : [...current, id]
  if (!next.includes('title')) next.unshift('title')
  localOverride.value = next
  persistColumns(activeView.value.id, next)
}

function resetColumns(): void {
  localOverride.value = null
  clearColumns(activeView.value.id)
}

async function saveLayoutToView(): Promise<void> {
  if (!canSaveLayoutToView.value || !activeView.value.uuid) return
  const ids = visibleColumnIds.value
  const fields = ids.map(columnIdToField).filter((f): f is string => !!f)
  const shape = {
    ...(activeView.value.shape as ListViewShape),
    visible_card_fields: fields as (keyof CardData)[],
  }
  await savedViewsStore.update(activeView.value.uuid, { shape })
  resetColumns()
}

// ---------------------------------------------------------------
// Sidebar items.
// ---------------------------------------------------------------
const sidebarItems = computed<TicketViewItem[]>(() => {
  const items: TicketViewItem[] = []
  for (const v of BUILTIN_VIEWS) {
    items.push({ id: v.id, name: v.name, group: 'Built-in' })
  }
  const grouped = {
    workspace: 'Workspace',
    project: 'Project',
    private: 'Private',
  } as const
  for (const scope of ['workspace', 'project', 'private'] as const) {
    const subset = savedViewsRef.value.filter(
      (v) => v.scope === scope && v.archived_at == null,
    )
    for (const v of subset) {
      items.push({
        id: v.uuid,
        name: v.name,
        group: grouped[scope],
        editable: true,
      })
    }
  }
  return items
})

const isSaving = ref(false)

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
      router.push({ path: route.path, query: { ...route.query, view: created.uuid } })
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
    router.push({ path: route.path, query: { ...route.query, view: MY_QUEUE_VIEW.id } })
  }
}

// ---------------------------------------------------------------
// Calendar overlays — unchanged.
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
  for (const card of filteredCards.value) {
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
</script>

<template>
  <div class="flex h-full bg-app">
    <TicketViewsSidebar
      :items="sidebarItems"
      :active-id="activeView.id"
      @select="selectViewById"
      @rename="renameById"
      @archive="archiveById"
      @save="saveAsView"
    />

    <div class="flex-1 min-w-0 flex flex-col">
      <!-- Toolbar: active view + count + columns + density. The
           page title ("Tickets") lives in the global SiteHeader
           so the chrome stays out of the data area. -->
      <div
        class="flex items-center gap-3 px-4 h-10 border-b border-subtle bg-surface shrink-0"
      >
        <h2 class="text-sm font-semibold text-primary truncate">
          {{ activeView.name }}
        </h2>
        <span class="text-[11px] text-tertiary tabular-nums shrink-0">
          {{ sortedCards.length }}
          <span class="text-tertiary/70">of {{ cards.length }}</span>
        </span>
        <div class="flex-1" />
        <ColumnPickerMenu
          :visible="visibleColumnIds"
          :can-save-to-view="canSaveLayoutToView"
          :layout-dirty="layoutDirty"
          @toggle="toggleColumn"
          @reset="resetColumns"
          @save="saveLayoutToView"
        />
        <div
          class="inline-flex items-center rounded-md border border-subtle overflow-hidden"
          role="group"
          aria-label="Row density"
        >
          <button
            v-for="opt in [
              { v: 'compact', l: 'Compact' },
              { v: 'cosy', l: 'Cosy' },
              { v: 'comfortable', l: 'Comfortable' },
            ]"
            :key="opt.v"
            type="button"
            class="text-[11px] px-2 py-1 transition-colors"
            :class="density === opt.v
              ? 'bg-accent/10 text-accent'
              : 'text-secondary hover:bg-surface-hover'"
            :aria-pressed="density === opt.v"
            @click="setDensity(opt.v as Density)"
          >{{ opt.l }}</button>
        </div>
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
        :cards="filteredCards"
        :date-field="activeView.shape.date_field"
        :overlays="mergedCalendarOverlays"
        :on-card-click="open"
        @visible-range="onCalendarVisibleRange"
      />

      <div
        v-else-if="sortedCards.length === 0"
        class="flex-1 flex flex-col items-center justify-center text-tertiary text-sm"
      >
        <p class="mb-1 font-medium">No tickets match this view.</p>
        <p class="text-xs">Try a different view from the sidebar.</p>
      </div>

      <!-- Dense, column-driven table. v-memo keys each row to the
           subset of CardData fields that actually change its
           rendering, so SSE bursts touch one row each rather
           than the whole list. -->
      <div v-else class="flex-1 min-h-0 overflow-auto">
        <table class="w-full text-sm border-separate border-spacing-0">
          <thead>
            <tr class="sticky top-0 z-10 bg-surface">
              <th
                v-for="col in visibleColumns"
                :key="col.id"
                class="text-left text-[11px] font-medium text-tertiary uppercase tracking-wide border-b border-subtle bg-surface"
                :class="[col.width, cellPadding, col.align === 'center' && 'text-center', col.align === 'right' && 'text-right']"
              >
                <button
                  v-if="col.sortKey"
                  type="button"
                  class="inline-flex items-center gap-1 hover:text-primary transition-colors"
                  @click="toggleSort(col.sortKey!)"
                >
                  {{ col.label }}
                  <span v-if="sortField === col.sortKey" class="text-[10px] leading-none">
                    {{ sortDir === 'asc' ? '↑' : '↓' }}
                  </span>
                </button>
                <span v-else>{{ col.label }}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="card in sortedCards"
              :key="card.id"
              v-memo="[...rowMemoKey(card), visibleColumns.length, density]"
              class="hover:bg-surface-hover cursor-pointer transition-colors group"
              :class="rowClass"
              @click="open(card.id)"
            >
              <td
                v-for="col in visibleColumns"
                :key="col.id"
                class="border-b border-subtle/40 align-middle"
                :class="[
                  col.width,
                  cellPadding,
                  col.align === 'center' && 'text-center',
                  col.align === 'right' && 'text-right',
                ]"
              >
                <template v-if="col.id === 'id'">
                  <span class="text-tertiary font-mono text-[11px] tabular-nums">#{{ card.id }}</span>
                </template>

                <template v-else-if="col.id === 'title'">
                  <div class="flex items-center gap-2 min-w-0 text-primary">
                    <span
                      v-if="card.recurrence_rule"
                      class="text-tertiary text-xs leading-none shrink-0"
                      title="Recurring ticket"
                    >↻</span>
                    <span class="truncate" :title="card.title">{{ card.title }}</span>
                    <span
                      v-if="card.sla?.breached"
                      class="text-[10px] font-semibold uppercase tracking-wide text-rose-600 dark:text-rose-400 shrink-0"
                      title="SLA breached"
                    >SLA</span>
                  </div>
                </template>

                <template v-else-if="col.id === 'workflow_state'">
                  <span class="inline-flex items-center gap-1.5 text-xs text-secondary">
                    <span
                      class="inline-block w-2 h-2 rounded-full bg-current shrink-0"
                      :class="paletteForColor(card.workflow_state.color).solid"
                      aria-hidden="true"
                    />
                    <span class="truncate">{{ card.workflow_state.name }}</span>
                  </span>
                </template>

                <template v-else-if="col.id === 'priority'">
                  <PriorityIndicator
                    v-if="priorityForBadge(card.priority)"
                    :priority="priorityForBadge(card.priority)!"
                    size="xs"
                  />
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>

                <template v-else-if="col.id === 'assignee'">
                  <div v-if="card.assignee_uuid" class="flex items-center gap-2 text-xs">
                    <UserAvatar
                      :name="card.assignee_uuid"
                      :avatar="null"
                      size="xxs"
                      :showName="false"
                      :clickable="false"
                    />
                    <span class="text-secondary truncate font-mono text-[11px]">
                      {{ card.assignee_uuid.slice(0, 8) }}
                    </span>
                  </div>
                  <span v-else class="text-xs text-tertiary italic">—</span>
                </template>

                <template v-else-if="col.id === 'requester'">
                  <div v-if="card.requester_uuid" class="flex items-center gap-2 text-xs">
                    <UserAvatar
                      :name="card.requester_uuid"
                      :avatar="null"
                      size="xxs"
                      :showName="false"
                      :clickable="false"
                    />
                    <span class="text-secondary truncate font-mono text-[11px]">
                      {{ card.requester_uuid.slice(0, 8) }}
                    </span>
                  </div>
                  <span v-else class="text-xs text-tertiary italic">—</span>
                </template>

                <template v-else-if="col.id === 'category'">
                  <span
                    v-if="card.category_id != null"
                    class="text-[11px] text-secondary bg-surface-hover rounded px-1.5 py-0.5"
                  >#{{ card.category_id }}</span>
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>

                <template v-else-if="col.id === 'cycle'">
                  <span
                    v-if="card.cycle_id != null"
                    class="text-[11px] text-accent bg-accent/10 rounded px-1.5 py-0.5"
                    title="Belongs to a cycle"
                  >cycle #{{ card.cycle_id }}</span>
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>

                <template v-else-if="col.id === 'due_date'">
                  <span
                    class="text-[11px] tabular-nums"
                    :class="card.due_date ? 'text-secondary' : 'text-tertiary'"
                    :title="card.due_date ? new Date(card.due_date).toLocaleString() : 'No due date'"
                  >{{ shortDate(card.due_date) }}</span>
                </template>

                <template v-else-if="col.id === 'last_activity'">
                  <span
                    class="text-[11px] text-tertiary tabular-nums"
                    :title="new Date(card.last_activity_at).toLocaleString()"
                  >{{ relativeTime(card.last_activity_at) }}</span>
                </template>

                <template v-else-if="col.id === 'created_at'">
                  <span
                    class="text-[11px] text-tertiary tabular-nums"
                    :title="new Date(card.created_at).toLocaleString()"
                  >{{ relativeTime(card.created_at) }}</span>
                </template>

                <template v-else-if="col.id === 'sla'">
                  <span
                    v-if="card.sla"
                    class="inline-flex items-center gap-1 text-[11px] tabular-nums"
                    :class="slaToneClass(card)"
                    :title="card.sla.breached ? 'Breached' : (card.sla.paused ? 'Paused' : 'On track')"
                  >
                    <Icon name="clock" class="w-3 h-3" />
                    {{ slaLabel(card) }}
                  </span>
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>

                <template v-else-if="col.id === 'kb_gap'">
                  <span
                    v-if="card.kb_gap_signal && card.kb_gap_signal !== 'none'"
                    class="text-[10px] font-semibold uppercase tracking-wide rounded px-1.5 py-0.5"
                    :class="card.kb_gap_signal === 'strong'
                      ? 'bg-amber-500/20 text-amber-700 dark:text-amber-300'
                      : 'bg-surface-hover text-secondary'"
                    :title="`${card.kb_gap_signal} knowledge gap signal`"
                  >KB</span>
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>

                <template v-else-if="col.id === 'devices'">
                  <span
                    v-if="card.affected_devices && card.affected_devices.count > 0"
                    class="text-[11px] text-secondary tabular-nums inline-flex items-center gap-1"
                    :title="card.affected_devices.first?.name ?? `${card.affected_devices.count} device(s)`"
                  >
                    <Icon name="device" class="w-3 h-3" />
                    {{ card.affected_devices.count }}
                  </span>
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>

                <template v-else-if="col.id === 'recurrence'">
                  <span
                    v-if="card.recurrence_rule"
                    class="text-[10px] font-medium rounded px-1.5 py-0.5 bg-violet-500/15 text-violet-700 dark:text-violet-300"
                    :title="card.recurrence_rule"
                  >{{ recurrenceLabel(card.recurrence_rule) }}</span>
                  <span v-else class="text-xs text-tertiary">—</span>
                </template>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>
