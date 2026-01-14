<script setup lang="ts">
/**
 * Plugin Slot Item
 *
 * Renders a single plugin component in a slot.
 * Handles its own API creation and component loading.
 */
import { onErrorCaptured, ref, watchEffect } from 'vue';
import { getLoadedPlugin, type PluginSlotRegistration } from '../loader';
import { createPluginAPI } from '../api';
import { createPluginComponent, canRenderComponent } from '../componentLoader';
import { logger } from '@/utils/logger';
import type { Ticket } from '@/types/ticket';
import type { Device } from '@/types/device';

const props = defineProps<{
  registration: PluginSlotRegistration;
  ticket?: Ticket;
  device?: Device;
}>();

// Error state
const error = ref<string | null>(null);

onErrorCaptured((err) => {
  logger.error('Plugin error:', err);
  error.value = err instanceof Error ? err.message : String(err);
  return false;
});

// Check if this plugin can render a component (has bundle, trusted, etc.)
const canRender = canRenderComponent(props.registration.pluginUuid);

// Create async component once at setup (stable reference prevents re-fetching)
const asyncComponent = canRender
  ? createPluginComponent(props.registration.pluginUuid, props.registration.componentName)
  : null;

// Create plugin API once at setup
const loaded = getLoadedPlugin(props.registration.pluginUuid);
const api = loaded ? createPluginAPI(loaded.plugin) : null;

// Keep API context in sync with props
watchEffect(() => {
  if (api) {
    api._setContext({
      ticket: props.ticket || null,
      device: props.device || null,
    });
  }
});
</script>

<template>
  <div
    class="plugin-slot-item"
    :data-plugin="registration.pluginName"
    :data-component="registration.componentName"
  >
    <!-- Error state -->
    <div v-if="error" class="p-3 bg-red-500/10 border border-red-500/30 rounded text-sm text-red-400">
      <div class="font-medium">Plugin Error</div>
      <div class="text-xs mt-1">{{ error }}</div>
    </div>

    <!-- Render component (defineAsyncComponent handles loading/error internally) -->
    <component
      v-else-if="canRender && asyncComponent && api"
      :is="asyncComponent"
      :api="api"
      :context="api.context"
    />

    <!-- Placeholder for plugins without bundle -->
    <div
      v-else-if="api"
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

<style scoped>
.plugin-slot-item {
  contain: layout style;
}

.plugin-placeholder {
  transition: border-color 0.2s;
}

.plugin-placeholder:hover {
  border-color: var(--accent);
}
</style>
