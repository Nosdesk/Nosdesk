<script setup lang="ts">
/**
 * Create-rollout handoff: turn a selected group of devices into a
 * project with one ticket per device, each ticket linked to its asset.
 * This is the planner-to-projects bridge. The modal only gathers the
 * project name + the ticket defaults (initial workflow state, priority);
 * the server does the rest in one transaction.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRouter } from 'vue-router'

import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue'
import AlertMessage from '@/components/common/AlertMessage.vue'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import { createAssetRollout } from '@/services/assetService'
import { extractErrorMessage } from '@/utils/errors'
import { useToastStore } from '@nosdesk/core/stores/toast'

const props = defineProps<{
  show: boolean
  /** Exact device ids the rollout will cover. */
  assetIds: number[]
  /** Suggested project name (e.g. the active bucket's label). */
  defaultName?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'created'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const router = useRouter()
const toast = useToastStore()
const workflowStates = useWorkflowStatesStore()

const name = ref('')
const workflowStateId = ref<number | null>(null)
const priority = ref('medium')
const saving = ref(false)
const error = ref<string | null>(null)

const deviceCount = computed(() => props.assetIds.length)

const workflowStateOptions = computed<DropdownOption[]>(() =>
  workflowStates.states
    .filter((s) => !s.archived_at)
    .map((s) => ({ value: String(s.id), label: s.name })),
)

const priorityOptions = computed<DropdownOption[]>(() => [
  { value: 'none', label: t('priority-none') },
  { value: 'low', label: t('priority-low') },
  { value: 'medium', label: t('priority-medium') },
  { value: 'high', label: t('priority-high') },
  { value: 'urgent', label: t('priority-urgent') },
])

const canSave = computed(
  () => name.value.trim() !== '' && workflowStateId.value != null && deviceCount.value > 0,
)

// Reset the form each time the modal opens, seeding the name from the
// active bucket and the workflow state from the workspace default.
watch(
  () => props.show,
  async (show) => {
    if (!show) return
    error.value = null
    name.value = props.defaultName?.trim() || ''
    priority.value = 'medium'
    await workflowStates.load()
    workflowStateId.value =
      workflowStates.defaultState?.id ?? workflowStates.states[0]?.id ?? null
  },
  { immediate: true },
)

async function submit() {
  if (!canSave.value || workflowStateId.value == null) return
  saving.value = true
  error.value = null
  try {
    const result = await createAssetRollout({
      name: name.value.trim(),
      workflow_state_id: workflowStateId.value,
      priority: priority.value,
      asset_ids: props.assetIds,
    })
    toast.success(t('asset-rollout-created', { count: result.ticket_count }))
    emit('created')
    await router.push(`/projects/${result.project_id}`)
  } catch (e) {
    error.value = extractErrorMessage(e, t('asset-rollout-create-failed'))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Modal :show="show" :title="$t('asset-rollout-title')" size="md" @close="emit('close')">
    <div class="flex flex-col gap-3">
      <p class="text-sm text-secondary">
        {{ $t('asset-rollout-summary', { count: deviceCount }) }}
      </p>

      <FormInput
        v-model="name"
        :label="$t('asset-rollout-name-label')"
        :placeholder="$t('asset-rollout-name-placeholder')"
        size="sm"
      />

      <div class="flex flex-col gap-1.5">
        <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
          {{ $t('asset-rollout-state-label') }}
        </label>
        <SearchableDropdown
          :model-value="workflowStateId != null ? String(workflowStateId) : ''"
          :options="workflowStateOptions"
          size="sm"
          @update:model-value="(v) => (workflowStateId = v ? Number(v) : null)"
        />
      </div>

      <div class="flex flex-col gap-1.5">
        <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
          {{ $t('asset-rollout-priority-label') }}
        </label>
        <SearchableDropdown
          :model-value="priority"
          :options="priorityOptions"
          size="sm"
          @update:model-value="(v) => (priority = String(v))"
        />
      </div>

      <AlertMessage v-if="error" type="error" :message="error" />

      <div class="flex justify-end gap-2">
        <Button variant="secondary" :disabled="saving" @click="emit('close')">
          {{ $t('common-cancel') }}
        </Button>
        <Button :disabled="!canSave || saving" :loading="saving" @click="submit">
          {{ $t('asset-rollout-create-action', { count: deviceCount }) }}
        </Button>
      </div>
    </div>
  </Modal>
</template>
