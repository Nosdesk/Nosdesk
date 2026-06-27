<!--
Merge dialog for the bulk "Merge" action. Phase 1 is a single-agent
form (the Yjs co-edit upgrade is Phase 2). The agent picks a
destination, optionally edits the merge-marker comment body and a
reason, and chooses whether to notify the customer. On open it snapshots
each ticket's workflow_state_id as an optimistic-lock token; a 409 means
a ticket changed underneath and the agent is asked to refresh.
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import FormInput from '@/components/common/FormInput.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import { useToastStore } from '@/stores/toast'
import { mergeTickets } from '@nosdesk/core/services/ticketService'

/** Minimal ticket shape the dialog needs. Both the API `Ticket` and the
 *  sync store's `SyncTicket` satisfy it, so the bulk bar can pass either.
 *  `created` is optional; when absent the oldest-default falls back to
 *  the lowest id (ids are monotonic, so the smallest is the oldest). */
export interface MergeDialogTicket {
  id: number
  title: string
  workflow_state_id?: number
  created?: string
}

const props = defineProps<{
  open: boolean
  selectedTickets: MergeDialogTicket[]
}>()

const emit = defineEmits<{
  close: []
  /** Emitted with the destination id after a successful merge so the
   *  parent can clear its selection. */
  merged: [destinationId: number]
}>()

const fluent = useFluent()
const toast = useToastStore()
const router = useRouter()

const destinationId = ref<number | null>(null)
const description = ref('')
// The last auto-generated seed, so we only reseed on a destination
// change while the agent hasn't edited the buffer.
const lastSeed = ref('')
const reason = ref('')
const notifyCustomer = ref(false)
const submitting = ref(false)
// ticket_id -> workflow_state_id snapshot taken when the dialog opened.
const stateSnapshot = ref<Record<number, number>>({})

const count = computed(() => props.selectedTickets.length)
const canSubmit = computed(() => count.value >= 2 && destinationId.value !== null)

const sources = computed(() =>
  props.selectedTickets.filter((t) => t.id !== destinationId.value),
)

// BaseDropdown is string-valued; bridge the numeric destination id.
const destinationModel = computed<string>({
  get: () => (destinationId.value == null ? '' : String(destinationId.value)),
  set: (v) => {
    destinationId.value = v === '' ? null : Number(v)
  },
})
const destinationOptions = computed(() =>
  props.selectedTickets.map((t) => ({ value: String(t.id), label: `#${t.id} ${t.title}` })),
)

/** Oldest selected ticket: by created timestamp when present (ISO
 *  strings sort lexicographically), else by lowest id. The agent can
 *  override via the picker. */
function oldest(tickets: MergeDialogTicket[]): MergeDialogTicket | null {
  if (tickets.length === 0) return null
  return [...tickets].sort((a, b) => {
    if (a.created && b.created) return a.created.localeCompare(b.created)
    return a.id - b.id
  })[0]
}

function seedDescription() {
  const dest = props.selectedTickets.find((t) => t.id === destinationId.value)
  const lines: string[] = []
  if (dest) lines.push(dest.title)
  lines.push('')
  lines.push('Incoming from:')
  for (const s of sources.value) {
    lines.push(`- #${s.id}: ${s.title}`)
  }
  description.value = lines.join('\n')
  lastSeed.value = description.value
}

// Snapshot each ticket's workflow_state_id as the optimistic-lock
// token. Re-taken on a 409 so an immediate retry uses fresh tokens.
function snapshotStates() {
  const snap: Record<number, number> = {}
  for (const t of props.selectedTickets) {
    if (t.workflow_state_id != null) snap[t.id] = t.workflow_state_id
  }
  stateSnapshot.value = snap
}

watch(
  () => props.open,
  (open) => {
    if (!open) return
    destinationId.value = oldest(props.selectedTickets)?.id ?? null
    reason.value = ''
    notifyCustomer.value = false
    submitting.value = false
    snapshotStates()
    seedDescription()
  },
  { immediate: true },
)

// Re-seed when the destination changes, but only if the agent hasn't
// edited the buffer (so we never clobber their wording).
watch(destinationId, () => {
  if (props.open && description.value === lastSeed.value) seedDescription()
})

async function submit() {
  if (!canSubmit.value || submitting.value || destinationId.value === null) return
  submitting.value = true
  const target = destinationId.value
  try {
    await mergeTickets({
      destination_ticket_id: target,
      source_ticket_ids: sources.value.map((t) => t.id),
      reason: reason.value.trim() || null,
      notify_customer: notifyCustomer.value,
      marker_body: description.value.trim() || null,
      expected_state: Object.entries(stateSnapshot.value).map(([ticket_id, workflow_state_id]) => ({
        ticket_id: Number(ticket_id),
        workflow_state_id,
      })),
    })
    toast.success(
      fluent.$t('ticket-merge-success-toast', { count: count.value, target_id: target }),
    )
    emit('merged', target)
    emit('close')
    router.push(`/tickets/${target}`)
  } catch (err: unknown) {
    const status = (err as { response?: { status?: number } })?.response?.status
    if (status === 409) {
      // Stale optimistic-lock token. Refresh the snapshot so the agent
      // can retry immediately without reopening the dialog.
      snapshotStates()
      toast.warning(fluent.$t('ticket-merge-conflict-toast'))
    } else {
      toast.error(fluent.$t('ticket-merge-error-toast'))
    }
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <Modal
    :show="open"
    :title="$t('ticket-merge-dialog-title', { count })"
    size="lg"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-4">
      <!-- Destination picker -->
      <label class="flex flex-col gap-1.5 text-sm">
        <span class="text-tertiary">{{ $t('ticket-merge-destination-label') }}</span>
        <BaseDropdown
          :model-value="destinationModel"
          :options="destinationOptions"
          size="sm"
          @update:model-value="destinationModel = String($event)"
        />
      </label>

      <!-- Source list (the non-destination selected tickets) -->
      <ul class="flex flex-col gap-1" :aria-label="$t('ticket-merge-sidebar-merged-in')">
        <li
          v-for="s in sources"
          :key="s.id"
          class="flex items-center gap-2 rounded border border-subtle bg-surface-alt px-3 py-2 text-sm"
        >
          <span class="text-tertiary">#{{ s.id }}</span>
          <span class="truncate">{{ s.title }}</span>
        </li>
      </ul>

      <!-- Description preview (becomes the merge-marker comment body) -->
      <FormTextarea
        v-model="description"
        :label="$t('ticket-merge-marker-comment-header', { count })"
        :rows="5"
      />

      <!-- Reason -->
      <FormInput
        v-model="reason"
        :label="$t('ticket-merge-reason-label')"
        :placeholder="$t('ticket-merge-reason-placeholder')"
      />

      <!-- Customer notification -->
      <div class="flex flex-col gap-1">
        <Checkbox v-model="notifyCustomer" :label="$t('ticket-merge-notify-customer-label')" />
        <span class="text-xs text-tertiary">{{ $t('ticket-merge-notify-customer-help') }}</span>
      </div>
    </div>

    <template #footer>
      <div class="flex justify-end gap-2">
        <Button variant="secondary" @click="emit('close')">
          {{ $t('ticket-merge-cancel-button') }}
        </Button>
        <Button variant="primary" :disabled="!canSubmit" :loading="submitting" @click="submit">
          {{ $t('ticket-merge-submit-button', { count }) }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
