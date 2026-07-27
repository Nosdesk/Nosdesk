// Host-side adapter for sandboxed plugins.
//
// `createHostApiImpl` reshapes the in-process `PluginAPI` (from `createPluginAPI`,
// with all its permission gating + service calls intact) into the boundary-safe
// `HostApi` the SDK bridge exposes over Comlink. Only the four surfaces the
// process boundary forces change: `fetch` returns a plain object (not a live
// `Response`), `collections`/`on` hand back Comlink proxies, and the sync
// `notify`/`isPrinting` become async. Context is delivered by the frame (via
// `postInit`/`postContext`), not through this object.
//
// The bridge itself (`createRemoteHostApi`) is pure transport; every permission
// check still runs here, in-process, exactly as for a first-party plugin.
import { proxy, BridgeGovernor, governHostApi } from '@nosdesk/plugin-sdk';
import type { HostApi, PluginFetchOptions } from '@nosdesk/plugin-sdk';
import type { Plugin } from '@nosdesk/core/types/plugin';
import { createPluginAPI, type PluginAPI } from './api';

/**
 * Build the `HostApi` a sandboxed plugin sees, backed by the in-process impl.
 * Returns the `inproc` instance too: the frame registers it in the live-instance
 * registry so the event dispatcher reaches the plugin's `on` handlers (which land
 * on `inproc` and forward across the bridge).
 */
export function createHostApiImpl(plugin: Plugin): { hostApi: HostApi; inproc: PluginAPI } {
  const inproc = createPluginAPI(plugin);

  const hostApi: HostApi = {
    version: inproc.version,
    plugin: inproc.plugin,

    tickets: {
      get: (id) => inproc.tickets.get(id),
      list: () => inproc.tickets.list(),
      addComment: (id, comment) => inproc.tickets.addComment(id, comment),
      update: (id, patch) => inproc.tickets.update(id, patch),
      delete: (id) => inproc.tickets.delete(id),
    },
    assets: {
      get: (id) => inproc.assets.get(id),
      list: () => inproc.assets.list(),
      update: (id, patch) => inproc.assets.update(id, patch),
    },
    users: {
      get: (uuid) => inproc.users.get(uuid),
    },
    attachments: {
      async list(ticketId) {
        // The in-process shape carries nullable/extra fields; narrow to the
        // boundary shape with sane defaults.
        const atts = await inproc.attachments.list(ticketId);
        return atts.map((a) => ({
          id: a.id,
          name: a.name,
          url: a.url,
          mimeType: a.mimeType ?? 'application/octet-stream',
          size: a.size ?? 0,
        }));
      },
      getBase64: (attachmentId, ticketId) => inproc.attachments.getBase64(attachmentId, ticketId),
    },

    async fetch(url, options) {
      // inproc.fetch throws a PluginApiError on denial / invalid URL / failure;
      // a returned Response is always a real response.
      const res = await inproc.fetch(url, options as PluginFetchOptions);
      const headers: Record<string, string> = {};
      res.headers.forEach((value, key) => {
        headers[key] = value;
      });
      return { status: res.status, headers, body: await res.text() };
    },

    storage: {
      get: (key) => inproc.storage.get(key),
      set: (key, value) => inproc.storage.set(key, value),
      delete: (key) => inproc.storage.delete(key),
    },

    // A collection sub-API is stateful (bound to `name`); proxy it so the
    // plugin's calls on it round-trip too.
    async collections(name) {
      return proxy(inproc.collections.get(name));
    },

    // The plugin passes a Comlink-proxied handler; forward it into the
    // in-process registry and proxy the unsubscribe back. (Dispatch of events to
    // sandboxed plugins is wired in a follow-up; registration is inert until
    // then, but never throws.)
    async on(event, handler) {
      const unsubscribe = inproc.on(event, (data) => {
        void handler(data);
      });
      return proxy(async () => {
        unsubscribe();
      });
    },

    async notify(message, type) {
      inproc.notify(message, type);
    },
    ui: {
      async isPrinting() {
        return inproc.ui.isPrinting();
      },
    },
  };

  // Meter every bridge call: rate limit + in-flight cap + per-call timeout, per
  // plugin instance. The plugin sees over-limit calls as thrown errors; the host
  // stays protected from a buggy or hostile plugin flooding shared capacity.
  return { hostApi: governHostApi(hostApi, new BridgeGovernor()), inproc };
}
