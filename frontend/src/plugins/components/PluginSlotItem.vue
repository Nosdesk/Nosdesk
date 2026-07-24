<script setup lang="ts">
/**
 * Plugin Slot Item
 *
 * Renders a single plugin component in a slot.
 * Handles its own API creation and component loading.
 * Bundles are preloaded during plugin init so components render instantly.
 */
import { onErrorCaptured, onUnmounted, ref, watchEffect } from 'vue';
import { getLoadedPlugin, type PluginSlotRegistration } from '../loader';
import { getHostApiForPlugin } from '../api';
import { createPluginComponent, canRenderComponent } from '../componentLoader';
import { isSandboxed } from '../sandboxHostApi';
import { registerPluginInstance } from '../pluginInstances';
import PluginSandboxFrame from './PluginSandboxFrame.vue';
import { logger } from '@nosdesk/core/utils/logger';
import type { Ticket } from '@nosdesk/core/types/ticket';
import type { Asset } from '@nosdesk/core/types/asset';

const props = defineProps<{
  registration: PluginSlotRegistration;
  slotName: string;
  ticket?: Ticket;
  device?: Asset;
  actionActivated?: number;
}>();

// Error state
const error = ref<string | null>(null);

onErrorCaptured((err) => {
  logger.error('Plugin error:', err);
  error.value = err instanceof Error ? err.message : String(err);
  return false;
});

// Sandboxed (community-tier) plugins render in an iframe via PluginSandboxFrame,
// not the in-process Vue-component path. Resolve the tier first so we don't build
// an in-process API/component for them.
const loaded = getLoadedPlugin(props.registration.pluginUuid);
const sandboxed = loaded ? isSandboxed(loaded.plugin) : false;

// Check if this plugin can render a component in-process (has bundle, trusted).
const canRender = !sandboxed && canRenderComponent(props.registration.pluginUuid);

// Create async component once at setup (stable reference prevents re-fetching)
const asyncComponent = canRender
  ? createPluginComponent(props.registration.pluginUuid, props.registration.componentName)
  : null;

// Create the in-process plugin API once at setup (sandboxed plugins get theirs
// inside the frame, over the bridge).
const api = !sandboxed && loaded ? getHostApiForPlugin(loaded.plugin) : null;

// Register this instance so the event dispatcher reaches its `on` handlers; drop
// it on unmount. (Sandboxed plugins register their own backing instance in the
// frame.)
if (api && loaded) {
  onUnmounted(registerPluginInstance(loaded.plugin.uuid, api));
}

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
    <!-- Error state (hidden on print) -->
    <div v-if="error" class="print:hidden p-3 bg-red-500/10 border border-red-500/30 rounded text-sm text-red-400">
      <div class="font-medium">Plugin Error</div>
      <div class="text-xs mt-1">{{ error }}</div>
    </div>

    <!-- Sandboxed (community-tier) plugin: opaque-origin iframe over the bridge -->
    <PluginSandboxFrame
      v-else-if="sandboxed && loaded"
      :plugin="loaded.plugin"
      :component="{ name: registration.componentName, slot: slotName }"
      :ticket="ticket"
      :device="device"
      :actionActivated="actionActivated"
    />

    <!-- Render component (bundle is preloaded so this resolves instantly) -->
    <component
      v-else-if="canRender && asyncComponent && api"
      :is="asyncComponent"
      :api="api"
      :context="api.context"
      :actionActivated="actionActivated"
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

/* Hide plugin placeholders on print - errors already have print:hidden class */
@media print {
  .plugin-placeholder {
    display: none !important;
  }
}
</style>
