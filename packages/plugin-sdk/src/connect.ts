import * as Comlink from 'comlink';

import type { HostApi, PluginContext } from './types';

/** Protocol messages the host posts to the plugin iframe's `contentWindow`. */
interface HostInitMessage {
  type: 'nosdesk-plugin-init';
  context: PluginContext;
}
interface HostContextMessage {
  type: 'nosdesk-plugin-context';
  context: PluginContext;
}

/** The ready plugin runtime handed to a plugin's `mount`. */
export interface PluginRuntime {
  /** The host API, proxied over the bridge (every call is a round-trip). */
  api: Comlink.Remote<HostApi>;
  /** The context snapshot at connect time. */
  context: PluginContext;
  /** Subscribe to context snapshots the host pushes after connect; returns an
   * unsubscribe. */
  onContextChange(cb: (ctx: PluginContext) => void): () => void;
}

/**
 * Iframe-side handshake. Waits for the host to post the init message (carrying
 * the transferred `MessageChannel` port + the initial context), wraps the port
 * with Comlink, and resolves the runtime. Thereafter all API traffic flows over
 * the port; the host pushes context updates as window messages.
 *
 * Auth is capability-based, not origin-based: an opaque sandboxed iframe reports
 * `event.origin === "null"`, so we trust the first init message our parent hands
 * us (only the host can post to this specific `contentWindow` with the port),
 * and communicate only over that port afterwards.
 */
export function connectToHost(): Promise<PluginRuntime> {
  return new Promise((resolve) => {
    const listeners = new Set<(ctx: PluginContext) => void>();
    // Placeholder until the host's init message delivers the real context; the
    // runtime resolves with `data.context`, so this is never handed to a plugin.
    let context: PluginContext = { ticket: null, device: null, component: { name: '', slot: '' } };
    let connected = false;

    function onMessage(event: MessageEvent) {
      const data = event.data as HostInitMessage | HostContextMessage | undefined;
      if (!data || typeof data !== 'object') return;

      if (!connected && data.type === 'nosdesk-plugin-init' && event.ports[0]) {
        connected = true;
        context = data.context;
        const api = Comlink.wrap<HostApi>(event.ports[0]);
        resolve({
          api,
          context,
          onContextChange(cb) {
            listeners.add(cb);
            return () => {
              listeners.delete(cb);
            };
          },
        });
      } else if (data.type === 'nosdesk-plugin-context') {
        context = data.context;
        for (const cb of listeners) cb(context);
      }
    }

    window.addEventListener('message', onMessage);
  });
}

/** Re-export so a plugin can wrap its own callbacks (e.g. for `api.on`) when it
 * needs to pass a function across the bridge. */
export const proxy = Comlink.proxy;

/**
 * Report the plugin's current content height (px) to the host so it can size the
 * iframe to the content (a cross-origin sandboxed iframe can't self-size). Posted
 * as a plain window message to the parent, not over the Comlink port; the host
 * matches it by `event.source` (the port is the API capability, this is display
 * only). The runtime calls this automatically via a ResizeObserver.
 */
export function reportHeight(height: number): void {
  window.parent.postMessage({ type: 'nosdesk-plugin-height', height }, '*');
}
