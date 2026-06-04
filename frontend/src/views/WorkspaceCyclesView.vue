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
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { cyclesService, type Cycle } from '@/services/cyclesService'
import { useSyncProjectsStore } from '@/sync/stores/projects'
import { subscribe } from '@/sync/lifecycle'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'
import { formatDate } from '@/utils/dateUtils'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const router = useRouter()
const projectsStore = useSyncProjectsStore()
const allProjects = projectsStore.all()

const includeCompleted = ref(false)

// Cycles are cached by Pinia Colada, keyed by the include-completed
// filter so each variant caches independently and a revisit renders
// instantly with a silent background revalidate. Toggling the filter
// switches keys, so Colada refetches that variant (no manual watch).
// A skeleton shows only on a cold cache for the active variant.
const cyclesQuery = useQuery({
  key: () => ['workspace-cycles', includeCompleted.value ? 'all' : 'open'],
  query: () =>
    cyclesService.listWorkspace(
      includeCompleted.value ? ['planned', 'active', 'completed'] : undefined,
    ),
})
const cycles = computed<Cycle[]>(() =>
  Array.isArray(cyclesQuery.data.value) ? cyclesQuery.data.value : [],
)
const isFirstLoad = computed(
  () => cyclesQuery.status.value === 'pending' && cyclesQuery.data.value === undefined,
)
const loadError = computed(() => {
  const e = cyclesQuery.error.value
  if (!e) return ''
  return e instanceof Error ? e.message : t('workspace-cycles-error-fallback')
})

onMounted(() => {
  // Workspace subscription pulls projects into the pool so the
  // project-name lookup below resolves without a per-row fetch.
  void subscribe('workspace:1')
})

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
        project_name: projectNameById.value.get(c.project_id) ?? t('workspace-cycles-project-fallback', { id: c.project_id }),
        cycles: [],
      }
      map.set(c.project_id, group)
    }
    group.cycles.push(c)
  }
  return Array.from(map.values()).sort((a, b) => a.project_name.localeCompare(b.project_name))
})

function fmt(iso: string | null): string {
  if (!iso) return t('workspace-cycles-date-missing')
  return formatDate(iso)
}

function stateLabel(state: string): string {
  switch (state) {
    case 'planned': return t('workspace-cycles-state-planned')
    case 'completed': return t('workspace-cycles-state-completed')
    default: return state
  }
}

function openProject(projectId: number): void {
  router.push(`/projects/${projectId}`)
}

function openCycle(uuid: string): void {
  router.push(`/cycles/${uuid}`)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">{{ $t('workspace-cycles-heading') }}</h1>
        <p class="text-xs text-tertiary mt-0.5">{{ $t('workspace-cycles-subheading') }}</p>
      </div>
      <Checkbox
        v-model="includeCompleted"
        size="sm"
        :label="$t('workspace-cycles-show-completed')"
      />
    </header>

    <Skeleton
      v-if="isFirstLoad"
      :label="$t('workspace-cycles-loading')"
      class="flex-1 overflow-y-auto p-6 flex flex-col gap-6"
    >
      <section v-for="n in 2" :key="n" class="flex flex-col gap-3">
        <SkeletonBar class="h-4 w-48 max-w-full" />
        <SkeletonBar class="h-24 w-full rounded-xl" />
      </section>
    </Skeleton>
    <div v-else-if="loadError" class="flex-1 flex items-center justify-center text-status-error text-sm">
      {{ loadError }}
    </div>
    <div
      v-else-if="grouped.length === 0"
      class="flex-1 flex flex-col items-center justify-center text-tertiary text-sm gap-1"
    >
      <p class="font-medium">{{ $t('workspace-cycles-empty-title') }}</p>
      <p class="text-xs">{{ $t('workspace-cycles-empty-hint') }}</p>
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
          <span class="text-xs text-tertiary">{{ $t('workspace-cycles-group-count', { count: group.cycles.length }) }}</span>
        </header>

        <div class="grid gap-3" style="grid-template-columns: repeat(auto-fill, minmax(20rem, 1fr))">
          <template v-for="cycle in group.cycles" :key="cycle.uuid">
            <div
              v-if="cycle.state === 'active'"
              class="cursor-pointer"
              @click="openCycle(cycle.uuid)"
            >
              <CycleBurndown :cycle="cycle" />
            </div>

            <article
              v-else
              class="rounded-md border border-subtle bg-app p-3 flex flex-col gap-1.5 cursor-pointer hover:border-default transition-colors"
              @click="openCycle(cycle.uuid)"
            >
              <header class="flex items-center justify-between">
                <h3 class="text-sm font-medium text-primary truncate">{{ cycle.name }}</h3>
                <span
                  class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                  :class="{
                    'bg-surface-hover text-tertiary': cycle.state === 'planned',
                    'bg-surface text-tertiary opacity-70': cycle.state === 'completed',
                  }"
                >{{ stateLabel(cycle.state) }}</span>
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
