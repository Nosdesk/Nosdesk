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
import { useCyclesStore } from '@/stores/cycles'
import CycleBurndown from '@/components/cycles/CycleBurndown.vue'
import ProjectTabBar from '@/components/views/ProjectTabBar.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import DatePicker from '@/components/common/DatePicker.vue'

const props = defineProps<{ id: string }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

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
  return new Date(iso).toLocaleDateString()
}

function stateLabel(state: string): string {
  switch (state) {
    case 'active': return t('project-cycles-state-active')
    case 'planned': return t('project-cycles-state-planned')
    case 'completed': return t('project-cycles-state-completed')
    default: return state
  }
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">{{ project?.name ?? $t('project-cycles-fallback-name') }}</h1>
        <p class="text-xs text-tertiary mt-0.5">
          {{ $t('project-cycles-count', { count: cycles.length }) }}
        </p>
      </div>
      <button
        type="button"
        class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90"
        @click="showCreate = !showCreate"
      >
        {{ showCreate ? $t('project-cycles-cancel-button') : $t('project-cycles-new-button') }}
      </button>
    </header>

    <ProjectTabBar :project-id="projectId" />

    <div class="flex-1 min-h-0 overflow-y-auto p-6 flex flex-col gap-4">
      <CycleBurndown v-if="activeCycle" :cycle="activeCycle" />

      <SectionCard v-if="showCreate" content-padding="p-4">
        <template #title>{{ $t('project-cycles-create-title') }}</template>
        <form class="flex items-end gap-3" @submit.prevent="createCycle">
          <label class="flex flex-col gap-1 text-[11px] text-tertiary flex-1">
            <span>{{ $t('project-cycles-field-name') }}</span>
            <input
              v-model="newCycleName"
              type="text"
              :placeholder="$t('project-cycles-name-placeholder')"
              class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary"
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
            class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50"
            :disabled="!newCycleName.trim()"
          >{{ $t('project-cycles-create-submit') }}</button>
        </form>
      </SectionCard>

      <SectionCard content-padding="">
        <template #title>{{ $t('project-cycles-all-title') }}</template>
        <template #headerActions>
          <span class="text-[11px] text-tertiary tabular-nums">{{ cycles.length }}</span>
        </template>

        <div
          v-if="cycles.length === 0"
          class="text-tertiary text-xs italic text-center py-8 px-4"
        >
          {{ $t('project-cycles-empty-prefix') }} <strong class="text-secondary">{{ $t('project-cycles-empty-cta') }}</strong> {{ $t('project-cycles-empty-suffix') }}
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
            >{{ stateLabel(cycle.state) }}</span>
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
              >{{ $t('project-cycles-action-promote') }}</button>
              <button
                v-if="cycle.state === 'active'"
                type="button"
                class="text-[11px] text-secondary hover:text-primary px-2 py-1 rounded hover:bg-surface-hover"
                @click="requestCompleteCycle(cycle.uuid)"
              >{{ $t('project-cycles-action-complete') }}</button>
              <button
                v-if="cycle.state !== 'completed'"
                type="button"
                class="text-[11px] text-tertiary hover:text-status-error px-2 py-1 rounded hover:bg-surface-hover"
                @click="requestArchiveCycle(cycle.uuid)"
              >{{ $t('project-cycles-action-archive') }}</button>
            </div>
          </li>
        </ul>
      </SectionCard>
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
