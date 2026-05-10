<script setup lang="ts">
/**
 * LinkedTicketChip — chip wrapper that resolves a ticket id to
 * its title for the property-list surface. While the title is
 * loading the chip shows `#{id}` as a placeholder so the chip
 * still occupies its slot and the row doesn't reflow.
 */
import { ref, watch } from 'vue'
import ticketService from '@/services/ticketService'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'

const props = defineProps<{
  ticketId: number
}>()

const emit = defineEmits<{
  (e: 'remove', id: number): void
}>()

const title = ref<string | null>(null)
const loading = ref(true)

watch(
  () => props.ticketId,
  async (id) => {
    loading.value = true
    title.value = null
    try {
      const fetched = await ticketService.getTicketById(id)
      title.value = fetched?.title ?? null
    } catch {
      title.value = null
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)
</script>

<template>
  <PropertyChip
    :label="title || `#${ticketId}`"
    :title="title ? `#${ticketId} · ${title}` : `Ticket #${ticketId}`"
    :to="`/tickets/${ticketId}`"
    :loading="loading"
    removable
    remove-title="Unlink ticket"
    @remove="emit('remove', ticketId)"
  >
    <template v-if="title" #leading>
      <span class="font-mono text-tertiary">#{{ ticketId }}</span>
    </template>
  </PropertyChip>
</template>
