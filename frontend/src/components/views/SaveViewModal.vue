<script setup lang="ts">
/**
 * Name-prompt modal for "Save current view as…". Used by the
 * asset and user list views. Tickets has its own path (window
 * prompt) that we'll converge on this same modal in a later
 * commit once the saved-view UX rolls together.
 */
import { computed, ref, watch } from 'vue'
import Modal from '@/components/Modal.vue'
import Spinner from '@/components/common/Spinner.vue'

const props = defineProps<{
  show: boolean
  defaultName?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'save', name: string): Promise<boolean> | boolean
}>()

const name = ref('')
const saving = ref(false)

// Reset / prefill whenever the modal opens. Watching `show`
// directly avoids leaking stale state from the previous save.
watch(
  () => props.show,
  (open) => {
    if (open) {
      name.value = props.defaultName ?? ''
      saving.value = false
    }
  },
)

const trimmed = computed(() => name.value.trim())
const canSave = computed(() => trimmed.value.length > 0 && !saving.value)

async function save(): Promise<void> {
  if (!canSave.value) return
  saving.value = true
  try {
    const ok = await emit('save', trimmed.value)
    if (ok) emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Modal
    :show="show"
    :title="$t('views-save-as-title')"
    size="sm"
    @close="emit('close')"
  >
    <form class="flex flex-col gap-5" @submit.prevent="save">
      <div class="flex flex-col gap-1.5">
        <label
          for="save-view-name"
          class="text-xs font-medium text-secondary"
        >
          {{ $t('views-save-as-name-label') }}
        </label>
        <input
          id="save-view-name"
          v-model="name"
          type="text"
          maxlength="120"
          required
          autofocus
          class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-sm text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
        />
      </div>

      <div class="flex items-center justify-end gap-2 pt-2 border-t border-subtle">
        <button
          type="button"
          class="px-4 py-2 text-sm rounded-lg text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
          @click="emit('close')"
        >
          {{ $t('views-save-as-cancel') }}
        </button>
        <button
          type="submit"
          :disabled="!canSave"
          class="px-4 py-2 text-sm rounded-lg text-on-accent bg-accent hover:opacity-90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
        >
          <Spinner v-if="saving" size="sm" />
          <span>{{ saving ? $t('views-save-as-saving') : $t('views-save-as-save') }}</span>
        </button>
      </div>
    </form>
  </Modal>
</template>
