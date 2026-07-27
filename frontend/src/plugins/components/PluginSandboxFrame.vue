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
import { createRemoteHostApi, postContext, postInit, watchPluginHeight } from '@nosdesk/plugin-sdk';
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
  /** Which manifest component this frame renders (a bundle may declare several). */
  component: { name: string; slot: string };
  ticket?: Ticket;
  device?: Asset;
  /** Monotonic counter from the host action menu; forwarded to the plugin. */
  actionActivated?: number;
}>();

const frameRef = ref<HTMLIFrameElement | null>(null);
const runtimeUrl = ref<string | null>(null);
const error = ref<string | null>(null);
const iframeHeight = ref<number | null>(null);

let bridge: HostBridge | null = null;
let unregisterInstance: (() => void) | null = null;
let stopHeight: (() => void) | null = null;
let connected = false;

// The bundle token lives ~60s; an iframe reload after expiry (bfcache / crash)
// re-fetches the bundle with a stale token and 403s. The runtime signals us, and
// we re-mint + reload — bounded so a genuinely broken bundle can't loop forever.
const MAX_BUNDLE_RETRIES = 3;
let bundleRetries = 0;

// The context is posted over postMessage (structured clone), so it must be a
// plain, clone-safe projection. The props are Vue reactive proxies and may carry
// non-cloneable fields; a JSON round-trip strips both the reactivity and anything
// structured clone would reject.
function plain<T>(value: T | undefined): T | null {
  return value == null ? null : (JSON.parse(JSON.stringify(value)) as T);
}
function snapshot(): PluginContext {
  return {
    ticket: plain(props.ticket),
    asset: plain(props.device),
    component: { name: props.component.name, slot: props.component.slot },
    actionActivated: props.actionActivated,
  };
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

  // Size the iframe to the plugin's reported content height.
  stopHeight?.();
  const iframe = frameRef.value;
  stopHeight = iframe
    ? watchPluginHeight(iframe, (px) => {
        iframeHeight.value = px;
        // A height report means the bundle loaded + mounted + rendered: the
        // token was fresh, so reset the refresh budget for the next expiry.
        bundleRetries = 0;
      })
    : null;
}

// Mint a fresh bundle token and (re)point the iframe at the runtime. Changing
// `runtimeUrl` swaps the `:src`, which reloads the frame with the new token.
async function mintToken(): Promise<void> {
  try {
    const { runtime_url } = await pluginService.getBundleToken(props.plugin.uuid);
    runtimeUrl.value = runtime_url;
  } catch (e) {
    logger.error('Failed to mint plugin bundle token', { plugin: props.plugin.name, error: e });
    error.value = 'Failed to load plugin';
  }
}

// The runtime posts this when its bundle fetch failed (usually an expired token
// on a reload). Re-mint + reload, bounded by the retry budget.
function onWindowMessage(event: MessageEvent): void {
  if (event.source !== frameRef.value?.contentWindow) return;
  if ((event.data as { type?: string } | undefined)?.type !== 'nosdesk-plugin-bundle-error') return;
  if (bundleRetries >= MAX_BUNDLE_RETRIES) {
    error.value = 'Failed to load plugin';
    return;
  }
  bundleRetries += 1;
  void mintToken();
}

onMounted(() => {
  window.addEventListener('message', onWindowMessage);
  void mintToken();
});

// Push a fresh context snapshot whenever ticket/device or the action counter
// changes (the action counter is how a host menu trigger reaches the plugin).
watch(
  [() => props.ticket, () => props.device, () => props.actionActivated],
  () => {
    const win = frameRef.value?.contentWindow;
    if (connected && win) postContext(win, snapshot());
  },
);

onBeforeUnmount(() => {
  window.removeEventListener('message', onWindowMessage);
  bridge?.dispose();
  bridge = null;
  unregisterInstance?.();
  unregisterInstance = null;
  stopHeight?.();
  stopHeight = null;
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
      :style="iframeHeight ? { height: `${iframeHeight}px` } : undefined"
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
  /* Height tracks the plugin's reported content height (inline style, set from
     the runtime's ResizeObserver over the bridge). min-height is the floor until
     the first report arrives. */
  min-height: 4rem;
}

@media print {
  .plugin-sandbox-frame {
    display: none;
  }
}
</style>
