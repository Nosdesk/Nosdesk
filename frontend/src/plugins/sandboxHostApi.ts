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
import { proxy } from '@nosdesk/plugin-sdk';
import type { HostApi, PluginFetchOptions } from '@nosdesk/plugin-sdk';
import type { Plugin } from '@nosdesk/core/types/plugin';
import { createPluginAPI } from './api';

/**
 * Which tiers render in the iframe sandbox. Community (and any future untrusted
 * tier) run sandboxed; official/verified/local stay on the in-process Vue-
 * component path until they migrate to the `{ mount }` contract. Keeping this in
 * one place mirrors `getHostApiForPlugin`'s single-dispatch DRY guarantee.
 */
export function isSandboxed(plugin: Plugin): boolean {
  return plugin.trust_level === 'community';
}

/** Build the `HostApi` a sandboxed plugin sees, backed by the in-process impl. */
export function createHostApiImpl(plugin: Plugin): HostApi {
  const inproc = createPluginAPI(plugin);

  return {
    version: inproc.version,
    plugin: inproc.plugin,

    tickets: {
      get: (id) => inproc.tickets.get(id),
      list: () => inproc.tickets.list(),
      addComment: (id, comment) => inproc.tickets.addComment(id, comment),
    },
    devices: {
      get: (id) => inproc.devices.get(id),
      list: () => inproc.devices.list(),
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
      const res = await inproc.fetch(url, options as PluginFetchOptions);
      if (!res) return null;
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
}
