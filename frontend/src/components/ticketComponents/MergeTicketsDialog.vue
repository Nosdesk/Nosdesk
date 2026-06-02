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
import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import { useToastStore } from '@/stores/toast'
import { mergeTickets } from '@/services/ticketService'
import type { Ticket } from '@/types/ticket'

const props = defineProps<{
  open: boolean
  selectedTickets: Ticket[]
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

/** Oldest selected ticket by created timestamp (ISO strings sort
 *  lexicographically). The agent can override via the picker. */
function oldest(tickets: Ticket[]): Ticket | null {
  if (tickets.length === 0) return null
  return [...tickets].sort((a, b) => (a.created || '').localeCompare(b.created || ''))[0]
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
}

watch(
  () => props.open,
  (open) => {
    if (!open) return
    destinationId.value = oldest(props.selectedTickets)?.id ?? null
    reason.value = ''
    notifyCustomer.value = false
    submitting.value = false
    const snap: Record<number, number> = {}
    for (const t of props.selectedTickets) {
      if (t.workflow_state_id != null) snap[t.id] = t.workflow_state_id
    }
    stateSnapshot.value = snap
    seedDescription()
  },
  { immediate: true },
)

// Re-seed the description when the agent changes the destination.
watch(destinationId, () => {
  if (props.open) seedDescription()
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
      toast.warning(fluent.$t('ticket-merge-conflict-toast'))
    } else {
      toast.error(fluent.$t('ticket-merge-cancel-button'), String(err))
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
        <select
          v-model.number="destinationId"
          class="w-full px-3 py-2 text-sm rounded border border-default bg-surface focus:outline-none focus:border-accent"
        >
          <option v-for="t in selectedTickets" :key="t.id" :value="t.id">
            #{{ t.id }} {{ t.title }}
          </option>
        </select>
      </label>

      <!-- Source list (the non-destination selected tickets) -->
      <div class="flex flex-col gap-1">
        <ul class="flex flex-col gap-1">
          <li
            v-for="s in sources"
            :key="s.id"
            class="flex items-center gap-2 rounded border border-subtle bg-surface-alt px-3 py-2 text-sm"
          >
            <span class="text-tertiary">#{{ s.id }}</span>
            <span class="truncate">{{ s.title }}</span>
          </li>
        </ul>
      </div>

      <!-- Description preview (becomes the merge-marker comment body) -->
      <FormTextarea v-model="description" :rows="5" />

      <!-- Reason -->
      <label class="flex flex-col gap-1.5 text-sm">
        <span class="text-tertiary">{{ $t('ticket-merge-reason-label') }}</span>
        <input
          v-model="reason"
          type="text"
          :placeholder="$t('ticket-merge-reason-placeholder')"
          class="w-full px-3 py-2 text-sm rounded border border-default bg-surface focus:outline-none focus:border-accent"
        />
      </label>

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
