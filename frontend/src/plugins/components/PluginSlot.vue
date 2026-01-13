<script setup lang="ts">
/**
 * Plugin Slot Component
 *
 * Renders plugin components registered for a specific slot.
 * Provides context (ticket, device, etc.) to plugin components.
 */
import { computed, provide } from 'vue';
import { getSlotRegistrations, getLoadedPlugin, type PluginSlotRegistration } from '../loader';
import { createPluginAPI, type PluginAPI } from '../api';
import type { PluginSlot as SlotType } from '@/types/plugin';
import type { Ticket } from '@/types/ticket';
import type { Device } from '@/types/device';

const props = defineProps<{
  slot: SlotType;
  ticket?: Ticket;
  device?: Device;
}>();

// Get registrations for this slot
const registrations = computed(() => getSlotRegistrations(props.slot));

// Create Plugin API for each plugin and set context
const getPluginAPI = (registration: PluginSlotRegistration): PluginAPI | null => {
  const loaded = getLoadedPlugin(registration.pluginUuid);
  if (!loaded) return null;

  const api = createPluginAPI(loaded.plugin);

  // Set context based on what's provided
  api._setContext({
    ticket: props.ticket || null,
    device: props.device || null,
  });

  return api;
};

// Provide the slot for plugin components to access
provide('pluginSlot', props.slot);
</script>

<template>
  <template v-for="registration in registrations" :key="`${registration.pluginUuid}-${registration.componentName}`">
    <div
      class="plugin-slot-item"
      :data-plugin="registration.pluginName"
      :data-component="registration.componentName"
    >
      <!-- Plugin components will be rendered here by the plugin loader -->
      <!-- For Phase 2, we just show a placeholder indicating a plugin is registered -->
      <div
        v-if="getPluginAPI(registration)"
        class="plugin-placeholder p-3 bg-surface-alt rounded-lg border border-border text-sm"
      >
        <div class="flex items-center gap-2 text-secondary">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" />
          </svg>
          <span class="font-medium">{{ registration.label || registration.componentName }}</span>
          <span class="text-tertiary text-xs">({{ registration.pluginName }})</span>
        </div>
      </div>
    </div>
  </template>
</template>

<style scoped>
.plugin-slot-item {
  /* Ensure plugin components have proper isolation */
  contain: layout style;
}

.plugin-placeholder {
  transition: border-color 0.2s;
}

.plugin-placeholder:hover {
  border-color: var(--accent);
}
</style>
