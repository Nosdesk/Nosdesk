<script setup lang="ts">
/**
 * Tickets list — workspace-wide table fed by the sync engine.
 *
 * Layout:
 * - Left: TicketViewsSidebar — persistent list of every view the
 *   user can switch into (Linear pattern: views as nav, not as a
 *   header dropdown). Collapses to icon spine on small viewports.
 * - Right: a thin toolbar (active view name, density toggle,
 *   record count) and a dense scrollable table.
 *
 * View-resolution flow (lock-step with ticketsListLoader):
 *   1. Loader fetches saved views in parallel with the first
 *      ticket page and primes savedViewsStore.
 *   2. On mount the workspace default is already in the store.
 *   3. If the URL has no `?view=`, we push to the canonical view
 *      URL once so the bar stays stable across reloads — no more
 *      My Queue → Triage swap on first frame.
 *
 * Density follows the Pencil & Paper enterprise data table guide:
 * compact 32px / cosy 40px / comfortable 48px row heights, user-
 * controlled, persisted in localStorage.
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
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'

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
// Subscription. The loader has already populated saved views by
// the time we mount, so the activeView resolution below sees the
// workspace default on frame one. ensureLoaded is still called as
// a safety net in case the loader was skipped (e.g. the view is
// re-entered without a navigation).
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

/** Workspace-default saved view, if seeded. */
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

// Canonicalise the URL on first mount so the active view is
// always reflected as `?view=<id>`. Stops the My Queue / Triage
// flip when the user revisits the page or pastes the bare URL.
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
// Sort.
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

const COLUMNS = [
  { field: 'id', label: '#', sortable: true, width: 'w-16' },
  { field: 'title', label: 'Title', sortable: true, width: '' },
  { field: 'workflow_state.name', label: 'Status', sortable: true, width: 'w-32' },
  { field: 'priority', label: 'Priority', sortable: true, width: 'w-20' },
  { field: 'assignee_uuid', label: 'Assignee', sortable: false, width: 'w-32' },
  { field: 'last_activity_at', label: 'Updated', sortable: true, width: 'w-20' },
] as const

// ---------------------------------------------------------------
// Density toggle. Per Pencil & Paper enterprise table guide,
// users should control density via an icon switcher outside the
// table itself. Persisted in localStorage so the choice survives
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
  // Numbers tuned to roughly 32 / 40 / 48 — Pencil & Paper's
  // condensed / regular / relaxed conventions, scaled in for
  // helpdesk dense use.
  if (density.value === 'compact') return 'h-8'
  if (density.value === 'comfortable') return 'h-12'
  return 'h-10'
})

const cellPadding = computed<string>(() =>
  density.value === 'compact' ? 'px-3 py-1' : density.value === 'comfortable' ? 'px-3 py-2.5' : 'px-3 py-1.5',
)

// ---------------------------------------------------------------
// Sidebar items + Save action.
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
// Calendar overlays — unchanged from the previous iteration.
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
      <!-- Toolbar: active view + density + count. The page title
           ("Tickets") lives in the global SiteHeader; this strip
           is purely about the surface the user is operating on,
           which keeps the chrome out of the data area. -->
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

      <!-- Initial-load state. Loader gates against this most of
           the time, but a cold first hit while sync hydrates the
           pool can briefly show this. -->
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

      <!-- Dense table. Sticky header, density-aware row height. -->
      <div v-else class="flex-1 min-h-0 overflow-auto">
        <table class="w-full text-sm border-separate border-spacing-0">
          <thead>
            <tr class="sticky top-0 z-10 bg-surface">
              <th
                v-for="col in COLUMNS"
                :key="col.field"
                class="text-left text-[11px] font-medium text-tertiary uppercase tracking-wide border-b border-subtle bg-surface"
                :class="[col.width, cellPadding]"
              >
                <button
                  v-if="col.sortable"
                  type="button"
                  class="inline-flex items-center gap-1 hover:text-primary transition-colors"
                  @click="toggleSort(col.field)"
                >
                  {{ col.label }}
                  <span v-if="sortField === col.field" class="text-[10px] leading-none">
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
              class="hover:bg-surface-hover cursor-pointer transition-colors group"
              :class="rowClass"
              @click="open(card.id)"
            >
              <td
                class="text-tertiary font-mono text-[11px] tabular-nums border-b border-subtle/40"
                :class="cellPadding"
              >#{{ card.id }}</td>
              <td
                class="text-primary border-b border-subtle/40 min-w-0"
                :class="cellPadding"
              >
                <div class="flex items-center gap-2 min-w-0">
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
              </td>
              <td class="border-b border-subtle/40" :class="cellPadding">
                <span class="inline-flex items-center gap-1.5 text-xs text-secondary">
                  <span
                    class="inline-block w-2 h-2 rounded-full bg-current shrink-0"
                    :class="paletteForColor(card.workflow_state.color).solid"
                    aria-hidden="true"
                  />
                  <span class="truncate">{{ card.workflow_state.name }}</span>
                </span>
              </td>
              <td class="border-b border-subtle/40" :class="cellPadding">
                <PriorityIndicator
                  v-if="priorityForBadge(card.priority)"
                  :priority="priorityForBadge(card.priority)!"
                  size="xs"
                />
                <span v-else class="text-xs text-tertiary">—</span>
              </td>
              <td class="border-b border-subtle/40" :class="cellPadding">
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
              </td>
              <td
                class="text-[11px] text-tertiary tabular-nums border-b border-subtle/40"
                :class="cellPadding"
                :title="new Date(card.last_activity_at).toLocaleString()"
              >
                {{ relativeTime(card.last_activity_at) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>
