<!-- Asks for the password of a protected PDF. -->
<script setup lang="ts">
import { nextTick, ref, watch, useId } from 'vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import PasswordInput from '@/components/common/PasswordInput.vue'
import type { PdfPasswordReason } from '@/utils/pdfjs'

const props = defineProps<{
  show: boolean
  filename: string
  reason: PdfPasswordReason
  /** The submitted password is being tried. */
  busy: boolean
}>()

const emit = defineEmits<{
  submit: [password: string]
  cancel: []
}>()

const formId = useId()
const inputId = useId()
const password = ref('')

// Start empty each time the prompt opens, and again after a wrong password.
watch(
  () => props.show,
  (show) => {
    if (show) password.value = ''
  },
)
watch(
  () => props.busy,
  async (busy) => {
    if (busy || !props.show) return
    password.value = ''
    // The field was disabled while busy, which dropped its focus.
    await nextTick()
    document.getElementById(inputId)?.focus()
  },
)

const submit = () => {
  if (password.value && !props.busy) emit('submit', password.value)
}
</script>

<template>
  <Modal
    :show="show"
    :title="$t('pdf-viewer-password-title')"
    :description="$t('pdf-viewer-password-description', { filename })"
    size="sm"
    @close="emit('cancel')"
  >
    <form :id="formId" class="flex flex-col gap-3" @submit.prevent="submit">
      <PasswordInput
        :id="inputId"
        v-model="password"
        :label="$t('pdf-viewer-password-label')"
        :error="reason === 'incorrect' ? $t('pdf-viewer-password-incorrect') : undefined"
        autocomplete="off"
        :disabled="busy"
      />
    </form>
    <template #footer>
      <div class="modal-actions flex justify-end gap-3">
        <Button variant="ghost" @click="emit('cancel')">
          {{ $t('pdf-viewer-password-cancel') }}
        </Button>
        <Button type="submit" :form="formId" :disabled="!password" :loading="busy">
          {{ $t('pdf-viewer-password-open') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
