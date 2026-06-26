<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import AlertMessage from '@/components/common/AlertMessage.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import LoadingSpinner from '@/components/common/LoadingSpinner.vue'
import Icon from '@/components/common/Icon.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import {
  workflowStatesService,
  type CreateWorkflowStateBody,
  type UpdateWorkflowStateBody,
} from '@/services/workflowStatesService'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import {
  getCategoryLabel,
  WORKFLOW_CATEGORIES,
  type WorkflowState,
  type WorkflowStateCategory,
} from '@nosdesk/core/types/workflow'
import { paletteForColor, SUPPORTED_COLOR_TOKENS } from '@/utils/workflowColors'

const store = useWorkflowStatesStore()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const isLoading = ref(false)
const errorMessage = ref('')
const successMessage = ref('')

// Pick from the palette the badge actually distinguishes today —
// see workflowColors.ts. Adding `slate` / `purple` back is purely a
// design-system change (distinct CSS vars) once they earn their keep.
const COLOR_TOKENS = SUPPORTED_COLOR_TOKENS

// BaseDropdown options for the palette, each carrying its swatch as a
// leading tone dot so the menu previews the colour.
const colorOptions = COLOR_TOKENS.map((c) => ({
  value: c,
  label: c,
  tones: [paletteForColor(c).solid],
}))

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
  // `merged` is set by the merge action, not picked from the admin
  // form, so the row never renders. Initialised purely to satisfy
  // the `Record<WorkflowStateCategory, _>` shape.
  merged: { name: '', color: 'subtle' },
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
    errorMessage.value = e instanceof Error ? e.message : t('admin-workflow-states-error-load')
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

function onDraftColor(state: WorkflowState, value: string) {
  const draft = drafts.value[state.id]
  if (!draft) return
  draft.color = value
  void saveDraft(state)
}

async function saveDraft(state: WorkflowState) {
  const draft = drafts.value[state.id]
  if (!draft) return
  const trimmed = draft.name.trim()
  if (!trimmed) {
    errorMessage.value = t('admin-workflow-states-error-name-required')
    return
  }
  const patch: UpdateWorkflowStateBody = {}
  if (trimmed !== state.name) patch.name = trimmed
  if (draft.color !== state.color) patch.color = draft.color
  if (Object.keys(patch).length === 0) return

  try {
    await workflowStatesService.update(state.id, patch)
    await reload()
    flash(t('admin-workflow-states-saved'))
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-workflow-states-error-save')
  }
}

async function promoteDefault(state: WorkflowState) {
  if (state.is_default) return
  try {
    await workflowStatesService.update(state.id, { is_default: true })
    await reload()
    flash(t('admin-workflow-states-default-flash', { name: state.name }))
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-workflow-states-error-default')
  }
}

/**
 * Toggle whether tickets sitting in this state pause the SLA clock.
 * The legacy rule (active category runs, every other category pauses)
 * is now per-state so an admin can hold a "Waiting on customer"
 * status under active without letting the timer keep counting.
 */
async function togglePause(state: WorkflowState) {
  try {
    await workflowStatesService.update(state.id, { pauses_sla: !state.pauses_sla })
    await reload()
    flash(
      t(
        !state.pauses_sla
          ? 'admin-workflow-states-sla-now-paused-flash'
          : 'admin-workflow-states-sla-now-running-flash',
        { name: state.name },
      ),
    )
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-workflow-states-error-save')
  }
}

const pendingArchive = ref<WorkflowState | null>(null)

function requestArchive(state: WorkflowState): void {
  if (state.is_default) {
    errorMessage.value = t('admin-workflow-states-error-promote-first')
    return
  }
  pendingArchive.value = state
}

async function confirmArchive(): Promise<void> {
  const state = pendingArchive.value
  if (!state) return
  pendingArchive.value = null
  try {
    await workflowStatesService.archive(state.id)
    await reload()
    flash(t('admin-workflow-states-archived-flash', { name: state.name }))
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-workflow-states-error-archive')
  }
}

async function createInCategory(category: WorkflowStateCategory) {
  const draft = newStateInputs.value[category]
  const trimmed = draft.name.trim()
  if (!trimmed) {
    errorMessage.value = t('admin-workflow-states-error-name-required')
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
    flash(t('admin-workflow-states-added-flash', { name: trimmed, category: getCategoryLabel(category) }))
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-workflow-states-error-create')
  }
}

onMounted(() => {
  reload()
})
</script>

<template>
  <div class="px-4 sm:px-6 py-6 max-w-4xl mx-auto w-full">
    <header class="mb-6">
      <h1 class="text-2xl font-semibold text-primary">{{ $t('admin-workflow-states-title') }}</h1>
      <p class="text-sm text-secondary mt-1">
        {{ $t('admin-workflow-states-description') }}
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
            {{ getCategoryLabel(cat) }}
          </h2>
          <span class="text-xs text-tertiary">
            {{ grouped[cat]?.length || 0 }} {{ (grouped[cat]?.length || 0) === 1 ? $t('admin-workflow-states-count-singular') : $t('admin-workflow-states-count-plural') }}
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
            <BaseDropdown
              v-if="drafts[state.id]"
              :model-value="drafts[state.id].color"
              :options="colorOptions"
              size="xs"
              class="w-32"
              @update:model-value="onDraftColor(state, String($event))"
            />
            <span
              v-if="state.is_default"
              class="text-[10px] uppercase tracking-wide font-semibold text-accent border border-accent/40 bg-accent/10 rounded px-1.5 py-0.5"
            >
              {{ $t('admin-workflow-states-default-badge') }}
            </span>
            <button
              v-else
              type="button"
              class="text-xs text-secondary hover:text-accent transition-colors"
              @click="promoteDefault(state)"
            >
              {{ $t('admin-workflow-states-make-default') }}
            </button>
            <button
              type="button"
              class="text-xs transition-colors"
              :class="state.pauses_sla ? 'text-status-warning hover:text-status-warning/80' : 'text-secondary hover:text-accent'"
              :aria-pressed="state.pauses_sla"
              :title="state.pauses_sla ? $t('admin-workflow-states-sla-paused-title') : $t('admin-workflow-states-sla-running-title')"
              @click="togglePause(state)"
            >
              {{ state.pauses_sla ? $t('admin-workflow-states-sla-paused') : $t('admin-workflow-states-sla-running') }}
            </button>
            <button
              type="button"
              class="text-tertiary hover:text-status-error transition-colors p-1"
              :disabled="state.is_default"
              :title="state.is_default ? $t('admin-workflow-states-archive-disabled-title') : $t('admin-workflow-states-archive-title')"
              @click="requestArchive(state)"
            >
              <Icon name="trash" />
            </button>
          </li>
          <li v-if="!grouped[cat] || grouped[cat].length === 0" class="px-4 py-3 text-sm text-tertiary italic">
            {{ $t('admin-workflow-states-empty-category') }}
          </li>
        </ul>

        <div class="flex flex-wrap items-center gap-2 px-4 py-3 bg-surface-alt border-t border-subtle">
          <input
            v-model="newStateInputs[cat].name"
            type="text"
            maxlength="64"
            :placeholder="$t('admin-workflow-states-add-placeholder')"
            class="flex-1 min-w-[150px] bg-surface border border-subtle rounded px-2 py-1 text-sm text-primary focus:border-accent focus:outline-none"
            @keydown.enter.prevent="createInCategory(cat)"
          />
          <BaseDropdown
            :model-value="newStateInputs[cat].color"
            :options="colorOptions"
            size="xs"
            class="w-32"
            @update:model-value="newStateInputs[cat].color = String($event)"
          />
          <button
            type="button"
            class="text-sm text-accent hover:underline"
            @click="createInCategory(cat)"
          >
            {{ $t('admin-workflow-states-add') }}
          </button>
        </div>
      </section>
    </div>

    <ConfirmModal
      :show="pendingArchive !== null"
      variant="warning"
      :title="$t('admin-workflow-states-archive-confirm-title')"
      :message="
        pendingArchive
          ? $t('admin-workflow-states-archive-confirm', { name: pendingArchive.name })
          : ''
      "
      :confirm-label="$t('admin-workflow-states-archive-confirm-label')"
      @confirm="confirmArchive"
      @close="pendingArchive = null"
    />
  </div>
</template>
