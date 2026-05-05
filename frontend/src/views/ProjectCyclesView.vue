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
import { subscribe } from '@/sync/lifecycle'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { useCyclesStore } from '@/stores/cycles'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import SectionCard from '@/components/common/SectionCard.vue'

const props = defineProps<{ id: string }>()

const router = useRouter()
const projectId = computed(() => Number(props.id))
const projectsStore = useSyncProjectsStore()
const cyclesStore = useCyclesStore()

const project = projectsStore.byId(projectId)
const cycles = cyclesStore.cyclesForProject(projectId.value)
const activeCycle = cyclesStore.activeCycle(projectId.value)

onMounted(async () => {
  await subscribe(`project:${projectId.value}`)
  await cyclesStore.ensureLoaded(projectId.value)
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

async function completeCycle(uuid: string): Promise<void> {
  if (!window.confirm('Complete this cycle? The snapshot freezes once you do.')) return
  await cyclesStore.complete(uuid)
}

async function archiveCycle(uuid: string): Promise<void> {
  if (!window.confirm('Archive this cycle?')) return
  await cyclesStore.archive(uuid)
}

function formatCycleDate(iso: string | null): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString()
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">{{ project?.name ?? 'Project' }}</h1>
        <p class="text-xs text-tertiary mt-0.5">
          {{ cycles.length }} cycle{{ cycles.length === 1 ? '' : 's' }}
        </p>
      </div>
      <button
        type="button"
        class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90"
        @click="showCreate = !showCreate"
      >
        {{ showCreate ? 'Cancel' : 'New cycle' }}
      </button>
    </header>

    <ProjectTabBar :project-id="projectId" />

    <div class="flex-1 min-h-0 overflow-y-auto p-6 flex flex-col gap-4">
      <CycleBurndown v-if="activeCycle" :cycle="activeCycle" />

      <SectionCard v-if="showCreate" content-padding="p-4">
        <template #title>New cycle</template>
        <form class="flex items-end gap-3" @submit.prevent="createCycle">
          <label class="flex flex-col gap-1 text-[11px] text-tertiary flex-1">
            <span>Name</span>
            <input
              v-model="newCycleName"
              type="text"
              placeholder="e.g. Sprint 14"
              class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary"
            />
          </label>
          <label class="flex flex-col gap-1 text-[11px] text-tertiary">
            <span>Start</span>
            <input
              v-model="newCycleStart"
              type="date"
              class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary"
            />
          </label>
          <label class="flex flex-col gap-1 text-[11px] text-tertiary">
            <span>End</span>
            <input
              v-model="newCycleEnd"
              type="date"
              class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary"
            />
          </label>
          <button
            type="submit"
            class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50"
            :disabled="!newCycleName.trim()"
          >Create</button>
        </form>
      </SectionCard>

      <SectionCard content-padding="">
        <template #title>All cycles</template>
        <template #headerActions>
          <span class="text-[11px] text-tertiary tabular-nums">{{ cycles.length }}</span>
        </template>

        <div
          v-if="cycles.length === 0"
          class="text-tertiary text-xs italic text-center py-8 px-4"
        >
          No cycles yet. Click <strong class="text-secondary">New cycle</strong> to start an iteration.
        </div>

        <ul v-else class="divide-y divide-subtle">
          <li
            v-for="cycle in cycles"
            :key="cycle.uuid"
            class="flex items-center gap-3 px-3 py-2 hover:bg-surface-hover transition-colors"
          >
            <span
              class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5 shrink-0"
              :class="{
                'bg-accent text-on-accent': cycle.state === 'active',
                'bg-surface-hover text-tertiary': cycle.state === 'planned',
                'bg-surface text-tertiary opacity-70': cycle.state === 'completed',
              }"
            >{{ cycle.state }}</span>
            <button
              type="button"
              class="text-sm text-primary flex-1 truncate text-left hover:text-accent"
              @click="router.push(`/cycles/${cycle.uuid}`)"
            >{{ cycle.name }}</button>
            <span class="text-[11px] text-tertiary tabular-nums shrink-0">
              {{ formatCycleDate(cycle.start_at) }} → {{ formatCycleDate(cycle.end_at) }}
            </span>
            <div class="flex items-center gap-0.5 shrink-0">
              <button
                v-if="cycle.state === 'planned'"
                type="button"
                class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover"
                @click="promoteToActive(cycle.uuid)"
              >Promote</button>
              <button
                v-if="cycle.state === 'active'"
                type="button"
                class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover"
                @click="completeCycle(cycle.uuid)"
              >Complete</button>
              <button
                v-if="cycle.state !== 'completed'"
                type="button"
                class="text-[11px] text-tertiary hover:text-status-error px-2 py-1 rounded hover:bg-surface-hover"
                @click="archiveCycle(cycle.uuid)"
              >Archive</button>
            </div>
          </li>
        </ul>
      </SectionCard>
    </div>
  </div>
</template>
