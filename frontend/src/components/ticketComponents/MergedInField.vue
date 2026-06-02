<!--
Destination-side cross-reference: lists the source tickets merged into
this one, under a "Merged in" heading. Sourced from the merge-history
endpoint (the ticket's linked_tickets only carry ids, not relation
types). Cached per ticket via Pinia Colada, so it renders instantly from
cache on revisit and revalidates in the background. Renders nothing when
no merges target this ticket.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@pinia/colada'
import { fetchMergeHistory } from '@/services/ticketService'

const props = defineProps<{ ticketId: number }>()

const { data } = useQuery({
  key: () => ['merge-history', props.ticketId],
  query: () => fetchMergeHistory(props.ticketId),
})

// Unique source ids across every merge that targeted this ticket.
const sourceIds = computed<number[]>(() => {
  const events = data.value?.merge_events ?? []
  return [...new Set(events.flatMap((ev) => ev.source_ticket_ids))]
})
</script>

<template>
  <div v-if="sourceIds.length > 0" class="flex flex-col gap-1.5">
    <span class="text-xs font-semibold text-secondary">{{ $t('ticket-merge-sidebar-merged-in') }}</span>
    <div class="flex flex-wrap gap-1.5">
      <RouterLink
        v-for="id in sourceIds"
        :key="id"
        :to="`/tickets/${id}`"
        class="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-medium border border-subtle bg-surface-alt text-secondary hover:text-primary hover:border-default transition-colors"
      >
        #{{ id }}
      </RouterLink>
    </div>
  </div>
</template>
