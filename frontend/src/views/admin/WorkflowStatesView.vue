<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AlertMessage from '@/components/common/AlertMessage.vue'
import LoadingSpinner from '@/components/common/LoadingSpinner.vue'
import Icon from '@/components/common/Icon.vue'
import {
  workflowStatesService,
  type CreateWorkflowStateBody,
  type UpdateWorkflowStateBody,
} from '@/services/workflowStatesService'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import {
  CATEGORY_LABELS,
  WORKFLOW_CATEGORIES,
  type WorkflowState,
  type WorkflowStateCategory,
} from '@/types/workflow'
import { paletteForColor, SUPPORTED_COLOR_TOKENS } from '@/utils/workflowColors'

const store = useWorkflowStatesStore()

const isLoading = ref(false)
const errorMessage = ref('')
const successMessage = ref('')

// Pick from the palette the badge actually distinguishes today —
// see workflowColors.ts. Adding `slate` / `purple` back is purely a
// design-system change (distinct CSS vars) once they earn their keep.
const COLOR_TOKENS = SUPPORTED_COLOR_TOKENS

interface DraftState {
  name: string
  color: string
}
const drafts = ref<Record<number, DraftState>>({})
const newStateInputs = ref<Record<WorkflowStateCategory, DraftState>>({
  triage: { name: '', color: 'slate' },
  backlog: { name: '', color: 'gray' },
  active: { name: '', color: 'blue' },
  in_review: { name: '', color: 'purple' },
  done: { name: '', color: 'green' },
  cancelled: { name: '', color: 'subtle' },
})

const grouped = computed<Record<WorkflowStateCategory, WorkflowState[]>>(() => store.byCategory)

async function reload() {
  isLoading.value = true
  errorMessage.value = ''
  try {
    await store.load(true)
    drafts.value = {}
    for (const s of store.states) {
      drafts.value[s.id] = { name: s.name, color: s.color }
    }
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : 'Failed to load workflow states'
  } finally {
    isLoading.value = false
  }
}

function flash(message: string) {
  successMessage.value = message
  setTimeout(() => {
    if (successMessage.value === message) successMessage.value = ''
  }, 2500)
}

async function saveDraft(state: WorkflowState) {
  const draft = drafts.value[state.id]
  if (!draft) return
  const trimmed = draft.name.trim()
  if (!trimmed) {
    errorMessage.value = 'Name is required'
    return
  }
  const patch: UpdateWorkflowStateBody = {}
  if (trimmed !== state.name) patch.name = trimmed
  if (draft.color !== state.color) patch.color = draft.color
  if (Object.keys(patch).length === 0) return

  try {
    await workflowStatesService.update(state.id, patch)
    await reload()
    flash('Saved')
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : 'Failed to save state'
  }
}

async function promoteDefault(state: WorkflowState) {
  if (state.is_default) return
  try {
    await workflowStatesService.update(state.id, { is_default: true })
    await reload()
    flash(`${state.name} is now the default for new tickets`)
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : 'Failed to set default'
  }
}

async function archive(state: WorkflowState) {
  if (state.is_default) {
    errorMessage.value = 'Promote another state as default before archiving this one.'
    return
  }
  if (!confirm(`Archive "${state.name}"? Existing tickets will keep this state.`)) return
  try {
    await workflowStatesService.archive(state.id)
    await reload()
    flash(`${state.name} archived`)
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : 'Failed to archive state'
  }
}

async function createInCategory(category: WorkflowStateCategory) {
  const draft = newStateInputs.value[category]
  const trimmed = draft.name.trim()
  if (!trimmed) {
    errorMessage.value = 'Name is required'
    return
  }
  const body: CreateWorkflowStateBody = {
    name: trimmed,
    category,
    color: draft.color,
  }
  try {
    await workflowStatesService.create(body)
    draft.name = ''
    await reload()
    flash(`${trimmed} added to ${CATEGORY_LABELS[category]}`)
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : 'Failed to create state'
  }
}

onMounted(() => {
  reload()
})
</script>

<template>
  <div class="px-4 sm:px-6 py-6 max-w-4xl mx-auto w-full">
    <header class="mb-6">
      <h1 class="text-2xl font-semibold text-primary">Workflow</h1>
      <p class="text-sm text-secondary mt-1">
        Add named ticket states inside the standard workflow categories. Categories are fixed so
        SLA, dashboards, and automation keep working consistently across teams. New tickets land in
        the state marked as default.
      </p>
    </header>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" class="mb-4" />
    <AlertMessage v-if="successMessage" type="success" :message="successMessage" class="mb-4" />

    <LoadingSpinner v-if="isLoading && store.states.length === 0" />

    <div v-else class="flex flex-col gap-6">
      <section
        v-for="cat in WORKFLOW_CATEGORIES"
        :key="cat"
        class="bg-surface border border-default rounded-lg overflow-hidden"
      >
        <header class="flex items-center justify-between px-4 py-3 bg-surface-alt border-b border-subtle">
          <h2 class="text-sm font-semibold text-primary uppercase tracking-wide">
            {{ CATEGORY_LABELS[cat] }}
          </h2>
          <span class="text-xs text-tertiary">
            {{ grouped[cat]?.length || 0 }} state{{ (grouped[cat]?.length || 0) === 1 ? '' : 's' }}
          </span>
        </header>

        <ul class="divide-y divide-subtle">
          <li
            v-for="state in grouped[cat]"
            :key="state.id"
            class="flex flex-wrap items-center gap-3 px-4 py-3"
          >
            <span
              :class="['inline-block w-3 h-3 rounded-full bg-current flex-shrink-0', paletteForColor(drafts[state.id]?.color || state.color).solid]"
              aria-hidden="true"
            />
            <input
              v-if="drafts[state.id]"
              v-model="drafts[state.id].name"
              type="text"
              maxlength="64"
              class="flex-1 min-w-[150px] bg-transparent border border-subtle rounded px-2 py-1 text-sm text-primary focus:border-accent focus:outline-none"
              @blur="saveDraft(state)"
              @keydown.enter.prevent="saveDraft(state)"
            />
            <select
              v-if="drafts[state.id]"
              v-model="drafts[state.id].color"
              class="bg-surface border border-subtle rounded px-2 py-1 text-sm text-primary"
              @change="saveDraft(state)"
            >
              <option v-for="c in COLOR_TOKENS" :key="c" :value="c">{{ c }}</option>
            </select>
            <span
              v-if="state.is_default"
              class="text-[10px] uppercase tracking-wide font-semibold text-accent border border-accent/40 bg-accent/10 rounded px-1.5 py-0.5"
            >
              Default
            </span>
            <button
              v-else
              type="button"
              class="text-xs text-secondary hover:text-accent transition-colors"
              @click="promoteDefault(state)"
            >
              Make default
            </button>
            <button
              type="button"
              class="text-tertiary hover:text-status-error transition-colors p-1"
              :disabled="state.is_default"
              :title="state.is_default ? 'Cannot archive the default state' : 'Archive state'"
              @click="archive(state)"
            >
              <Icon name="trash" />
            </button>
          </li>
          <li v-if="!grouped[cat] || grouped[cat].length === 0" class="px-4 py-3 text-sm text-tertiary italic">
            No states in this category.
          </li>
        </ul>

        <div class="flex flex-wrap items-center gap-2 px-4 py-3 bg-surface-alt border-t border-subtle">
          <input
            v-model="newStateInputs[cat].name"
            type="text"
            maxlength="64"
            placeholder="Add state name"
            class="flex-1 min-w-[150px] bg-surface border border-subtle rounded px-2 py-1 text-sm text-primary focus:border-accent focus:outline-none"
            @keydown.enter.prevent="createInCategory(cat)"
          />
          <select
            v-model="newStateInputs[cat].color"
            class="bg-surface border border-subtle rounded px-2 py-1 text-sm text-primary"
          >
            <option v-for="c in COLOR_TOKENS" :key="c" :value="c">{{ c }}</option>
          </select>
          <button
            type="button"
            class="text-sm text-accent hover:underline"
            @click="createInCategory(cat)"
          >
            Add
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
