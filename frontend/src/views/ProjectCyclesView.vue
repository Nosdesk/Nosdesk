<script setup lang="ts">
/**
 * Project / Cycles route. Same data plumbing as ProjectDetailView's
 * Board, but renders the cycles list + active-cycle burndown +
 * create form as a full-page surface rather than a drawer the user
 * has to remember to open.
 *
 * The active cycle's burndown sits at the top so a sprint team
 * checking in lands on the metric they care about most. Below it,
 * a list of every cycle (planned / active / completed) lets the
 * user navigate into a Scrum board (link to /cycles/:uuid) or
 * trigger lifecycle actions inline.
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { type SyncTicket } from '@/sync/stores/tickets'
import { useCyclesStore } from '@nosdesk/core/stores/cycles'
import { useProjectTickets } from '@/composables/useProjectTickets'
import {
  WORKFLOW_CATEGORIES,
  TERMINAL_CATEGORIES,
  getCategoryLabel,
  type WorkflowStateCategory,
} from '@nosdesk/core/types/workflow'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import CycleCard from '@/components/cycles/CycleCard.vue'
import DataTable from '@/components/common/DataTable.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import type { Cycle } from '@nosdesk/core/services/cyclesService'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import ProjectViewHeader from '@/components/projectComponents/ProjectViewHeader.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import { formatDate } from '@nosdesk/core/utils/dateUtils'

const props = defineProps<{ id: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const cyclesStore = useCyclesStore()

const project = projectsStore.byId(projectId)
// Wrapped so they track projectId across route changes (consistent with
// the gantt view), not bound to the id at first render.
const cycles = computed<Cycle[]>(() => cyclesStore.cyclesForProject(projectId.value).value)
const activeCycle = computed(() => cyclesStore.activeCycle(projectId.value).value)

const { tickets: projectTickets } = useProjectTickets(projectId)

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await cyclesStore.ensureLoaded(projectId.value)
})

// The active cycle's tickets, grouped by workflow-state category, so the
// cycles page shows the in-flight work without a click into the board.
// Project members carry a denormalised cycle_id, so this stays live as
// tickets move, get carried over, etc.
const activeCycleTickets = computed<SyncTicket[]>(() => {
  const cy = activeCycle.value
  if (!cy) return []
  return projectTickets.value.filter((t) => t.cycle_id === cy.id)
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
  const done = cycles.value
    .filter((c) => c.state === 'completed' && c.completion_snapshot)
    .sort((a, b) => (b.completed_at ?? '').localeCompare(a.completed_at ?? ''))
    .slice(0, 3)
  if (done.length === 0) return null
  const total = done.reduce(
    (sum, c) => sum + Number((c.completion_snapshot as Record<string, unknown>).completed ?? 0),
    0,
  )
  return Math.round(total / done.length)
})

const newCycleName = ref('')
const newCycleStart = ref('')
const newCycleEnd = ref('')
const showCreate = ref(false)

async function createCycle(): Promise<void> {
  const name = newCycleName.value.trim()
  if (!name) return
  await cyclesStore.create(projectId.value, {
    name,
    start_at: newCycleStart.value ? new Date(newCycleStart.value).toISOString() : null,
    end_at: newCycleEnd.value ? new Date(newCycleEnd.value).toISOString() : null,
  })
  newCycleName.value = ''
  newCycleStart.value = ''
  newCycleEnd.value = ''
  showCreate.value = false
}

async function promoteToActive(uuid: string): Promise<void> {
  await cyclesStore.update(uuid, { state: 'active' })
}

// Single pending-action state covers both complete + archive so
// the template renders one ConfirmModal instance.
const pendingAction = ref<
  | { kind: 'complete'; uuid: string }
  | { kind: 'archive'; uuid: string }
  | null
>(null)
const confirmActionMessage = computed<string>(() => {
  if (!pendingAction.value) return ''
  return pendingAction.value.kind === 'complete'
    ? t('project-cycles-confirm-complete')
    : t('project-cycles-confirm-archive')
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
  if (action.kind === 'complete') await cyclesStore.complete(action.uuid)
  else await cyclesStore.archive(action.uuid)
}

function formatCycleDate(iso: string | null): string {
  if (!iso) return t('project-cycles-date-missing')
  return formatDate(iso)
}

// Per-cycle completed/total, derived from the project's ticket pool by
// cycle_id (the pool carries every ticket, denormalised with cycle_id).
// Completed cycles read from their frozen snapshot so post-completion
// edits don't move the numbers.
const cycleStats = computed(() => {
  const map = new Map<number, { completed: number; total: number }>()
  for (const ticket of projectTickets.value) {
    if (ticket.cycle_id == null) continue
    const s = map.get(ticket.cycle_id) ?? { completed: 0, total: 0 }
    s.total++
    if (ticket.workflow_state && TERMINAL_CATEGORIES.has(ticket.workflow_state.category)) s.completed++
    map.set(ticket.cycle_id, s)
  }
  return map
})
function statsFor(cycle: { id: number; state: string; completion_snapshot?: unknown }): {
  completed: number
  total: number
} {
  if (cycle.state === 'completed' && cycle.completion_snapshot) {
    const snap = cycle.completion_snapshot as Record<string, unknown>
    return { completed: Number(snap.completed ?? 0), total: Number(snap.tickets ?? 0) }
  }
  return cycleStats.value.get(cycle.id) ?? { completed: 0, total: 0 }
}
function pctFor(cycle: Cycle): number {
  const s = statsFor(cycle)
  return s.total > 0 ? Math.round((s.completed / s.total) * 100) : 0
}

// DataTable is generic, but its dynamic cell slots don't reliably flow
// the row type through vue-tsc, so the slot `item` lands as `object`.
// This narrows it back to Cycle at the call site.
const asCycle = (item: object): Cycle => item as Cycle

// Shared state-pill mapping for the desktop table's state cell (the
// mobile CycleCard computes its own). Active is the figure (accent);
// planned + completed recede to neutral.
function stateLabel(state: string): string {
  switch (state) {
    case 'active': return t('project-cycles-state-active')
    case 'planned': return t('project-cycles-state-planned')
    case 'completed': return t('project-cycles-state-completed')
    default: return state
  }
}
function stateTone(state: string): StatusPillTone {
  return state === 'active' ? 'accent' : 'neutral'
}

// Desktop list columns, mirroring the projects table.
const cycleColumns = computed(() => [
  { field: 'state', label: t('project-cycles-col-state'), width: '150px' },
  { field: 'name', label: t('project-cycles-col-name'), width: 'minmax(180px, 1fr)' },
  { field: 'dates', label: t('project-cycles-col-dates'), width: 'minmax(150px, 230px)' },
  { field: 'progress', label: t('project-cycles-col-progress'), width: 'minmax(140px, 220px)' },
  { field: 'actions', label: '', width: '132px' },
])
</script>

<template>
  <div class="flex flex-col h-full">
    <ProjectViewHeader
      :project="project"
      :subtitle="$t('project-cycles-count', { count: cycles.length })"
      :fallback-name="$t('project-cycles-fallback-name')"
    />

    <ProjectTabBar :project-id="projectId">
      <template #actions>
        <button
          type="button"
          class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          @click="showCreate = !showCreate"
        >
          {{ showCreate ? $t('project-cycles-cancel-button') : $t('project-cycles-new-button') }}
        </button>
      </template>
    </ProjectTabBar>

    <div class="flex-1 min-h-0 overflow-y-auto p-6 flex flex-col gap-4">
      <template v-if="activeCycle">
        <div
          v-if="activeCycleEnded"
          class="flex items-center gap-3 rounded-lg border border-status-warning/40 bg-status-warning/10 px-4 py-3 text-sm text-status-warning"
        >
          <span class="flex-1">
            {{ $t('project-cycles-ended-warning', { date: formatCycleDate(activeCycle.end_at) }) }}
          </span>
          <button
            type="button"
            class="text-xs font-medium rounded-md px-3 py-1.5 border border-status-warning/50 hover:bg-status-warning/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-status-warning"
            @click="requestCompleteCycle(activeCycle.uuid)"
          >{{ $t('project-cycles-action-complete') }}</button>
        </div>

        <CycleBurndown :cycle="activeCycle" :to="`/cycles/${activeCycle.uuid}`" />

        <SectionCard v-if="activeCycleGroups.length > 0" content-padding="">
          <template #title>{{ $t('project-cycles-active-work-title') }}</template>
          <div class="flex flex-col">
            <div v-for="group in activeCycleGroups" :key="group.category">
              <div class="px-3 py-1.5 text-[10px] uppercase tracking-wide font-semibold text-tertiary bg-surface-alt border-b border-subtle/50">
                {{ group.label }} <span class="text-tertiary">({{ group.tickets.length }})</span>
              </div>
              <button
                v-for="ticket in group.tickets"
                :key="ticket.id"
                type="button"
                class="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-surface-hover transition-colors motion-reduce:transition-none border-b border-subtle/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
                @click="router.push(`/tickets/${ticket.id}`)"
              >
                <span class="font-mono text-tertiary text-xs shrink-0">#{{ ticket.id }}</span>
                <span class="text-sm text-primary truncate flex-1">{{ ticket.title }}</span>
              </button>
            </div>
          </div>
        </SectionCard>
      </template>

      <SectionCard v-if="showCreate" content-padding="p-4">
        <template #title>{{ $t('project-cycles-create-title') }}</template>
        <p v-if="velocity != null" class="text-[11px] text-tertiary mb-3">
          {{ $t('project-cycles-velocity-hint', { count: velocity }) }}
        </p>
        <form class="flex flex-col sm:flex-row sm:items-end gap-3" @submit.prevent="createCycle">
          <label class="flex flex-col gap-1 text-[11px] text-tertiary flex-1">
            <span>{{ $t('project-cycles-field-name') }}</span>
            <input
              v-model="newCycleName"
              type="text"
              :placeholder="$t('project-cycles-name-placeholder')"
              class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            />
          </label>
          <label class="flex flex-col gap-1 text-[11px] text-tertiary">
            <span>{{ $t('project-cycles-field-start') }}</span>
            <DatePicker
              v-model="newCycleStart"
              size="md"
              block
              :aria-label="$t('project-cycles-field-start')"
            />
          </label>
          <label class="flex flex-col gap-1 text-[11px] text-tertiary">
            <span>{{ $t('project-cycles-field-end') }}</span>
            <DatePicker
              v-model="newCycleEnd"
              size="md"
              block
              :aria-label="$t('project-cycles-field-end')"
            />
          </label>
          <button
            type="submit"
            class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :disabled="!newCycleName.trim()"
          >{{ $t('project-cycles-create-submit') }}</button>
        </form>
      </SectionCard>

      <section class="flex flex-col gap-3">
        <div class="flex items-center gap-2 px-0.5">
          <h2 class="text-sm font-semibold text-primary">{{ $t('project-cycles-all-title') }}</h2>
          <span class="text-[11px] text-tertiary tabular-nums">{{ cycles.length }}</span>
        </div>

        <div
          v-if="cycles.length === 0"
          class="text-tertiary text-xs italic text-center py-10 px-4 bg-surface border border-default rounded-lg"
        >
          {{ $t('project-cycles-empty-prefix') }} <strong class="text-secondary">{{ $t('project-cycles-empty-cta') }}</strong> {{ $t('project-cycles-empty-suffix') }}
        </div>

        <template v-else>
          <!-- Desktop: full list view (shared DataTable), like projects. -->
          <div class="hidden lg:block bg-surface border border-default rounded-lg overflow-hidden">
            <DataTable
              :columns="cycleColumns"
              :data="cycles"
              :selected-items="[]"
              :selectable="false"
              item-id-field="uuid"
              @row-click="(c) => router.push(`/cycles/${asCycle(c).uuid}`)"
            >
              <template #cell-state="{ item }">
                <StatusPill
                  :tone="stateTone(asCycle(item).state)"
                  :label="stateLabel(asCycle(item).state)"
                  :class="{ 'opacity-70': asCycle(item).state === 'completed' }"
                />
              </template>
              <template #cell-name="{ item }">
                <button
                  type="button"
                  class="block max-w-full truncate text-left text-sm font-medium text-primary rounded transition-colors hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                  @click.stop="router.push(`/cycles/${asCycle(item).uuid}`)"
                >{{ asCycle(item).name }}</button>
              </template>
              <template #cell-dates="{ item }">
                <span class="text-xs text-tertiary tabular-nums">
                  {{ formatCycleDate(asCycle(item).start_at) }} → {{ formatCycleDate(asCycle(item).end_at) }}
                </span>
              </template>
              <template #cell-progress="{ item }">
                <div class="flex items-center gap-2 w-full">
                  <div class="flex-1 h-1.5 rounded-full bg-surface-hover overflow-hidden">
                    <div class="h-full rounded-full bg-accent" :style="{ width: `${pctFor(asCycle(item))}%` }" />
                  </div>
                  <span class="shrink-0 text-xs tabular-nums text-tertiary">
                    {{ statsFor(asCycle(item)).completed }}/{{ statsFor(asCycle(item)).total }}
                  </span>
                </div>
              </template>
              <template #cell-actions="{ item }">
                <div class="flex items-center gap-0.5" @click.stop>
                  <button
                    v-if="asCycle(item).state === 'planned'"
                    type="button"
                    class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    @click="promoteToActive(asCycle(item).uuid)"
                  >{{ $t('project-cycles-action-promote') }}</button>
                  <button
                    v-if="asCycle(item).state === 'active'"
                    type="button"
                    class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    @click="requestCompleteCycle(asCycle(item).uuid)"
                  >{{ $t('project-cycles-action-complete') }}</button>
                  <button
                    v-if="asCycle(item).state !== 'completed'"
                    type="button"
                    class="text-[11px] text-tertiary hover:text-status-error px-2 py-1 rounded hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-status-error"
                    @click="requestArchiveCycle(asCycle(item).uuid)"
                  >{{ $t('project-cycles-action-archive') }}</button>
                </div>
              </template>
            </DataTable>
          </div>

          <!-- Tablet / mobile: enriched cards. -->
          <div class="lg:hidden grid grid-cols-1 sm:grid-cols-2 gap-4">
            <CycleCard
              v-for="cycle in cycles"
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
        </template>
      </section>
    </div>

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
  </div>
</template>
