<script setup lang="ts">
/**
 * Plugin Sandbox Frame
 *
 * Renders a sandboxed (community-tier) plugin in an opaque-origin iframe. Mints
 * a bundle token, points the iframe at the runtime, and once it loads, exposes
 * the host API over a MessageChannel (transferred in with the init message).
 * Context snapshots are pushed on ticket/device change. The iframe carries only
 * `sandbox="allow-scripts"` (no `allow-same-origin`), so it is a null origin:
 * no cookies, no first-party DOM, all host access flows through the bridge.
 */
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { createRemoteHostApi, postContext, postInit } from '@nosdesk/plugin-sdk';
import type { HostBridge, PluginContext } from '@nosdesk/plugin-sdk';
import pluginService from '@nosdesk/core/services/pluginService';
import { logger } from '@nosdesk/core/utils/logger';
import type { Plugin } from '@nosdesk/core/types/plugin';
import type { Ticket } from '@nosdesk/core/types/ticket';
import type { Asset } from '@nosdesk/core/types/asset';
import { createHostApiImpl } from '../sandboxHostApi';
import { registerPluginInstance } from '../pluginInstances';

const props = defineProps<{
  plugin: Plugin;
  ticket?: Ticket;
  device?: Asset;
}>();

const frameRef = ref<HTMLIFrameElement | null>(null);
const runtimeUrl = ref<string | null>(null);
const error = ref<string | null>(null);

let bridge: HostBridge | null = null;
let unregisterInstance: (() => void) | null = null;
let connected = false;

// The context is posted over postMessage (structured clone), so it must be a
// plain, clone-safe projection. The props are Vue reactive proxies and may carry
// non-cloneable fields; a JSON round-trip strips both the reactivity and anything
// structured clone would reject.
function plain<T>(value: T | undefined): T | null {
  return value == null ? null : (JSON.parse(JSON.stringify(value)) as T);
}
function snapshot(): PluginContext {
  return { ticket: plain(props.ticket), device: plain(props.device) };
}

function onFrameLoad(): void {
  const win = frameRef.value?.contentWindow;
  if (!win) return;
  // A fresh bridge per load (the runtime reloads on token refresh / remount).
  bridge?.dispose();
  unregisterInstance?.();
  const { hostApi, inproc } = createHostApiImpl(props.plugin);
  // Register the backing instance so the event dispatcher can reach this
  // sandboxed plugin's `on` handlers (they land on `inproc`, over the bridge).
  unregisterInstance = registerPluginInstance(props.plugin.uuid, inproc);
  bridge = createRemoteHostApi(hostApi);
  postInit(win, bridge, snapshot());
  connected = true;
}

onMounted(async () => {
  try {
    const { runtime_url } = await pluginService.getBundleToken(props.plugin.uuid);
    runtimeUrl.value = runtime_url;
  } catch (e) {
    logger.error('Failed to mint plugin bundle token', { plugin: props.plugin.name, error: e });
    error.value = 'Failed to load plugin';
  }
});

// Push a fresh context snapshot whenever the slot's ticket/device changes.
watch(
  [() => props.ticket, () => props.device],
  () => {
    const win = frameRef.value?.contentWindow;
    if (connected && win) postContext(win, snapshot());
  },
);

onBeforeUnmount(() => {
  bridge?.dispose();
  bridge = null;
  unregisterInstance?.();
  unregisterInstance = null;
  connected = false;
});
</script>

<template>
  <div class="plugin-sandbox-frame" :data-plugin="plugin.name">
    <!-- Error state (hidden on print) -->
    <div
      v-if="error"
      class="print:hidden p-3 bg-red-500/10 border border-red-500/30 rounded text-sm text-red-400"
    >
      <div class="font-medium">Plugin Error</div>
      <div class="text-xs mt-1">{{ error }}</div>
    </div>

    <iframe
      v-else-if="runtimeUrl"
      ref="frameRef"
      :src="runtimeUrl"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      class="plugin-sandbox-iframe"
      @load="onFrameLoad"
    />
  </div>
</template>

<style scoped>
.plugin-sandbox-iframe {
  display: block;
  width: 100%;
  border: 0;
  background: transparent;
  /* v1: a fixed min-height. Auto-resize to content (a plugin-reported height
     over the bridge) is a follow-up; a cross-origin iframe can't self-size. */
  min-height: 8rem;
}

@media print {
  .plugin-sandbox-frame {
    display: none;
  }
}
</style>
