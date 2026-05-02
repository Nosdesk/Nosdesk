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
import { paletteForColor } from '@/utils/workflowColors'
import {
  BUILTIN_VIEWS,
  findBuiltinView,
  TRIAGE_VIEW,
  MY_QUEUE_VIEW,
  type BuiltInView,
} from './builtinViews'
import { buildPredicate } from './filter'
import type { CardData } from './types'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'

const router = useRouter()
const route = useRoute()
const ticketsStore = useSyncTicketsStore()
const authStore = useAuthStore()

// ---------------------------------------------------------------
// Subscription — list view is workspace-scoped, so we subscribe
// to workspace:1 on mount. The lifecycle layer is idempotent
// against repeated subscribes (re-entry to /tickets is a no-op).
// ---------------------------------------------------------------
onMounted(async () => {
  await subscribe('workspace:1')
})

// ---------------------------------------------------------------
// Active view — pulled from `?view=<id>` on the URL with a
// fallback to My Queue. Keeps the URL bookmark-able.
// ---------------------------------------------------------------
const activeView = computed<BuiltInView>(() => {
  const requested = (route.query.view as string | undefined) ?? ''
  return findBuiltinView(requested) ?? MY_QUEUE_VIEW
})

function selectView(view: BuiltInView): void {
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
    if (!t.workflow_state) continue
    out.push({
      id: t.id,
      title: t.title,
      workflow_state: t.workflow_state,
      priority: t.priority,
      assignee_uuid: t.assignee_uuid,
      requester_uuid: t.requester_uuid,
      due_date: null,
      created_at: t.created_at,
      updated_at: t.updated_at,
      last_activity_at: t.last_activity_at,
      category_id: t.category_id,
    })
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
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- View picker -->
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">{{ activeView.name }}</h1>
        <p class="text-xs text-tertiary mt-0.5">{{ activeView.description }}</p>
      </div>
      <div class="flex items-center gap-2">
        <button
          v-for="v in BUILTIN_VIEWS"
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
      </div>
    </header>

    <!-- Empty / loading -->
    <div
      v-if="isInitiallyLoading"
      class="flex-1 flex items-center justify-center text-tertiary"
    >
      Loading tickets…
    </div>
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
