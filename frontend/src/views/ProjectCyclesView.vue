<script setup lang="ts">
/**
 * Project / Cycles route: the active cycle is home.
 *
 * Top to bottom: an "ended but not completed" prompt (completion is
 * the deliberate act that freezes the snapshot + carries work over),
 * the active-cycle hero (health, headline numbers, burnup), the
 * active cycle's tickets grouped by state (the board stays one click
 * away at /cycles/:uuid), then Upcoming and Completed as quiet row
 * lists. Creation lives in a modal on the page create action.
 *
 * Data: cycle rows come from the sync pool (seeded once, live via
 * SSE), per-cycle counts fold from the ticket pool by cycle_id, and
 * only the burnup daily series stays a REST query. Completed cycles
 * read their frozen completion_snapshot, so history never moves.
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { type SyncTicket } from '@/sync/stores/tickets'
import { useProjectCycles, type PoolCycle } from '@/composables/useProjectCycles'
import { useCycleMutations } from '@/composables/useCycleMutations'
import { useCycleStats } from '@/composables/useCycleStats'
import { useCycleBurnup } from '@/composables/useCycleBurnup'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { useProjectTickets } from '@/composables/useProjectTickets'
import { dateInputToIso } from '@/utils/cycleDates'
import {
  WORKFLOW_CATEGORIES,
  getCategoryLabel,
  type WorkflowStateCategory,
} from '@nosdesk/core/types/workflow'
import CycleHero from '@/components/cycles/CycleHero.vue'
import CycleListRow from '@/components/cycles/CycleListRow.vue'
import CycleCard from '@/components/cycles/CycleCard.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import ProjectViewHeader from '@/components/projectComponents/ProjectViewHeader.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import PullToRefresh from '@/components/common/PullToRefresh.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Modal from '@/components/Modal.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'
import Icon from '@/components/common/Icon.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import { formatDate, formatCompactDate } from '@nosdesk/core/utils/dateUtils'

const props = defineProps<{ id: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()

// Pull-to-refresh (Tauri app) binds to the scroll container below the
// project tab bar; defaults to the global re-sync.
const scrollEl = ref<HTMLElement | null>(null)
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const mutations = useCycleMutations()

const project = projectsStore.byId(projectId)
const { cycles, activeCycle, seed: seedCycles } = useProjectCycles(projectId)
const { tickets: projectTickets } = useProjectTickets(projectId)
const { statsFor } = useCycleStats(projectTickets)

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await seedCycles()
})
watch(projectId, async () => {
  await subscribe(`project:${projectId.value}`)
  await seedCycles()
})

const upcomingCycles = computed(() => cycles.value.filter((c) => c.state === 'planned'))
const completedCycles = computed(() => cycles.value.filter((c) => c.state === 'completed'))

// Burnup series for the hero (live dated active cycle only).
const { burnup } = useCycleBurnup(
  () => activeCycle.value?.uuid ?? null,
  () => !!activeCycle.value?.start_at && !!activeCycle.value?.end_at,
)
const activeStats = computed(() => (activeCycle.value ? statsFor(activeCycle.value) : null))

// The active cycle's tickets, grouped by workflow-state category, so
// the page shows the in-flight work without a click into the board.
// cycle_id stays live via the backend's ticket.cycle_changed event.
const activeCycleTickets = computed<SyncTicket[]>(() => {
  const cy = activeCycle.value
  if (!cy) return []
  return projectTickets.value.filter((ticket) => ticket.cycle_id === cy.id)
})

const activeCycleGroups = computed(() => {
  const groups = new Map<WorkflowStateCategory, SyncTicket[]>()
  for (const ticket of activeCycleTickets.value) {
    const cat = ticket.workflow_state?.category
    if (!cat) continue
    const bucket = groups.get(cat) ?? []
    bucket.push(ticket)
    groups.set(cat, bucket)
  }
  return WORKFLOW_CATEGORIES.filter((c) => groups.has(c)).map((c) => ({
    category: c,
    label: getCategoryLabel(c),
    tickets: groups.get(c) as SyncTicket[],
  }))
})

// Active cycle whose end date has passed but hasn't been completed.
// Surfaces a prompt to complete it (completion is the deliberate action
// that triggers the snapshot + carryover).
const activeCycleEnded = computed<boolean>(() => {
  const cy = activeCycle.value
  return !!cy && cy.state === 'active' && !!cy.end_at && new Date(cy.end_at).getTime() < Date.now()
})

// Recent velocity: mean completed-ticket count over the last 3 completed
// cycles, read straight off their frozen snapshots. Count-based, no
// points. Planning guidance only, so null until there's history.
const velocity = computed<number | null>(() => {
  const done = completedCycles.value
    .filter((c) => c.completion_snapshot)
    .sort((a, b) => (b.completed_at ?? '').localeCompare(a.completed_at ?? ''))
    .slice(0, 3)
  if (done.length === 0) return null
  const total = done.reduce(
    (sum, c) => sum + Number((c.completion_snapshot as Record<string, unknown>).completed ?? 0),
    0,
  )
  return Math.round(total / done.length)
})

// ---- Create (modal on the page create action) -----------------------
const showCreate = ref(false)
const newCycleName = ref('')
const newCycleStart = ref('')
const newCycleEnd = ref('')
const createPending = ref(false)

usePageCreateAction(() => {
  showCreate.value = true
})

async function createCycle(): Promise<void> {
  const name = newCycleName.value.trim()
  if (!name || createPending.value) return
  createPending.value = true
  try {
    await mutations.create(projectId.value, {
      name,
      start_at: dateInputToIso(newCycleStart.value),
      end_at: dateInputToIso(newCycleEnd.value),
    })
    newCycleName.value = ''
    newCycleStart.value = ''
    newCycleEnd.value = ''
    showCreate.value = false
  } finally {
    createPending.value = false
  }
}

// ---- Lifecycle actions ----------------------------------------------

// Promote guard: the DB enforces one active cycle per project; catch
// it here with a friendly message instead of a constraint error.
const promoteBlocked = ref(false)
async function promoteToActive(uuid: string): Promise<void> {
  if (activeCycle.value) {
    promoteBlocked.value = true
    return
  }
  await mutations.update(uuid, { state: 'active' })
}

// Single pending-action state covers both complete + archive so
// the template renders one ConfirmModal instance.
const pendingAction = ref<
  | { kind: 'complete'; uuid: string }
  | { kind: 'archive'; uuid: string }
  | null
>(null)

/** Where carryover lands, mirroring the backend rule: the next
 *  non-archived planned/active cycle by start date (nulls last). */
function carryoverTarget(excludeUuid: string): PoolCycle | null {
  const candidates = cycles.value.filter(
    (c) => c.uuid !== excludeUuid && (c.state === 'planned' || c.state === 'active'),
  )
  const sorted = [...candidates].sort(
    (a, b) => (a.start_at ?? '9999').localeCompare(b.start_at ?? '9999') || a.id - b.id,
  )
  return sorted[0] ?? null
}

const confirmActionMessage = computed<string>(() => {
  const action = pendingAction.value
  if (!action) return ''
  if (action.kind === 'archive') return t('project-cycles-confirm-archive')
  const cycle = cycles.value.find((c) => c.uuid === action.uuid)
  const stats = cycle ? statsFor(cycle) : null
  const open = stats ? stats.total - stats.completed : 0
  if (open <= 0) return t('project-cycles-confirm-complete')
  const target = carryoverTarget(action.uuid)
  return target
    ? t('project-cycles-confirm-complete-carryover', { count: open, target: target.name })
    : t('project-cycles-confirm-complete-backlog', { count: open })
})

function requestCompleteCycle(uuid: string): void {
  pendingAction.value = { kind: 'complete', uuid }
}
function requestArchiveCycle(uuid: string): void {
  pendingAction.value = { kind: 'archive', uuid }
}
async function confirmCycleAction(): Promise<void> {
  const action = pendingAction.value
  if (!action) return
  pendingAction.value = null
  if (action.kind === 'complete') await mutations.complete(action.uuid)
  else await mutations.archive(action.uuid)
}

function formatCycleDate(iso: string | null | undefined): string {
  if (!iso) return t('project-cycles-date-missing')
  return formatDate(iso)
}

/** PriorityIndicator speaks low/medium/high; urgent renders as high
 *  (same mapping the drag preview uses) and none renders nothing. */
function rowPriority(p: SyncTicket['priority']): 'low' | 'medium' | 'high' | null {
  if (!p || p === 'none') return null
  return p === 'urgent' ? 'high' : p
}

/** Due-chip tint: overdue reads as an error, due within 48 hours as
 *  a warning, anything further out stays quiet. */
function dueClass(due: string): string {
  const ms = new Date(due).getTime() - Date.now()
  if (ms < 0) return 'text-status-error'
  if (ms < 2 * 86_400_000) return 'text-status-warning'
  return 'text-tertiary'
}

// ---- Move-to-cycle menu on active work rows -------------------------
const moveMenu = ref<{ open: boolean; x: number; y: number; ticketId: number | null }>({
  open: false,
  x: 0,
  y: 0,
  ticketId: null,
})

function openMoveMenu(ticket: SyncTicket, event: MouseEvent): void {
  moveMenu.value = { open: true, x: event.clientX, y: event.clientY, ticketId: ticket.id }
}

const moveMenuItems = computed<MenuItem[]>(() => {
  const items: MenuItem[] = cycles.value
    .filter((c) => c.state !== 'completed' && c.id !== activeCycle.value?.id)
    .map((c) => ({ id: `move:${c.uuid}`, label: t('project-cycles-move-to', { name: c.name }) }))
  items.push({ id: 'remove', label: t('project-cycles-remove-from-cycle') })
  return items
})

async function onMoveMenuSelect(id: string): Promise<void> {
  const ticketId = moveMenu.value.ticketId
  moveMenu.value = { ...moveMenu.value, open: false, ticketId: null }
  if (ticketId == null) return
  if (id === 'remove') {
    const cy = activeCycle.value
    if (cy) await mutations.removeTicket(cy.uuid, ticketId)
    return
  }
  if (id.startsWith('move:')) {
    await mutations.addTicket(id.slice('move:'.length), ticketId)
  }
}
</script>

<template>
  <div class="flex flex-col h-full">
    <PullToRefresh :target="scrollEl" />
    <ProjectViewHeader
      :project="project"
      :subtitle="$t('project-cycles-count', { count: cycles.length })"
      :fallback-name="$t('project-cycles-fallback-name')"
    />

    <ProjectTabBar :project-id="projectId">
      <template #actions>
        <button
          type="button"
          class="text-xs font-medium rounded-md px-3 py-1.5 min-h-[44px] sm:min-h-0 inline-flex items-center justify-center bg-accent text-on-accent hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          @click="showCreate = true"
        >{{ $t('project-cycles-new-button') }}</button>
      </template>
    </ProjectTabBar>

    <!-- The scroller is a plain block; the flex layout lives one level
         down. A flex-col scroll container squeezes its children to the
         viewport height before overflowing (the cards clip overflow for
         their rounded corners, so min-height:auto stops protecting
         them), which crushed the work list into a sliver. -->
    <div ref="scrollEl" class="flex-1 min-h-0 overflow-y-auto">
      <div class="p-6 flex flex-col gap-4">
        <!-- No cycles at all: explain + create. -->
        <EmptyState
          v-if="cycles.length === 0"
          icon="calendar"
          variant="page"
          :title="$t('project-cycles-empty-title')"
          :description="$t('project-cycles-empty-description')"
          :action-label="$t('project-cycles-new-button')"
          @action="showCreate = true"
        />

        <template v-else>
          <!-- Ended-but-not-completed: a deliberate prompt above the
               hero, because completing is the human action that freezes
               the snapshot and carries work over. -->
          <div
            v-if="activeCycle && activeCycleEnded"
            class="flex items-center gap-3 rounded-lg border border-status-warning/40 bg-status-warning/10 px-4 py-3 text-sm text-status-warning"
          >
            <span class="flex-1">
              {{ $t('project-cycles-ended-warning', { date: formatCycleDate(activeCycle.end_at) }) }}
            </span>
            <button
              type="button"
              class="text-xs font-medium rounded-md px-3 py-1.5 min-h-[44px] sm:min-h-0 inline-flex items-center justify-center border border-status-warning/50 hover:bg-status-warning/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-status-warning"
              @click="requestCompleteCycle(activeCycle.uuid)"
            >{{ $t('project-cycles-action-complete') }}</button>
          </div>

          <!-- No active cycle: promote the next planned one. -->
          <div
            v-if="!activeCycle && upcomingCycles.length > 0"
            class="flex items-center gap-3 rounded-lg border border-subtle bg-surface px-4 py-3 text-sm text-secondary"
          >
            <span class="flex-1">
              {{ $t('project-cycles-no-active-hint', { name: upcomingCycles[0].name }) }}
            </span>
            <button
              type="button"
              class="text-xs font-medium rounded-md px-3 py-1.5 min-h-[44px] sm:min-h-0 inline-flex items-center justify-center bg-accent text-on-accent hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              @click="promoteToActive(upcomingCycles[0].uuid)"
            >{{ $t('project-cycles-action-start') }}</button>
          </div>

          <!-- Two columns at lg when a cycle is running: the active
               cycle (hero + its work) on the left, Upcoming + Completed
               on the right. Below lg, and when nothing is active,
               everything stacks. -->
          <div
            class="grid items-start gap-4"
            :class="activeCycle && activeStats ? 'lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]' : ''"
          >
            <!-- Left: the active cycle. -->
            <div v-if="activeCycle && activeStats" class="min-w-0 flex flex-col gap-4">
              <CycleHero
                :cycle="activeCycle"
                :stats="activeStats"
                :burnup="burnup"
                variant="full"
                :to="`/cycles/${activeCycle.uuid}`"
              />

              <SectionCard v-if="activeCycleGroups.length > 0" content-padding="">
                <template #title>{{ $t('project-cycles-active-work-title') }}</template>
                <!-- Issue-list density (priority, id, title, due,
                     assignee), responsive against the card's own width:
                     only the title truncates, the due chip yields first
                     when the column narrows. Status is carried by the
                     group header, so rows don't repeat it. -->
                <div class="flex flex-col @container">
                  <div v-for="group in activeCycleGroups" :key="group.category">
                    <div class="px-3 py-1.5 text-[11px] uppercase tracking-wide font-semibold text-tertiary bg-surface-alt border-b border-subtle/50">
                      {{ group.label }} <span class="text-tertiary">({{ group.tickets.length }})</span>
                    </div>
                    <div
                      v-for="ticket in group.tickets"
                      :key="ticket.id"
                      class="group/row flex items-center border-b border-subtle/30 hover:bg-surface-hover transition-colors motion-reduce:transition-none"
                    >
                      <button
                        type="button"
                        class="flex-1 min-w-0 flex items-center gap-2 pl-3 pr-1 py-2 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                        @click="router.push(`/tickets/${ticket.id}`)"
                      >
                        <PriorityIndicator
                          v-if="rowPriority(ticket.priority)"
                          :priority="rowPriority(ticket.priority)!"
                          size="xs"
                          class="shrink-0"
                        />
                        <span class="font-mono text-tertiary text-xs shrink-0">#{{ ticket.id }}</span>
                        <span class="text-sm text-primary truncate flex-1">{{ ticket.title }}</span>
                        <span
                          v-if="ticket.due_date"
                          class="hidden @sm:inline shrink-0 whitespace-nowrap text-xs tabular-nums"
                          :class="dueClass(ticket.due_date)"
                        >{{ formatCompactDate(ticket.due_date) }}</span>
                        <UserAvatar
                          v-if="ticket.assignee_uuid"
                          :uuid="ticket.assignee_uuid"
                          size="xxs"
                          :show-name="false"
                          :clickable="false"
                          class="shrink-0"
                        />
                      </button>
                      <!-- `pointer-coarse:opacity-100`: there is no hover on touch, so
                           without it this menu is permanently invisible, and it is the
                           only way to move a ticket between cycles from this view. Same
                           convention as `TicketDetails` / `CustomDropdown`. -->
                      <button
                        type="button"
                        class="mr-2 p-1 min-h-[44px] min-w-[44px] sm:min-h-0 sm:min-w-0 inline-flex items-center justify-center rounded text-tertiary opacity-0 group-hover/row:opacity-100 pointer-coarse:opacity-100 focus:opacity-100 hover:text-primary hover:bg-surface-alt focus:outline-none focus-visible:ring-2 focus-visible:ring-accent transition-opacity"
                        :aria-label="$t('project-cycles-ticket-menu')"
                        @click="openMoveMenu(ticket, $event)"
                      >
                        <Icon name="more" size="sm" />
                      </button>
                    </div>
                  </div>
                </div>
              </SectionCard>

              <!-- Running cycle with nothing in it yet. -->
              <div
                v-else
                class="rounded-lg border border-subtle bg-surface px-4 py-6 text-center text-sm text-tertiary"
              >
                {{ $t('project-cycles-active-empty') }}
              </div>
            </div>

            <!-- Right: upcoming + completed. -->
            <div class="min-w-0 flex flex-col gap-4">
              <!-- Upcoming -->
              <section v-if="upcomingCycles.length > 0" class="flex flex-col gap-2">
                <div class="flex items-center gap-2 px-0.5">
                  <h2 class="text-sm font-semibold text-primary">{{ $t('project-cycles-upcoming-title') }}</h2>
                  <span class="text-[11px] text-tertiary tabular-nums">{{ upcomingCycles.length }}</span>
                </div>
                <div class="hidden sm:flex flex-col @container bg-surface border border-default rounded-lg p-1.5">
                  <CycleListRow
                    v-for="cycle in upcomingCycles"
                    :key="cycle.uuid"
                    :cycle="cycle"
                    :completed="statsFor(cycle).completed"
                    :total="statsFor(cycle).total"
                    @open="router.push(`/cycles/${cycle.uuid}`)"
                    @promote="promoteToActive(cycle.uuid)"
                    @complete="requestCompleteCycle(cycle.uuid)"
                    @archive="requestArchiveCycle(cycle.uuid)"
                  />
                </div>
                <div class="sm:hidden grid grid-cols-1 gap-4">
                  <CycleCard
                    v-for="cycle in upcomingCycles"
                    :key="cycle.uuid"
                    :cycle="cycle"
                    :completed="statsFor(cycle).completed"
                    :total="statsFor(cycle).total"
                    @open="router.push(`/cycles/${cycle.uuid}`)"
                    @promote="promoteToActive(cycle.uuid)"
                    @complete="requestCompleteCycle(cycle.uuid)"
                    @archive="requestArchiveCycle(cycle.uuid)"
                  />
                </div>
              </section>

              <!-- Completed -->
              <section v-if="completedCycles.length > 0" class="flex flex-col gap-2">
                <div class="flex items-center gap-2 px-0.5">
                  <h2 class="text-sm font-semibold text-primary">{{ $t('project-cycles-completed-title') }}</h2>
                  <span class="text-[11px] text-tertiary tabular-nums">{{ completedCycles.length }}</span>
                </div>
                <div class="hidden sm:flex flex-col @container bg-surface border border-default rounded-lg p-1.5">
                  <CycleListRow
                    v-for="cycle in completedCycles"
                    :key="cycle.uuid"
                    :cycle="cycle"
                    :completed="statsFor(cycle).completed"
                    :total="statsFor(cycle).total"
                    @open="router.push(`/cycles/${cycle.uuid}`)"
                  />
                </div>
                <div class="sm:hidden grid grid-cols-1 gap-4">
                  <CycleCard
                    v-for="cycle in completedCycles"
                    :key="cycle.uuid"
                    :cycle="cycle"
                    :completed="statsFor(cycle).completed"
                    :total="statsFor(cycle).total"
                    @open="router.push(`/cycles/${cycle.uuid}`)"
                  />
                </div>
              </section>
            </div>
          </div>
        </template>
      </div>
    </div>

    <!-- Create cycle -->
    <Modal :show="showCreate" :title="$t('project-cycles-create-title')" size="sm" @close="showCreate = false">
      <form class="flex flex-col gap-3" @submit.prevent="createCycle">
        <p v-if="velocity != null" class="text-[11px] text-tertiary">
          {{ $t('project-cycles-velocity-hint', { count: velocity }) }}
        </p>
        <label class="flex flex-col gap-1 text-xs text-secondary">
          <span>{{ $t('project-cycles-field-name') }}</span>
          <input
            v-model="newCycleName"
            type="text"
            :placeholder="$t('project-cycles-name-placeholder')"
            class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
          />
        </label>
        <div class="flex flex-col sm:flex-row gap-3">
          <label class="flex flex-col gap-1 text-xs text-secondary flex-1">
            <span>{{ $t('project-cycles-field-start') }}</span>
            <DatePicker
              v-model="newCycleStart"
              size="md"
              block
              :aria-label="$t('project-cycles-field-start')"
            />
          </label>
          <label class="flex flex-col gap-1 text-xs text-secondary flex-1">
            <span>{{ $t('project-cycles-field-end') }}</span>
            <DatePicker
              v-model="newCycleEnd"
              size="md"
              block
              :aria-label="$t('project-cycles-field-end')"
            />
          </label>
        </div>
      </form>
      <template #footer>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="text-xs font-medium rounded-md px-3 py-1.5 min-h-[44px] sm:min-h-0 inline-flex items-center justify-center border border-default hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            @click="showCreate = false"
          >{{ $t('project-cycles-cancel-button') }}</button>
          <button
            type="button"
            class="text-xs font-medium rounded-md px-3 py-1.5 min-h-[44px] sm:min-h-0 inline-flex items-center justify-center bg-accent text-on-accent hover:opacity-90 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :disabled="!newCycleName.trim() || createPending"
            @click="createCycle"
          >{{ $t('project-cycles-create-submit') }}</button>
        </div>
      </template>
    </Modal>

    <!-- Move-to-cycle menu for active work rows -->
    <ContextMenu
      :items="moveMenuItems"
      :x="moveMenu.x"
      :y="moveMenu.y"
      :open="moveMenu.open"
      @select="onMoveMenuSelect"
      @close="moveMenu.open = false"
    />

    <ConfirmModal
      :show="pendingAction !== null"
      variant="warning"
      :title="
        pendingAction?.kind === 'complete'
          ? $t('project-cycles-confirm-complete-title')
          : $t('project-cycles-confirm-archive-title')
      "
      :message="confirmActionMessage"
      :confirm-label="
        pendingAction?.kind === 'complete'
          ? $t('project-cycles-action-complete')
          : $t('project-cycles-action-archive')
      "
      @confirm="confirmCycleAction"
      @close="pendingAction = null"
    />

    <!-- Promote guard: one active cycle per project. -->
    <ConfirmModal
      :show="promoteBlocked"
      variant="warning"
      :title="$t('project-cycles-promote-blocked-title')"
      :message="$t('project-cycles-promote-blocked')"
      :confirm-label="$t('project-cycles-promote-blocked-ok')"
      @confirm="promoteBlocked = false"
      @close="promoteBlocked = false"
    />
  </div>
</template>
