<script setup lang="ts">
/**
 * Tickets list — sync-engine version. Renders the workspace's
 * tickets as a table, filtered + sorted client-side via the
 * FilterState evaluator.
 *
 * Phase 5 scope:
 * - Built-in saved views (Triage, My Queue) selectable from a
 *   pill row at the top.
 * - Sort by any sortable column.
 * - Click row to open ticket detail.
 *
 * Deferred to later commits:
 * - Saved view CRUD (DB table + URL round-trip).
 * - Bulk select on rows.
 * - Inline edit of priority / assignee.
 * - Virtualized scrolling for >500 rows.
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
  TRIAGE_VIEW,
  MY_QUEUE_VIEW,
  type BuiltInView,
} from './builtinViews'
import { buildPredicate } from './filter'
import { toCardData } from './cardData'
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
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'

const router = useRouter()
const route = useRoute()
const ticketsStore = useSyncTicketsStore()
const authStore = useAuthStore()
const savedViewsStore = useSavedViewsStore()

// Common shape both built-in views and DB-backed SavedView rows
// satisfy. Lets `activeView` switch on either source without the
// renderer caring which one it got.
interface ResolvedView {
  id: string
  name: string
  description: string
  /** List or calendar today; gantt / scrum land when those
   * renderers ship. The renderer branches on `shape.type`. */
  shape: ListViewShape | CalendarViewShape
  filter: FilterState
  source: 'builtin' | 'saved'
  uuid?: string
}

// ---------------------------------------------------------------
// Subscription — list view is workspace-scoped, so we subscribe
// to workspace:1 on mount. The lifecycle layer is idempotent
// against repeated subscribes (re-entry to /tickets is a no-op).
// ---------------------------------------------------------------
onMounted(async () => {
  await subscribe('workspace:1')
  // Workspace-scoped tickets list. ensureLoaded with projectId=null
  // pulls workspace + the caller's private views in one round trip.
  await savedViewsStore.ensureLoaded(null)
})

// ---------------------------------------------------------------
// Saved views — list shapes only. Kanban-shaped saved views still
// exist in the same table; they belong on the kanban routes, so
// the list view filters them out.
// ---------------------------------------------------------------
const savedViewsRef = savedViewsStore.viewsForProject(null)

const listSavedViews = computed<SavedView[]>(() => {
  return savedViewsRef.value.filter((v) => {
    const t = (v.shape as ViewShape | null)?.type
    return t === 'list' || t === 'calendar'
  })
})

function toResolved(view: SavedView): ResolvedView {
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

const builtinResolved = computed<ResolvedView[]>(() =>
  BUILTIN_VIEWS.map(fromBuiltin),
)

const savedResolved = computed<ResolvedView[]>(() =>
  listSavedViews.value.map(toResolved),
)

// ---------------------------------------------------------------
// Active view — `?view=<id-or-uuid>` resolves against built-ins
// first, then DB views. URL stays bookmark-able for both.
// ---------------------------------------------------------------
/** Workspace-scoped default saved view, if one exists. The seed
 * migration installs Triage as the workspace default so a fresh
 * page load lands there instead of MY_QUEUE_VIEW. Falls back to
 * the built-in only when no DB-backed default has been set up. */
const workspaceDefaultView = computed<SavedView | null>(() => {
  return savedViewsRef.value.find(
    (v) => v.scope === 'workspace' && v.is_default && v.archived_at == null,
  ) ?? null
})

const activeView = computed<ResolvedView>(() => {
  const requested = (route.query.view as string | undefined) ?? ''
  const builtin = findBuiltinView(requested)
  if (builtin) return fromBuiltin(builtin)
  const saved = listSavedViews.value.find((v) => v.uuid === requested)
  if (saved) return toResolved(saved)
  if (workspaceDefaultView.value) return toResolved(workspaceDefaultView.value)
  return fromBuiltin(MY_QUEUE_VIEW)
})

function selectView(view: ResolvedView): void {
  router.push({ path: route.path, query: { ...route.query, view: view.id } })
}

// ---------------------------------------------------------------
// Sort state — initialised from the active view's `shape.sort`
// but mutable per session so the user can re-sort without
// changing the saved view.
// ---------------------------------------------------------------
const sortField = ref<string>(activeView.value.shape.sort[0]?.field ?? 'last_activity_at')
const sortDir = ref<'asc' | 'desc'>(activeView.value.shape.sort[0]?.dir ?? 'desc')

watch(activeView, (next) => {
  // Reset sort to the new view's default whenever the user picks a
  // different built-in. Matches the architecture's "saved view is
  // a snapshot of shape + filter" intent.
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
// Cards — sourced from the tickets store, mapped to CardData,
// filtered through the active view's predicate, sorted.
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
  // Fallback: coerce nested objects to a JSON string so they sort
  // deterministically rather than throwing.
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
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.round(hours / 24)
  if (days < 30) return `${days}d ago`
  return new Date(iso).toLocaleDateString()
}

const COLUMNS = [
  { field: 'id', label: '#', sortable: true, width: 'w-16' },
  { field: 'title', label: 'Title', sortable: true, width: '' },
  { field: 'workflow_state.name', label: 'Status', sortable: true, width: 'w-36' },
  { field: 'priority', label: 'Priority', sortable: true, width: 'w-28' },
  { field: 'assignee_uuid', label: 'Assignee', sortable: false, width: 'w-40' },
  { field: 'last_activity_at', label: 'Updated', sortable: true, width: 'w-32' },
] as const

// Touch the imports so a future refactor that drops them gets a
// type error, not a silent no-op.
void TRIAGE_VIEW

// ---------------------------------------------------------------
// Save the active view's shape + filter as a private SavedView.
// `window.prompt` is the deliberately-cheap modal here. A fancier
// dialog (with shape preview, scope picker, default toggle) lands
// when the views surface gets its own UI in a later phase.
// ---------------------------------------------------------------
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

const canEditActiveView = computed<boolean>(() => {
  if (activeView.value.source !== 'saved' || !activeView.value.uuid) return false
  const row = savedViewsRef.value.find((v) => v.uuid === activeView.value.uuid)
  if (!row) return false
  if (row.scope === 'private') {
    return row.scope_id === authStore.user?.uuid
  }
  // Workspace edits gate at the server (admin only). The pill is
  // shown to everyone; the rename action surfaces the API error
  // through the store's `lastError` if forbidden.
  return row.scope === 'workspace'
})

async function renameActiveView(): Promise<void> {
  if (!canEditActiveView.value || !activeView.value.uuid) return
  const next = window.prompt('Rename view', activeView.value.name)
  if (!next || next.trim() === activeView.value.name) return
  await savedViewsStore.update(activeView.value.uuid, { name: next.trim() })
}

async function archiveActiveView(): Promise<void> {
  if (!canEditActiveView.value || !activeView.value.uuid) return
  if (!window.confirm(`Archive "${activeView.value.name}"?`)) return
  const ok = await savedViewsStore.archive(activeView.value.uuid)
  if (ok) {
    router.push({ path: route.path, query: { ...route.query, view: MY_QUEUE_VIEW.id } })
  }
}

// ---------------------------------------------------------------
// Calendar overlays. Fetched lazily when CalendarBoard emits its
// visible-range; cached by `start..end` so paging back to a month
// you already viewed doesn't refetch. The cache lives only for
// the session, which is the right grain for warranty data that
// changes on the timescale of weeks.
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
    // Soft-fail: the calendar still renders without overlays.
    calendarOverlays.value = []
  }
}

function onCalendarVisibleRange(range: { start: string; end: string }): void {
  void loadOverlays(range.start, range.end)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- View picker -->
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">{{ activeView.name }}</h1>
        <p class="text-xs text-tertiary mt-0.5">{{ activeView.description }}</p>
      </div>
      <div class="flex items-center gap-2 flex-wrap justify-end">
        <button
          v-for="v in builtinResolved"
          :key="v.id"
          type="button"
          class="text-xs font-medium rounded-md px-2.5 py-1.5 transition-colors"
          :class="
            v.id === activeView.id
              ? 'bg-accent text-on-accent'
              : 'text-secondary hover:bg-surface-hover'
          "
          @click="selectView(v)"
        >
          {{ v.name }}
        </button>
        <span
          v-if="savedResolved.length"
          class="w-px h-4 bg-subtle mx-1"
          aria-hidden="true"
        />
        <button
          v-for="v in savedResolved"
          :key="v.id"
          type="button"
          class="text-xs font-medium rounded-md px-2.5 py-1.5 transition-colors"
          :class="
            v.id === activeView.id
              ? 'bg-accent text-on-accent'
              : 'text-secondary hover:bg-surface-hover'
          "
          :title="v.description"
          @click="selectView(v)"
        >
          {{ v.name }}
        </button>
        <span class="w-px h-4 bg-subtle mx-1" aria-hidden="true" />
        <button
          v-if="canEditActiveView"
          type="button"
          class="text-xs font-medium rounded-md px-2.5 py-1.5 text-secondary hover:bg-surface-hover transition-colors"
          @click="renameActiveView"
        >
          Rename
        </button>
        <button
          v-if="canEditActiveView"
          type="button"
          class="text-xs font-medium rounded-md px-2.5 py-1.5 text-secondary hover:bg-surface-hover transition-colors"
          @click="archiveActiveView"
        >
          Archive
        </button>
        <button
          type="button"
          class="text-xs font-medium rounded-md px-2.5 py-1.5 border border-subtle text-primary hover:bg-surface-hover transition-colors disabled:opacity-50"
          :disabled="isSaving"
          @click="saveAsView"
        >
          {{ isSaving ? 'Saving…' : 'Save as view' }}
        </button>
      </div>
    </header>

    <!-- Empty / loading -->
    <div
      v-if="isInitiallyLoading"
      class="flex-1 flex items-center justify-center text-tertiary"
    >
      Loading tickets…
    </div>

    <!-- Calendar shape branches before the empty-state check; an
         empty calendar grid is still useful (the user wants to see
         the month, not a "no tickets" panel). -->
    <CalendarBoard
      v-else-if="activeView.shape.type === 'calendar'"
      class="flex-1 min-h-0"
      :cards="filteredCards"
      :date-field="activeView.shape.date_field"
      :overlays="calendarOverlays"
      :on-card-click="open"
      @visible-range="onCalendarVisibleRange"
    />

    <div
      v-else-if="sortedCards.length === 0"
      class="flex-1 flex flex-col items-center justify-center text-tertiary text-sm"
    >
      <p class="mb-1 font-medium">No tickets match this view.</p>
      <p class="text-xs">Try a different saved view.</p>
    </div>

    <!-- Table -->
    <div v-else class="flex-1 min-h-0 overflow-auto">
      <table class="w-full text-sm">
        <thead class="sticky top-0 z-10 bg-app border-b border-subtle">
          <tr>
            <th
              v-for="col in COLUMNS"
              :key="col.field"
              class="text-left px-3 py-2 text-xs font-medium text-tertiary uppercase tracking-wide"
              :class="col.width"
            >
              <button
                v-if="col.sortable"
                type="button"
                class="flex items-center gap-1 hover:text-primary transition-colors"
                @click="toggleSort(col.field)"
              >
                {{ col.label }}
                <span v-if="sortField === col.field" class="text-[10px]">
                  {{ sortDir === 'asc' ? '↑' : '↓' }}
                </span>
              </button>
              <span v-else>{{ col.label }}</span>
            </th>
          </tr>
        </thead>
        <tbody class="divide-y divide-subtle">
          <tr
            v-for="card in sortedCards"
            :key="card.id"
            class="hover:bg-surface-hover cursor-pointer transition-colors"
            @click="open(card.id)"
          >
            <td class="px-3 py-2 text-tertiary font-mono text-xs">#{{ card.id }}</td>
            <td class="px-3 py-2 text-primary">
              <span class="line-clamp-1">{{ card.title }}</span>
            </td>
            <td class="px-3 py-2">
              <span
                class="inline-flex items-center gap-1.5 text-xs"
              >
                <span
                  class="inline-block w-2 h-2 rounded-full bg-current"
                  :class="paletteForColor(card.workflow_state.color).solid"
                  aria-hidden="true"
                />
                {{ card.workflow_state.name }}
              </span>
            </td>
            <td class="px-3 py-2">
              <PriorityIndicator
                v-if="priorityForBadge(card.priority)"
                :priority="priorityForBadge(card.priority)!"
                size="xs"
              />
              <span v-else class="text-xs text-tertiary">{{ card.priority }}</span>
            </td>
            <td class="px-3 py-2">
              <div v-if="card.assignee_uuid" class="flex items-center gap-2 text-xs">
                <UserAvatar
                  :name="card.assignee_uuid"
                  :avatar="null"
                  size="xxs"
                  :showName="false"
                  :clickable="false"
                />
                <span class="text-secondary truncate">{{ card.assignee_uuid.slice(0, 8) }}…</span>
              </div>
              <span v-else class="text-xs text-tertiary italic">unassigned</span>
            </td>
            <td class="px-3 py-2 text-xs text-tertiary">
              {{ relativeTime(card.last_activity_at) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.line-clamp-1 {
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
