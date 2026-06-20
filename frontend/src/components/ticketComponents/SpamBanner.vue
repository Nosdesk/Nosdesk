<script setup lang="ts">
/**
 * Shown on a ticket the inbound mail filter flagged as possible spam. The
 * ticket is never dropped — it opens flagged + low-priority — so this banner
 * gives the agent the two triage actions: clear the flag (false positive) or
 * delete the ticket (real spam).
 *
 * "Not spam" clears the flag via a normal ticket update; the resulting pool
 * update removes this banner. "Delete" is emitted so the parent reuses its
 * existing delete-confirm flow.
 */
import { ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { updateTicket } from '@/services/ticketService';
import Icon from '@/components/common/Icon.vue';

const props = defineProps<{ ticketId: number }>();
const emit = defineEmits<{ delete: [] }>();

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const clearing = ref(false);
async function markNotSpam() {
  if (clearing.value) return;
  clearing.value = true;
  try {
    // Clearing the flag emits ticket.updated (spam_suspected: false); the pool
    // update removes this banner.
    await updateTicket(props.ticketId, { spam_suspected: false });
  } finally {
    clearing.value = false;
  }
}
</script>

<template>
  <div
    class="flex items-center gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm"
  >
    <Icon name="warning" size="sm" class="text-amber-600 dark:text-amber-400 flex-shrink-0" />
    <span class="flex-1 text-amber-800 dark:text-amber-200">{{ t('ticket-spam-banner-text') }}</span>
    <button
      type="button"
      class="px-2 py-1 rounded text-xs border border-default hover:bg-hover disabled:opacity-50"
      :disabled="clearing"
      @click="markNotSpam"
    >
      {{ t('ticket-spam-not-spam') }}
    </button>
    <button
      type="button"
      class="inline-flex items-center gap-1 px-2 py-1 rounded text-xs bg-status-error text-white hover:opacity-90"
      @click="emit('delete')"
    >
      <Icon name="trash" size="xs" />
      {{ t('ticket-spam-delete') }}
    </button>
  </div>
</template>
