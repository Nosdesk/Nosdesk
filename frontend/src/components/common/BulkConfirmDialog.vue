<!--
Confirm dialog for destructive bulk actions.

Wraps the generic `ConfirmModal` with one bulk-specific affordance:

  "Type the action verb" gate for high-blast-radius operations
  (Stripe / GitHub repo-deletion pattern). The Confirm button stays
  disabled until the typed text matches `requireConfirmText`.

Title, message, and confirm label are required props rather than
derived from `count`/`itemLabel`/`actionVerb` defaults. Building
prose from string concatenation forced English plurals
(`${count} ${itemLabel}s`) on every consumer; pushing the
sentences up to the caller lets each one resolve through Fluent
with proper selectors. Defaults to the `danger` variant since
that's what most bulk confirms are.

Used by views that want a modal for irreversible bulk actions; for
reversible ones (Linear / Asana / Gmail style), prefer
`optimisticBulkAction` instead, which uses an Undo toast.
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

const props = withDefaults(
  defineProps<{
    show: boolean
    /** Localized title, e.g. `$t('user-mgmt-bulk-delete-title', { count })`. */
    title: string
    /** Localized message body. */
    message: string
    /** Localized confirm-button label. */
    confirmLabel: string
    /** Variant for the confirm button. Defaults to `danger`. */
    variant?: 'danger' | 'warning' | 'info'
    /** When set, requires the user to type this text before the
     *  Confirm button enables. The Stripe / GitHub destructive
     *  pattern, reserved for irreversible operations. */
    requireConfirmText?: string
    /** Localized prompt for the typed-confirmation input. Required
     *  when `requireConfirmText` is set. The bound input expects
     *  the word the user must type to land inside this string in
     *  the locale's natural sentence order. */
    typeToConfirmLabel?: string
  }>(),
  {
    variant: 'danger',
  },
)

const emit = defineEmits<{
  confirm: []
  close: []
}>()

// Typed-confirmation state. Reset every time the dialog re-opens so
// stale input from a previous open doesn't leak.
const typedConfirm = ref('')
watch(
  () => props.show,
  (open) => {
    if (open) typedConfirm.value = ''
  },
)

const typedConfirmMatches = computed(() =>
  !props.requireConfirmText ||
  typedConfirm.value.trim() === props.requireConfirmText.trim(),
)

function handleConfirm() {
  if (!typedConfirmMatches.value) return
  emit('confirm')
}
</script>

<template>
  <ConfirmModal
    :show="show"
    :title="title"
    :message="message"
    :confirm-label="confirmLabel"
    :variant="variant"
    :confirm-disabled="!typedConfirmMatches"
    @confirm="handleConfirm"
    @close="emit('close')"
  >
    <!-- Typed-confirmation input. ConfirmModal's body slot, when it
         exists, renders below the message. If ConfirmModal doesn't
         expose a body slot the input simply doesn't render and the
         button stays bound to its enable state. -->
    <template v-if="requireConfirmText" #body>
      <label class="flex flex-col gap-1.5 text-xs">
        <span class="text-tertiary">{{ typeToConfirmLabel }}</span>
        <input
          v-model="typedConfirm"
          type="text"
          autocomplete="off"
          spellcheck="false"
          class="w-full px-3 py-2 text-sm rounded border border-default bg-surface focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
        />
      </label>
    </template>
  </ConfirmModal>
</template>
