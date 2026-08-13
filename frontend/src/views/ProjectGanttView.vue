<script setup lang="ts">
/**
 * Project Gantt route. Same data plumbing as ProjectDetailView's
 * kanban: subscribe to the project, materialise CardData from the
 * pool's tickets via the shared toCardData helper. Adds a one-shot
 * fetch of the project's dependency edges so the renderer can draw
 * arrows for `blocks`-typed linked_tickets entries.
 */
import { computed, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useProjectCycles } from '@/composables/useProjectCycles'
import { useProjectTickets } from '@/composables/useProjectTickets'
import { useGanttViewport } from '@/composables/useGanttViewport'
import { useListGrouping, NONE_AXIS_KEY } from '@/composables/useListGrouping'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { useListDensity } from '@/composables/useTicketsDensity'
import { useProjectDependencies } from '@/composables/useProjectDependencies'
import { getCategoryLabel, WORKFLOW_CATEGORIES } from '@nosdesk/core/types/workflow'
import GanttBoard from '@/sync/views/gantt/GanttBoard.vue'
import VerticalTimeline from '@/sync/views/gantt/VerticalTimeline.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'
import type { ScheduledCard } from '@/sync/views/gantt/rowModel'
import GanttToolbar from '@/components/views/GanttToolbar.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import ProjectViewHeader from '@/components/projectComponents/ProjectViewHeader.vue'

const props = defineProps<{ id: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()

const project = projectsStore.byId(projectId)
const { cards } = useProjectTickets(projectId)

// Cycles read live from the sync pool (bands + grouping); the seed
// covers cold entry, SSE keeps them current.
const { cycles, seed: seedCycles } = useProjectCycles(projectId)

// Owned here so the viewport toolbar can live in the project tab bar;
// GanttBoard consumes the same instance for all its geometry. Zoom +
// scroll position persist per project.
const viewport = useGanttViewport({
  storageKey: () => `gantt-viewport:${projectId.value}`,
})

// ---------------- row grouping ----------------

const { getUser } = useUsersDirectory()

/** Bucket-sort order for the cycle axis: date order, No cycle last. */
const cycleOrder = computed(() => {
  const sorted = [...cycles.value].sort((a, b) =>
    (a.start_at ?? '9999').localeCompare(b.start_at ?? '9999'),
  )
  return new Map(sorted.map((c, i) => [`cycle:${c.id}`, i]))
})

const { isMobile } = useMobileDetection('md')

const grouping = useListGrouping<ScheduledCard>({
  storageNamespace: 'gantt',
  getViewId: () => String(projectId.value),
  t,
  axes: [
    {
      key: 'cycle',
      labelKey: 'gantt-group-cycle',
      bucketFor: (s) => {
        const id = s.card.cycle_id
        if (id == null) return { key: 'cycle-none', label: t('gantt-group-no-cycle') }
        const cycle = cycles.value.find((c) => c.id === id)
        return { key: `cycle:${id}`, label: cycle?.name ?? `#${id}` }
      },
      sortBy: (key) => (key === 'cycle-none' ? 9999 : (cycleOrder.value.get(key) ?? 9998)),
    },
    {
      key: 'state',
      labelKey: 'gantt-group-state',
      bucketFor: (s) => ({
        key: s.card.workflow_state.category,
        label: getCategoryLabel(s.card.workflow_state.category),
      }),
      sortBy: (key) => {
        const i = WORKFLOW_CATEGORIES.indexOf(key as (typeof WORKFLOW_CATEGORIES)[number])
        return i === -1 ? 99 : i
      },
    },
    {
      key: 'assignee',
      labelKey: 'gantt-group-assignee',
      bucketFor: (s) => {
        const uuid = s.card.assignee_uuid
        if (!uuid) return { key: 'assignee-none', label: t('gantt-group-unassigned') }
        return { key: `assignee:${uuid}`, label: getUser(uuid).value?.name ?? '' }
      },
      // Unassigned sorts last; '~' sorts after letters.
      sortBy: (key, label) => (key === 'assignee-none' ? '~~~' : label.toLowerCase()),
    },
  ],
})

// Cycles are the gantt's differentiator: default to cycle grouping
// the first time a project that has cycles opens (no stored pref).
// Choosing "No grouping" clears the stored key, so a reload can
// re-apply this default; accepted trade-off for a one-line rule.
watch(cycles, (list) => {
  if (list.length === 0 || grouping.groupBy.value !== NONE_AXIS_KEY) return
  if (localStorage.getItem(`gantt-group-by:${projectId.value}`) == null) {
    grouping.setGroupBy('cycle')
  }
})

// Dependency edges: Pinia Colada (cache-first, silent revalidate).
// Failure keeps the board fully usable; a slim notice offers retry.
const { edges, failed: edgesFailed, refetch: refetchEdges } = useProjectDependencies(projectId)

// Row density: comfortable by default on the gantt (the design's
// calm default); the toggle persists compact/cosy per user.
const { density, setDensity } = useListDensity('gantt-density')
if (typeof localStorage !== 'undefined' && localStorage.getItem('gantt-density') == null) {
  setDensity('comfortable')
}

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await seedCycles()
})

watch(projectId, async () => {
  await subscribe(`project:${projectId.value}`)
  await seedCycles()
})

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

/** Direct-manipulation write-back (bar move, either edge handle, or
 *  a tray drop). Optimistic via the sync store, so the bar stays
 *  where it was dropped while the patch round-trips. */
function reschedule(
  cardId: number,
  patch: { start_date?: string; due_date?: string },
): void {
  void ticketsStore.patchKanbanFields(cardId, patch)
}

const headerSubtitle = computed(() => {
  // `visibleCount` counts bars inside the HORIZONTAL viewport's scroll window.
  // The vertical timeline has no such window — it lays every scheduled ticket
  // out down the page — so the counter reported "0 of 4 in view" while four
  // were plainly on screen. Below `md` report the plain total instead.
  if (isMobile.value) return t('gantt-tickets-total', { count: cards.value.length })
  return t('gantt-tickets-of-total-in-view', {
    count: cards.value.length,
    visible: viewport.visibleCount.value,
  })
})
</script>

<template>
  <!-- @container: the toolbar + board responsiveness key off this
       panel's width (correct regardless of nav sidebar state), matching
       the @container convention used elsewhere in the app. -->
  <div class="@container flex flex-col h-full">
    <ProjectViewHeader
      :project="project"
      :subtitle="headerSubtitle"
      :fallback-name="$t('project-gantt-fallback-name')"
    />

    <ProjectTabBar :project-id="projectId">
      <template #actions>
        <GanttToolbar
        v-if="!isMobile"
          :viewport="viewport"
          :grouping="grouping"
          :density="density"
          :set-density="setDensity"
        />
      </template>
    </ProjectTabBar>

    <!-- Arrows are supplementary: a failed edges fetch never blocks
         the board, it just loses the dependency layer + offers retry. -->
    <div
      v-if="edgesFailed"
      class="flex items-center justify-between gap-2 px-3 py-1.5 text-xs text-secondary bg-surface-alt border-b border-subtle"
    >
      <span>{{ t('gantt-dependencies-failed') }}</span>
      <button
        type="button"
        class="font-medium text-accent hover:underline"
        @click="refetchEdges()"
      >{{ t('gantt-retry') }}</button>
    </div>

    <!-- Below `md` the gantt transposes: time runs down and concurrent tickets
         become columns. A horizontal gantt is bounded by time span, which no
         phone can satisfy (3737px of canvas in 390px); the vertical form is
         bounded by concurrency instead, and puts the unbounded axis on the one
         the device scrolls naturally. See docs/plans/gantt-mobile-research.md. -->
    <VerticalTimeline
      v-if="isMobile"
      class="flex-1 min-h-0"
      :cards="cards"
      :cycles="cycles"
      :on-card-click="openCard"
      :on-reschedule="reschedule"
    />
    <GanttBoard
      v-else
      class="flex-1 min-h-0"
      :cards="cards"
      :edges="edges"
      :cycles="cycles"
      :viewport="viewport"
      :grouping="grouping"
      :density="density"
      :on-card-click="openCard"
      :on-reschedule="reschedule"
    />
  </div>
</template>
