<script setup lang="ts">
/**
 * Plugin Slot Component
 *
 * Renders plugin components registered for a specific slot.
 * Provides context (ticket, device, etc.) to plugin components.
 */
import { computed, provide } from 'vue';
import { slotRegistrations } from '../loader';
import type { PluginSlot as SlotType } from '@nosdesk/core/types/plugin';
import type { Ticket } from '@nosdesk/core/types/ticket';
import type { Asset } from '@nosdesk/core/types/asset';
import PluginSlotItem from './PluginSlotItem.vue';

const props = defineProps<{
  slotName: SlotType;
  ticket?: Ticket;
  device?: Asset;
  actionActivatedMap?: Map<string, number>;
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
    :slot-name="slotName"
    :ticket="ticket"
    :device="device"
    :actionActivated="actionActivatedMap?.get(`${reg.pluginUuid}:${reg.componentName}`)"
  />
</template>
