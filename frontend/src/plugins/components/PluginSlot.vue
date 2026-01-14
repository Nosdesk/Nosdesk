<script setup lang="ts">
/**
 * Plugin Slot Component
 *
 * Renders plugin components registered for a specific slot.
 * Provides context (ticket, device, etc.) to plugin components.
 */
import { computed, provide } from 'vue';
import { slotRegistrations } from '../loader';
import type { PluginSlot as SlotType } from '@/types/plugin';
import type { Ticket } from '@/types/ticket';
import type { Device } from '@/types/device';
import PluginSlotItem from './PluginSlotItem.vue';

const props = defineProps<{
  slotName: SlotType;
  ticket?: Ticket;
  device?: Device;
}>();

// Get registrations for this slot
const registrations = computed(() => {
  return slotRegistrations.get(props.slotName) || [];
});

// Provide slot name for nested components
provide('pluginSlot', props.slotName);
</script>

<template>
  <PluginSlotItem
    v-for="reg in registrations"
    :key="`${reg.pluginUuid}-${reg.componentName}`"
    :registration="reg"
    :ticket="ticket"
    :device="device"
  />
</template>
