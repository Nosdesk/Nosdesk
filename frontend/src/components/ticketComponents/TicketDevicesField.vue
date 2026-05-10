<script setup lang="ts">
/**
 * TicketDevicesField — property-list row for devices attached to
 * the ticket. Each device renders as a chip linking to the
 * device detail page; the trailing X detaches it.
 */
import type { Device } from '@/types/device'
import PropertyChipRow from '@/components/ticketComponents/PropertyChipRow.vue'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'

defineProps<{
  devices: Device[]
}>()

const emit = defineEmits<{
  (e: 'add'): void
  (e: 'remove', deviceId: number): void
}>()
</script>

<template>
  <PropertyChipRow
    label="Devices"
    add-label="Add device"
    @add="emit('add')"
  >
    <PropertyChip
      v-for="device in devices"
      :key="device.id"
      :label="device.hostname || `Device #${device.id}`"
      :title="device.model ? `${device.hostname} · ${device.model}` : device.hostname"
      :to="`/devices/${device.id}`"
      removable
      remove-title="Detach device"
      @remove="emit('remove', device.id)"
    />
  </PropertyChipRow>
</template>
