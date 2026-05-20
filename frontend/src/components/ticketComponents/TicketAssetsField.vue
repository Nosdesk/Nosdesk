<script setup lang="ts">
/**
 * TicketDevicesField — property-list row for devices attached to
 * the ticket. Each device renders as a chip linking to the
 * device detail page; the trailing X detaches it.
 */
import { useFluent } from 'fluent-vue'
import type { Asset } from '@/types/asset'
import PropertyChipRow from '@/components/ticketComponents/PropertyChipRow.vue'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'

defineProps<{
  devices: Asset[]
}>()

const emit = defineEmits<{
  (e: 'add'): void
  (e: 'remove', deviceId: number): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

function deviceLabel(device: Asset): string {
  const hostname = device.attributes?.hostname as string | undefined
  return device.name || hostname || t('ticket-field-devices-fallback-name', { id: device.id })
}

function deviceTitle(device: Asset): string | undefined {
  const hostname = device.attributes?.hostname as string | undefined
  if (!hostname) return device.model || undefined
  if (device.model) {
    return t('ticket-field-devices-title-with-model', { hostname, model: device.model })
  }
  return hostname
}
</script>

<template>
  <PropertyChipRow
    :label="$t('ticket-field-devices-label')"
    :add-label="$t('ticket-field-devices-add')"
    @add="emit('add')"
  >
    <PropertyChip
      v-for="device in devices"
      :key="device.id"
      :label="deviceLabel(device)"
      :title="deviceTitle(device)"
      :to="`/assets/${device.id}`"
      removable
      :remove-title="$t('ticket-field-devices-detach')"
      @remove="emit('remove', device.id)"
    />
  </PropertyChipRow>
</template>
