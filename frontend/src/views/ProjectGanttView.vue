<script setup lang="ts">
/**
 * Project Gantt route. Same data plumbing as ProjectDetailView's
 * kanban: subscribe to the project, materialise CardData from the
 * pool's tickets via the shared toCardData helper. Adds a one-shot
 * fetch of the project's dependency edges so the renderer can draw
 * arrows for `blocks`-typed linked_tickets entries.
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useCyclesStore } from '@/stores/cycles'
import { useProjectTickets } from '@/composables/useProjectTickets'
import { useGanttViewport, GANTT_ZOOMS, ganttZoomLabel } from '@/composables/useGanttViewport'
import { dependenciesService, type DependencyEdge } from '@/services/dependenciesService'
import GanttBoard from '@/sync/views/GanttBoard.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import ProjectViewHeader from '@/components/projectComponents/ProjectViewHeader.vue'

const props = defineProps<{ id: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const ticketsStore = useSyncTicketsStore()
const cyclesStore = useCyclesStore()

const project = projectsStore.byId(projectId)
const { cards } = useProjectTickets(projectId)

// Owned here so the viewport toolbar can live in the project tab bar;
// GanttBoard consumes the same instance for all its geometry.
const viewport = useGanttViewport()

// Cycles render as shaded context bands behind the bars. Sourced from
// the cycles store (cached + reactive to cycle mutations).
const cycles = computed(() => cyclesStore.cyclesForProject(projectId.value).value)

const edges = ref<DependencyEdge[]>([])

async function loadEdges(): Promise<void> {
  try {
    edges.value = await dependenciesService.forProject(projectId.value)
  } catch {
    edges.value = []
  }
}

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await Promise.all([loadEdges(), cyclesStore.ensureLoaded(projectId.value)])
})

watch(projectId, async () => {
  await subscribe(`project:${projectId.value}`)
  await Promise.all([loadEdges(), cyclesStore.ensureLoaded(projectId.value)])
})

function openCard(cardId: number): void {
  router.push(`/tickets/${cardId}`)
}

/** Drag-the-due-handle write-back. Optimistic via the sync store, so
 *  the bar stays where it was dropped while the patch round-trips. */
function reschedule(cardId: number, dueDate: string): void {
  void ticketsStore.patchKanbanFields(cardId, { due_date: dueDate })
}

const headerSubtitle = computed(() =>
  t('gantt-tickets-of-total-in-view', {
    count: cards.value.length,
    visible: viewport.visibleCount.value,
  }),
)
</script>

<template>
  <div class="flex flex-col h-full">
    <ProjectViewHeader
      :project="project"
      :subtitle="headerSubtitle"
      :fallback-name="$t('project-gantt-fallback-name')"
    />

    <ProjectTabBar :project-id="projectId">
      <template #actions>
        <!-- Zoom segmented control -->
        <div class="flex items-center rounded-md border border-subtle overflow-hidden">
          <button
            v-for="z in GANTT_ZOOMS"
            :key="z"
            type="button"
            class="text-xs px-2.5 py-1 transition-colors"
            :class="viewport.zoom.value === z
              ? 'bg-accent text-on-accent font-medium'
              : 'text-secondary hover:bg-surface-hover'"
            @click="viewport.setZoom(z)"
          >{{ $t(ganttZoomLabel[z]) }}</button>
        </div>

        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle"
          @click="viewport.fitToProject()"
        >{{ $t('gantt-fit') }}</button>
        <button
          type="button"
          class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle"
          @click="viewport.centerOnToday()"
        >{{ $t('gantt-today') }}</button>

        <div class="flex items-center gap-1">
          <button
            type="button"
            class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
            :aria-label="$t('gantt-pan-previous')"
            :title="$t('gantt-pan-previous')"
            @click="viewport.pan(-1)"
          ><span aria-hidden="true">‹</span></button>
          <button
            type="button"
            class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
            :aria-label="$t('gantt-pan-next')"
            :title="$t('gantt-pan-next')"
            @click="viewport.pan(1)"
          ><span aria-hidden="true">›</span></button>
        </div>
      </template>
    </ProjectTabBar>

    <GanttBoard
      class="flex-1 min-h-0"
      :cards="cards"
      :edges="edges"
      :cycles="cycles"
      :viewport="viewport"
      :on-card-click="openCard"
      :on-reschedule="reschedule"
    />
  </div>
</template>
