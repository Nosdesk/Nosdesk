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
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  createRemoteHostApi,
  postContext,
  postInit,
  postTheme,
  watchPluginHeight,
} from '@nosdesk/plugin-sdk';
import type { HostBridge, PluginContext, PluginTheme, PluginAddress } from '@nosdesk/plugin-sdk';
import pluginService from '@nosdesk/core/services/pluginService';
import { logger } from '@nosdesk/core/utils/logger';
import type { Plugin } from '@nosdesk/core/types/plugin';
import type { UserAddress } from '@nosdesk/core/services/userContactService';
import { useThemeStore } from '@/stores/theme';
import type { PluginSlotContext } from '../context';
import { snapshotPluginTheme } from '../theme';
import { usePluginLayout } from '../layout';
import { createHostApiImpl } from '../sandboxHostApi';
import { registerPluginInstance } from '../pluginInstances';

const props = defineProps<{
  plugin: Plugin;
  /** Which manifest component this frame renders (a bundle may declare several). */
  component: { name: string; slot: string };
  /** Host-provided context bag; the snapshot projects its fields onto the wire. */
  context?: PluginSlotContext;
  /** Monotonic counter from the host action menu; forwarded to the plugin. */
  actionActivated?: number;
}>();

const emit = defineEmits<{
  /** Latest content height the plugin reported. `0` means it rendered nothing,
   *  which lets a parent drop its chrome; `null` means nothing reported yet.
   *  Parents must keep this component MOUNTED and LAID OUT when collapsing
   *  (zero height and clipping, never `v-if` or `display: none`), or the guest
   *  stops being able to measure itself and a plugin that fills in later can
   *  never report its way back. */
  (e: 'contentHeight', px: number | null): void;
}>();

const frameRef = ref<HTMLIFrameElement | null>(null);
const runtimeUrl = ref<string | null>(null);
const error = ref<string | null>(null);
const iframeHeight = ref<number | null>(null);

watch(iframeHeight, (px) => emit('contentHeight', px));

/** Every report is a size, so it pins directly: `0` collapses the frame, which
 *  is exactly what the parent wants when the plugin drew nothing. `null` means
 *  nothing has been reported yet, so the iframe keeps its natural height until
 *  the first measurement rather than flashing to zero. */
const pinnedHeight = computed(() => iframeHeight.value);

const themeStore = useThemeStore();
const layout = usePluginLayout();

// The host design tokens for the plugin sandbox, read fresh so they reflect the
// active theme (and accent override). The runtime injects them as `--nd-*`.
function themeSnapshot(): PluginTheme {
  return snapshotPluginTheme(
    themeStore.isDarkMode ? 'dark' : 'light',
    themeStore.effectiveTheme?.meta?.id ?? themeStore.currentTheme,
  );
}

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
// Project the host address row onto the clean `PluginAddress` wire shape
// (camelCase + a `formatted` one-liner the plugin geocodes / shows).
function toPluginAddress(a: UserAddress | undefined): PluginAddress | null {
  if (!a) return null;
  const formatted = [a.street, a.city, a.region, a.postal_code, a.country]
    .filter(Boolean)
    .join(', ');
  return {
    id: a.id,
    addressType: a.address_type,
    isPrimary: a.is_primary,
    street: a.street,
    city: a.city,
    region: a.region,
    postalCode: a.postal_code,
    country: a.country,
    label: a.label,
    formatted,
  };
}
function snapshot(): PluginContext {
  return {
    ticket: plain(props.context?.ticket),
    asset: plain(props.context?.asset),
    user: plain(props.context?.user),
    address: toPluginAddress(props.context?.address),
    ticketIds: plain(props.context?.ticketIds),
    component: { name: props.component.name, slot: props.component.slot },
    layout: { ...layout.value },
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
  postInit(win, bridge, snapshot(), themeSnapshot());
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

// Push a fresh context snapshot whenever the context bag, the component, or the
// action counter changes. `deep` is required: the sync pool patches ticket/asset
// rows IN PLACE (same object identity), so a shallow, reference-keyed watch would
// miss field updates and leave the plugin on stale context.
// The breakpoint is watched as a STRING, not as `layout` itself. The computed
// re-evaluates on every debounced resize and returns a fresh object, so
// watching the ref would re-post the whole context (a full JSON clone of the
// ticket / asset / user bag, per frame) every 150ms for the length of a drag.
// Watching the bucket means one push per breakpoint crossing.
watch(
  [
    () => props.context,
    () => props.component,
    () => props.actionActivated,
    () => layout.value.breakpoint,
  ],
  () => {
    const win = frameRef.value?.contentWindow;
    if (connected && win) postContext(win, snapshot());
  },
  { deep: true },
);

// Re-push the design tokens when the host theme or accent changes. `flush: 'post'`
// so the DOM has the new theme applied before `snapshotPluginTheme` reads the
// resolved values off `:root`.
watch(
  [
    () => themeStore.isDarkMode,
    () => themeStore.effectiveTheme?.meta?.id,
    () => themeStore.accentColorOverride,
  ],
  () => {
    const win = frameRef.value?.contentWindow;
    if (connected && win) postTheme(win, themeSnapshot());
  },
  { flush: 'post' },
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
    <!-- Error state (hidden on print). Matches the app's inline error
         treatment (centred, small, `text-status-error`) rather than drawing
         its own red box, which read as foreign next to built-in cards. -->
    <div
      v-if="error"
      class="print:hidden flex items-center justify-center px-4 py-6 text-center text-xs text-status-error"
    >
      {{ error }}
    </div>

    <iframe
      v-else-if="runtimeUrl"
      ref="frameRef"
      :src="runtimeUrl"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      class="plugin-sandbox-iframe"
      :style="pinnedHeight != null ? { height: `${pinnedHeight}px` } : undefined"
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
     the runtime's ResizeObserver over the bridge).

     No min-height floor: one used to hold the frame at 4rem until the first
     report landed, so every panel painted an empty 64px box and then jumped to
     its real height, and a plugin that rendered nothing held 64px of blank
     space forever. Starting at 0 and growing means the only motion is the
     content arriving. The host chrome renders immediately either way, so there
     is no blank-then-pop, and nothing here needs a skeleton. */
}

@media print {
  .plugin-sandbox-frame {
    display: none;
  }
}
</style>
