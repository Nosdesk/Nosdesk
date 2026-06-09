<!--
Styled confirmation dialog that replaces ad-hoc `window.confirm`
calls. Thin wrapper over the design-system `Modal.vue`; consumer
owns the `show` state and handles the `confirm` event.

Usage:
  <ConfirmModal
    :show="showConfirm"
    variant="danger"
    title="Remove stored password?"
    message="..."
    confirm-label="Remove"
    @confirm="doRemove"
    @close="showConfirm = false"
  />
-->
<template>
  <Modal :show="show" :title="title" size="sm" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <p class="text-sm text-secondary whitespace-pre-line leading-relaxed">{{ message }}</p>
      <slot name="body" />
    </div>

    <template #footer>
      <div class="modal-actions">
        <button
          type="button"
          class="text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
          @click="emit('close')"
        >
          {{ cancelLabel }}
        </button>
        <button
          type="button"
          :disabled="confirmDisabled || loading"
          :class="[
            'text-white transition-colors inline-flex items-center justify-center gap-2',
            confirmDisabled && !loading
              ? 'bg-surface-alt text-tertiary cursor-not-allowed'
              : variant === 'danger'
                ? 'bg-status-error hover:opacity-90'
                : variant === 'warning'
                  ? 'bg-status-warning hover:opacity-90'
                  : 'bg-accent hover:opacity-90',
          ]"
          @click="confirmDisabled || loading ? undefined : emit('confirm')"
        >
          <Spinner v-if="loading" size="xs" />
          {{ confirmLabel }}
        </button>
      </div>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import Modal from '@/components/Modal.vue'
import Spinner from '@/components/common/Spinner.vue'

withDefaults(
  defineProps<{
    show: boolean
    title: string
    message: string
    confirmLabel?: string
    cancelLabel?: string
    variant?: 'danger' | 'warning' | 'info'
    confirmDisabled?: boolean
    loading?: boolean
  }>(),
  {
    confirmLabel: 'Confirm',
    cancelLabel: 'Cancel',
    variant: 'info',
    confirmDisabled: false,
    loading: false,
  },
)

const emit = defineEmits<{
  (e: 'confirm'): void
  (e: 'close'): void
}>()
</script>
