<script setup lang="ts">
/**
 * Plugin Slot Item
 *
 * Renders one registered plugin component in a slot. Every plugin — every trust
 * tier — runs in the opaque-origin sandbox via PluginSandboxFrame; the in-process
 * Vue-component path was removed in the sandbox-all migration (4b-3c-4).
 */
import { onErrorCaptured, ref } from 'vue';
import { getLoadedPlugin, type PluginSlotRegistration } from '../loader';
import type { PluginSlotContext } from '../context';
import PluginSandboxFrame from './PluginSandboxFrame.vue';
import { logger } from '@nosdesk/core/utils/logger';

const props = defineProps<{
  registration: PluginSlotRegistration;
  slotName: string;
  context?: PluginSlotContext;
  actionActivated?: number;
}>();

const error = ref<string | null>(null);
onErrorCaptured((err) => {
  logger.error('Plugin error:', err);
  error.value = err instanceof Error ? err.message : String(err);
  return false;
});

const loaded = getLoadedPlugin(props.registration.pluginUuid);
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

    <!-- Opaque-origin sandbox iframe over the bridge (all tiers) -->
    <PluginSandboxFrame
      v-else-if="loaded"
      :plugin="loaded.plugin"
      :component="{ name: registration.componentName, slot: slotName }"
      :context="context"
      :actionActivated="actionActivated"
    />
  </div>
</template>

<style scoped>
.plugin-slot-item {
  contain: layout style;
}
</style>
