import * as Comlink from 'comlink';

import type { HostApi, PluginHostApi, PluginContext, PluginEvent, PluginEventHandler } from './types';

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
  /** The host API, proxied over the bridge (every call is a round-trip). `on` is
   * handled locally, see `wrapEvents`. */
  api: PluginHostApi;
  /** The context snapshot at connect time. */
  context: PluginContext;
  /** Subscribe to context snapshots the host pushes after connect; returns an
   * unsubscribe. */
  onContextChange(cb: (ctx: PluginContext) => void): () => void;
}

/**
 * Wrap the Comlink-remote host API so `api.on(event, handler)` works with a plain
 * plugin callback. A separately-bundled plugin can't hand a working Comlink-proxy
 * callback across the bridge — Comlink's `proxyMarker` is a per-module `Symbol`,
 * so the plugin's proxy marker differs from the marker the runtime's `api`
 * (`Comlink.wrap`) recognizes, and the handler fails to structured-clone
 * (`DataCloneError`). So instead: keep the plugin's handlers LOCAL to the iframe,
 * and register a single dispatcher per event with the host, proxied with THIS
 * module's Comlink (the same instance that wrapped the port, so the marker
 * matches). The host calls that one dispatcher; we fan out to local handlers.
 * Every other member passes straight through to the remote.
 */
function wrapEvents(remote: Comlink.Remote<HostApi>): PluginHostApi {
  const local = new Map<PluginEvent, Set<PluginEventHandler>>();
  const remoteUnsub = new Map<PluginEvent, Promise<() => Promise<void>>>();

  const dispatch = (event: PluginEvent, data: unknown): void => {
    const set = local.get(event);
    if (!set) return;
    // Copy so a handler that unsubscribes mid-dispatch doesn't skip the next.
    for (const h of [...set]) {
      try {
        void h(data);
      } catch {
        // Isolate one handler's failure from the others.
      }
    }
  };

  const on = async (event: PluginEvent, handler: PluginEventHandler): Promise<() => void> => {
    let set = local.get(event);
    if (!set) {
      set = new Set();
      local.set(event, set);
      remoteUnsub.set(event, remote.on(event, Comlink.proxy((d: unknown) => dispatch(event, d))));
    }
    set.add(handler);
    return () => {
      const s = local.get(event);
      if (!s) return;
      s.delete(handler);
      if (s.size === 0) {
        local.delete(event);
        const unsubP = remoteUnsub.get(event);
        remoteUnsub.delete(event);
        // Fire-and-forget the host-side unsubscribe.
        void unsubP?.then((fn) => fn()).catch(() => {});
      }
    };
  };

  return new Proxy(remote, {
    get(target, prop, receiver) {
      if (prop === 'on') return on;
      return Reflect.get(target, prop, receiver);
    },
  }) as unknown as PluginHostApi;
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
        const api = wrapEvents(Comlink.wrap<HostApi>(event.ports[0]));
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

/** Re-export of Comlink's `proxy`. Plugins do NOT need this for `api.on` (the
 * runtime proxies that callback for them); kept for advanced cases where a plugin
 * itself hands a function across the bridge. */
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
