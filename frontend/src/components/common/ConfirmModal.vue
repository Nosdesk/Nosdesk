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
        <Button variant="ghost" @click="emit('close')">{{ cancelLabel }}</Button>
        <Button
          :variant="confirmVariant"
          :disabled="confirmDisabled"
          :loading="loading"
          @click="emit('confirm')"
        >
          {{ confirmLabel }}
        </Button>
      </div>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'

const props = withDefaults(
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

// Map the semantic dialog variant onto the shared Button's variants.
// `info` is the neutral affirmative confirm, so it takes the accent
// primary (theme-aware on-accent foreground); danger / warning pass
// through. Button disables itself while `disabled` or `loading`, so a
// blocked confirm can't emit.
const confirmVariant = computed(() =>
  props.variant === 'danger' ? 'danger' : props.variant === 'warning' ? 'warning' : 'primary',
)

const emit = defineEmits<{
  (e: 'confirm'): void
  (e: 'close'): void
}>()
</script>
