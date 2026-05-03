<script setup lang="ts">
/**
 * Workspace cycles overview.
 *
 * Lists every active + planned cycle in the workspace, grouped by
 * project. Each active cycle gets a burndown card; planned cycles
 * render a thinner row with the dates and a "Promote" link. The
 * goal is the answer to "what's in flight right now?" without
 * having to walk every project route.
 *
 * Completed cycles are hidden by default; a toggle pulls them in
 * for retro / planning sessions.
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { cyclesService, type Cycle } from '@/services/cyclesService'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { subscribe } from '@/sync/lifecycle'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'

const router = useRouter()
const projectsStore = useSyncProjectsStore()
const allProjects = projectsStore.all()

const cycles = ref<Cycle[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)
const includeCompleted = ref(false)

async function load(): Promise<void> {
  isLoading.value = true
  error.value = null
  try {
    cycles.value = await cyclesService.listWorkspace(
      includeCompleted.value ? ['planned', 'active', 'completed'] : undefined,
    )
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load cycles'
  } finally {
    isLoading.value = false
  }
}

onMounted(async () => {
  // Workspace subscription pulls projects into the pool so the
  // project-name lookup below resolves without a per-row fetch.
  await subscribe('workspace:1')
  await load()
})

watch(includeCompleted, () => { void load() })

const projectNameById = computed<Map<number, string>>(() => {
  const map = new Map<number, string>()
  for (const p of allProjects.value) {
    map.set(p.id, p.name)
  }
  return map
})

interface ProjectGroup {
  project_id: number
  project_name: string
  cycles: Cycle[]
}

const grouped = computed<ProjectGroup[]>(() => {
  const map = new Map<number, ProjectGroup>()
  for (const c of cycles.value) {
    let group = map.get(c.project_id)
    if (!group) {
      group = {
        project_id: c.project_id,
        project_name: projectNameById.value.get(c.project_id) ?? `Project #${c.project_id}`,
        cycles: [],
      }
      map.set(c.project_id, group)
    }
    group.cycles.push(c)
  }
  return Array.from(map.values()).sort((a, b) => a.project_name.localeCompare(b.project_name))
})

function fmt(iso: string | null): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString()
}

function openProject(projectId: number): void {
  router.push(`/projects/${projectId}`)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">Cycles</h1>
        <p class="text-xs text-tertiary mt-0.5">In-flight iterations across every project</p>
      </div>
      <label class="flex items-center gap-2 text-xs text-secondary">
        <input
          v-model="includeCompleted"
          type="checkbox"
          class="rounded border-subtle"
        />
        <span>Show completed</span>
      </label>
    </header>

    <div v-if="isLoading" class="flex-1 flex items-center justify-center text-tertiary text-sm">
      Loading cycles…
    </div>
    <div v-else-if="error" class="flex-1 flex items-center justify-center text-rose-500 text-sm">
      {{ error }}
    </div>
    <div
      v-else-if="grouped.length === 0"
      class="flex-1 flex flex-col items-center justify-center text-tertiary text-sm gap-1"
    >
      <p class="font-medium">No cycles yet.</p>
      <p class="text-xs">Open a project and start one from the Cycles drawer.</p>
    </div>

    <div v-else class="flex-1 min-h-0 overflow-y-auto p-6 flex flex-col gap-6">
      <section
        v-for="group in grouped"
        :key="group.project_id"
        class="flex flex-col gap-3"
      >
        <header class="flex items-baseline justify-between">
          <button
            type="button"
            class="text-sm font-semibold text-primary hover:text-accent"
            @click="openProject(group.project_id)"
          >{{ group.project_name }}</button>
          <span class="text-xs text-tertiary">{{ group.cycles.length }} cycle{{ group.cycles.length === 1 ? '' : 's' }}</span>
        </header>

        <div class="grid gap-3" style="grid-template-columns: repeat(auto-fill, minmax(20rem, 1fr))">
          <template v-for="cycle in group.cycles" :key="cycle.uuid">
            <CycleBurndown
              v-if="cycle.state === 'active'"
              :cycle="cycle"
            />

            <article
              v-else
              class="rounded-md border border-subtle bg-app p-3 flex flex-col gap-1.5"
            >
              <header class="flex items-center justify-between">
                <h3 class="text-sm font-medium text-primary truncate">{{ cycle.name }}</h3>
                <span
                  class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                  :class="{
                    'bg-surface-hover text-tertiary': cycle.state === 'planned',
                    'bg-surface text-tertiary opacity-70': cycle.state === 'completed',
                  }"
                >{{ cycle.state }}</span>
              </header>
              <p class="text-[11px] text-tertiary tabular-nums">
                {{ fmt(cycle.start_at) }} → {{ fmt(cycle.end_at) }}
              </p>
            </article>
          </template>
        </div>
      </section>
    </div>
  </div>
</template>
