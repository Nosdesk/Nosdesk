<!--
Destination-side cross-reference: lists the source tickets merged into
this one, under a "Merged in" heading. Sourced from the merge-history
endpoint (the ticket's linked_tickets only carry ids, not relation
types). Renders nothing when no merges target this ticket.
-->
<script setup lang="ts">
import { ref, watch } from 'vue'
import { fetchMergeHistory } from '@/services/ticketService'

const props = defineProps<{ ticketId: number }>()

const sourceIds = ref<number[]>([])

async function load() {
  try {
    const history = await fetchMergeHistory(props.ticketId)
    const ids = new Set<number>()
    for (const ev of history.merge_events) {
      for (const id of ev.source_ticket_ids) ids.add(id)
    }
    sourceIds.value = [...ids]
  } catch {
    sourceIds.value = []
  }
}

watch(() => props.ticketId, load, { immediate: true })
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
