import * as Comlink from 'comlink';

import type {
  HostApi,
  PluginHostApi,
  PluginContext,
  PluginTheme,
  PluginEvent,
  PluginEventHandler,
  PluginEventPayload,
} from './types';

/** Protocol messages the host posts to the plugin iframe's `contentWindow`. */
interface HostInitMessage {
  type: 'nosdesk-plugin-init';
  context: PluginContext;
  theme: PluginTheme;
}
interface HostContextMessage {
  type: 'nosdesk-plugin-context';
  context: PluginContext;
}
interface HostThemeMessage {
  type: 'nosdesk-plugin-theme';
  theme: PluginTheme;
}

/** The ready plugin runtime handed to a plugin's `mount`. */
export interface PluginRuntime {
  /** The host API, proxied over the bridge (every call is a round-trip). `on` is
   * handled locally, see `wrapEvents`. */
  api: PluginHostApi;
  /** The context snapshot at connect time. */
  context: PluginContext;
  /** The host design tokens at connect time. The runtime injects these; a plugin
   * rarely reads it directly. */
  theme: PluginTheme;
  /** Subscribe to context snapshots the host pushes after connect; returns an
   * unsubscribe. */
  onContextChange(cb: (ctx: PluginContext) => void): () => void;
  /** Subscribe to design-token snapshots the host pushes on a theme change;
   * returns an unsubscribe. The runtime uses this to re-inject the variables. */
  onThemeChange(cb: (theme: PluginTheme) => void): () => void;
  /** Drop all `api.on` subscriptions. The runtime calls this before re-mounting a
   * simple plugin so event handlers don't accumulate across re-mounts. */
  resetEvents(): void;
}

/** How long to wait for the host's init message before giving up. */
const CONNECT_TIMEOUT_MS = 10_000;

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
function wrapEvents(remote: Comlink.Remote<HostApi>): {
  api: PluginHostApi;
  reset: () => void;
} {
  const local = new Map<PluginEvent, Set<PluginEventHandler>>();
  const remoteUnsub = new Map<PluginEvent, Promise<() => Promise<void>>>();

  const dispatch = (event: PluginEvent, payload: PluginEventPayload): void => {
    const set = local.get(event);
    if (!set) return;
    // Copy so a handler that unsubscribes mid-dispatch doesn't skip the next.
    for (const h of [...set]) {
      try {
        void h(payload);
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
      remoteUnsub.set(
        event,
        remote.on(event, Comlink.proxy((d: PluginEventPayload) => dispatch(event, d))),
      );
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

  // Drop every local subscription + its single host-side dispatcher. The runtime
  // calls this before re-mounting a simple plugin (one that returned void, so the
  // runtime unmount+re-mounts on context change) — otherwise a plugin that
  // subscribes in `mount` would accumulate a handler per re-mount and receive
  // each event N times. A plugin returning `{ update }` is never re-mounted, so
  // its subscriptions persist.
  const reset = (): void => {
    for (const unsubP of remoteUnsub.values()) {
      void unsubP.then((fn) => fn()).catch(() => {});
    }
    remoteUnsub.clear();
    local.clear();
  };

  const api = new Proxy(remote, {
    get(target, prop, receiver) {
      if (prop === 'on') return on;
      return Reflect.get(target, prop, receiver);
    },
  }) as unknown as PluginHostApi;

  return { api, reset };
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
  return new Promise((resolve, reject) => {
    const listeners = new Set<(ctx: PluginContext) => void>();
    const themeListeners = new Set<(theme: PluginTheme) => void>();
    // Placeholder until the host's init message delivers the real context; the
    // runtime resolves with `data.context`, so this is never handed to a plugin.
    let context: PluginContext = {
      ticket: null,
      asset: null,
      user: null,
      address: null,
      component: { name: '', slot: '' },
    };
    let theme: PluginTheme = { tokens: {}, colorScheme: 'light', name: 'light' };
    let connected = false;

    // Fail loudly if the host never connects (dropped port / host disposed before
    // postInit) so `boot()` can render an error instead of hanging on a blank
    // frame. The listener stays after connect — context updates reuse it.
    const timer = setTimeout(() => {
      if (!connected) {
        window.removeEventListener('message', onMessage);
        reject(new Error('sandbox runtime: host did not connect in time'));
      }
    }, CONNECT_TIMEOUT_MS);

    function onMessage(event: MessageEvent) {
      const data = event.data as
        | HostInitMessage
        | HostContextMessage
        | HostThemeMessage
        | undefined;
      if (!data || typeof data !== 'object') return;

      if (!connected && data.type === 'nosdesk-plugin-init' && event.ports[0]) {
        connected = true;
        clearTimeout(timer);
        context = data.context;
        theme = data.theme;
        const { api, reset } = wrapEvents(Comlink.wrap<HostApi>(event.ports[0]));
        resolve({
          api,
          context,
          theme,
          onContextChange(cb) {
            listeners.add(cb);
            return () => {
              listeners.delete(cb);
            };
          },
          onThemeChange(cb) {
            themeListeners.add(cb);
            return () => {
              themeListeners.delete(cb);
            };
          },
          resetEvents: reset,
        });
      } else if (data.type === 'nosdesk-plugin-context') {
        context = data.context;
        for (const cb of listeners) cb(context);
      } else if (data.type === 'nosdesk-plugin-theme') {
        theme = data.theme;
        for (const cb of themeListeners) cb(theme);
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
