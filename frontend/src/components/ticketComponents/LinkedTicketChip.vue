<script setup lang="ts">
/**
 * LinkedTicketChip — chip wrapper that resolves a ticket id to
 * its title for the property-list surface. While the title is
 * loading the chip shows `#{id}` as a placeholder so the chip
 * still occupies its slot and the row doesn't reflow.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import ticketService from '@nosdesk/core/services/ticketService'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'

const props = defineProps<{
  ticketId: number
}>()

const emit = defineEmits<{
  (e: 'remove', id: number): void
}>()

const title = ref<string | null>(null)
const loading = ref(true)

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const chipLabel = computed(() => title.value || `#${props.ticketId}`)
const chipTooltip = computed(() => title.value
  ? t('ticket-chip-linked-ticket-title', { id: props.ticketId, title: title.value })
  : t('ticket-chip-linked-ticket-fallback', { id: props.ticketId }))
const unlinkTitle = computed(() => t('ticket-chip-unlink-ticket'))

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
    :label="chipLabel"
    :title="chipTooltip"
    :to="`/tickets/${ticketId}`"
    :loading="loading"
    removable
    :remove-title="unlinkTitle"
    @remove="emit('remove', ticketId)"
  >
    <template v-if="title" #leading>
      <span class="font-mono text-tertiary">#{{ ticketId }}</span>
    </template>
  </PropertyChip>
</template>
