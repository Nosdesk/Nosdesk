<!--
Count-aware confirm dialog for destructive bulk actions.

Wraps the generic `ConfirmModal` with three things specific to
bulk operations:
 - Count + plural item label baked into the title and message.
 - "Type the action verb" affordance for high-blast-radius
   operations (Stripe / GitHub repo-deletion pattern); enabled by
   passing `requireConfirmText`. The Confirm button stays disabled
   until the typed text matches.
 - Defaults to the `danger` variant since that's what most bulk
   confirms are.

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
    /** Number of selected items the action will apply to. Used in
     *  the title and message ("Delete 14 tickets?"). */
    count: number
    /** Singular item label, e.g. `"ticket"`. Pluralised for counts. */
    itemLabel?: string
    /** The verb of the action, used in the title and the typed
     *  confirmation. Defaults to "delete". */
    actionVerb?: string
    /** Override the auto-generated message. */
    message?: string
    /** Override the title. */
    title?: string
    /** Confirm button label. */
    confirmLabel?: string
    /** Variant for the confirm button. Defaults to `danger`. */
    variant?: 'danger' | 'warning' | 'info'
    /** When set, requires the user to type this text before the
     *  Confirm button enables. The Stripe / GitHub destructive
     *  pattern, reserved for irreversible operations. */
    requireConfirmText?: string
  }>(),
  {
    itemLabel: 'item',
    actionVerb: 'delete',
    variant: 'danger',
  },
)

const emit = defineEmits<{
  confirm: []
  close: []
}>()

const pluralLabel = computed(() =>
  props.count === 1 ? props.itemLabel : `${props.itemLabel}s`,
)

const computedTitle = computed(() =>
  props.title ??
  `${capitalise(props.actionVerb)} ${props.count} ${pluralLabel.value}?`,
)

const computedMessage = computed(() =>
  props.message ??
  `This will ${props.actionVerb} ${props.count} ${pluralLabel.value}. This action cannot be undone.`,
)

const computedConfirmLabel = computed(() =>
  props.confirmLabel ??
  `${capitalise(props.actionVerb)} ${props.count} ${pluralLabel.value}`,
)

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

function capitalise(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1)
}

function handleConfirm() {
  if (!typedConfirmMatches.value) return
  emit('confirm')
}
</script>

<template>
  <ConfirmModal
    :show="show"
    :title="computedTitle"
    :message="computedMessage"
    :confirm-label="computedConfirmLabel"
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
        <span class="text-tertiary">
          Type <code class="px-1 py-0.5 rounded bg-surface-alt text-primary font-mono">{{ requireConfirmText }}</code> to confirm.
        </span>
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
