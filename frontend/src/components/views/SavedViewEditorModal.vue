<!--
Edit a saved view. Replaces the earlier scattered window.prompt
(rename) + window.confirm (archive) UX with a single focused
modal — the user makes name edits and triggers delete from one
surface.

Delete is gated through ConfirmModal so the destructive path
needs a deliberate second click. Save and Delete are independent:
saving the name commits the rename without dismissing the modal;
delete dismisses on success and routes the parent to fall back to
the built-in default.
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import Modal from '@/components/Modal.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Spinner from '@/components/common/Spinner.vue'

/** The editor only reads `uuid` and `name`; widening to this
 *  minimal shape lets the modal accept any SavedView<S, F> from
 *  the dataset-generic savedViewsService without per-consumer
 *  casts. */
interface EditableView {
  uuid: string
  name: string
}

const props = defineProps<{
  /** When non-null the modal is open and editing the given view. */
  view: EditableView | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'rename', uuid: string, name: string): Promise<boolean> | boolean
  (e: 'delete', uuid: string): Promise<boolean> | boolean
}>()

const name = ref('')
const saving = ref(false)
const deleting = ref(false)
const showConfirmDelete = ref(false)

watch(
  () => props.view?.uuid,
  () => {
    name.value = props.view?.name ?? ''
    saving.value = false
    deleting.value = false
    showConfirmDelete.value = false
  },
  { immediate: true },
)

const trimmed = computed(() => name.value.trim())
const dirty = computed(() => trimmed.value !== (props.view?.name ?? ''))
const canSave = computed(() => dirty.value && trimmed.value.length > 0 && !saving.value)

async function save(): Promise<void> {
  if (!props.view || !canSave.value) return
  saving.value = true
  try {
    const ok = await emit('rename', props.view.uuid, trimmed.value)
    if (ok) emit('close')
  } finally {
    saving.value = false
  }
}

async function confirmDelete(): Promise<void> {
  if (!props.view) return
  deleting.value = true
  try {
    const ok = await emit('delete', props.view.uuid)
    if (ok) {
      showConfirmDelete.value = false
      emit('close')
    }
  } finally {
    deleting.value = false
  }
}
</script>

<template>
  <Modal
    :show="view !== null"
    :title="$t('views-saved-editor-title')"
    size="sm"
    @close="emit('close')"
  >
    <form
      v-if="view"
      class="flex flex-col gap-5"
      @submit.prevent="save"
    >
      <div class="flex flex-col gap-1.5">
        <label
          for="saved-view-name"
          class="text-xs font-medium text-secondary"
        >
          {{ $t('views-saved-editor-name-label') }}
        </label>
        <input
          id="saved-view-name"
          v-model="name"
          type="text"
          maxlength="120"
          required
          autofocus
          class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-sm text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
        />
      </div>

      <div class="flex items-center justify-between gap-2 pt-2 border-t border-subtle">
        <button
          type="button"
          :disabled="deleting"
          class="px-3 py-2 text-sm rounded-lg text-status-error hover:bg-status-error/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          @click="showConfirmDelete = true"
        >
          {{ $t('views-saved-editor-delete') }}
        </button>
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="px-4 py-2 text-sm rounded-lg text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
            @click="emit('close')"
          >
            {{ $t('views-saved-editor-cancel') }}
          </button>
          <button
            type="submit"
            :disabled="!canSave"
            class="px-4 py-2 text-sm rounded-lg text-on-accent bg-accent hover:opacity-90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
          >
            <Spinner v-if="saving" size="sm" />
            <span>{{ saving ? $t('views-saved-editor-saving') : $t('views-saved-editor-save') }}</span>
          </button>
        </div>
      </div>
    </form>
  </Modal>

  <ConfirmModal
    :show="showConfirmDelete"
    variant="danger"
    :title="$t('views-saved-editor-confirm-title')"
    :message="view ? $t('views-saved-editor-confirm-message', { name: view.name }) : ''"
    :confirm-label="$t('views-saved-editor-delete')"
    @confirm="confirmDelete"
    @close="showConfirmDelete = false"
  />
</template>
