<script setup lang="ts">
/**
 * LinkedTicketChip — chip wrapper that resolves a ticket id to
 * its number and title for the property-list surface. While they
 * load the chip shows the pooled number (if any) as a placeholder so
 * the chip still occupies its slot and the row doesn't reflow.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import ticketService from '@nosdesk/core/services/ticketService'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'
import { pooledTicketNumber, ticketNumber, ticketPath } from '@/utils/ticketNumbers'

const props = defineProps<{
  ticketId: number
}>()

const emit = defineEmits<{
  (e: 'remove', id: number): void
}>()

const title = ref<string | null>(null)
const number = ref<number | undefined>(pooledTicketNumber(props.ticketId))
const loading = ref(true)

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const chipLabel = computed(() => title.value || (number.value !== undefined ? `#${number.value}` : ''))
const chipTooltip = computed(() => title.value
  ? t('ticket-chip-linked-ticket-title', { id: number.value ?? '', title: title.value })
  : t('ticket-chip-linked-ticket-fallback', { id: number.value ?? '' }))
const unlinkTitle = computed(() => t('ticket-chip-unlink-ticket'))

watch(
  () => props.ticketId,
  async (id) => {
    loading.value = true
    title.value = null
    number.value = pooledTicketNumber(id)
    try {
      const fetched = await ticketService.getTicketById(id)
      title.value = fetched?.title ?? null
      if (fetched) number.value = ticketNumber(fetched)
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
    :to="ticketPath({ id: ticketId, number })"
    :loading="loading"
    removable
    :remove-title="unlinkTitle"
    @remove="emit('remove', ticketId)"
  >
    <template v-if="title && number !== undefined" #leading>
      <span class="font-mono text-tertiary">#{{ number }}</span>
    </template>
  </PropertyChip>
</template>
