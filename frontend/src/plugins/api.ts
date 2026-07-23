/**
 * Plugin API
 *
 * Defines the `PluginAPI` interface (the host-side contract every
 * plugin sees) and the in-process implementation that backs
 * official, verified, and local-tier plugins. Plugin code receives
 * a value typed as `PluginAPI`; whether the underlying methods run
 * directly against Vue stores (this file) or marshal across a
 * postMessage boundary to a sandboxed iframe (planned follow-up
 * for community-tier plugins) is a host-side detail the plugin
 * never needs to know.
 *
 * That single-interface, multiple-impl shape is the architectural
 * commitment behind the eventual community-tier sandbox: when the
 * remote impl lands, plugin authors don't change a line of code,
 * and the only place that decides which transport to use is the
 * `getHostApiForPlugin` factory below.
 */

import pluginService from '@nosdesk/core/services/pluginService';
import { getTicketById, getTickets, addCommentToTicket } from '@nosdesk/core/services/ticketService';
import { getAssetById, getAssets } from '@/services/assetService';
import { logger } from '@nosdesk/core/utils/logger';
import { useToastStore } from '@nosdesk/core/stores/toast';
import type { Plugin, PluginPermission, PluginProxyRequest, PluginEvent, CollectionRow, CollectionListResponse } from '@nosdesk/core/types/plugin';
import type { Ticket } from '@nosdesk/core/types/ticket';
import type { Asset } from '@nosdesk/core/types/asset';

// =============================================================================
// Types
// =============================================================================

export interface PluginComment {
  content: string;
  metadata?: Record<string, unknown>;
}

export interface PluginAttachment {
  id: number;
  name: string;
  url: string;
  mimeType: string | null;
  size: number | null;
  thumbnailUrl: string | null;
  ticketId: number;
  commentId: number;
}

export interface PluginContext {
  ticket: Ticket | null;
  device: Asset | null;
}

export interface PluginUIHelpers {
  /** Check if the page is currently being printed (via matchMedia) */
  isPrinting(): boolean;
}

export type EventHandler = (data: unknown) => void | Promise<void>;

/** Options for plugin fetch() — extends standard RequestInit with proxy-specific fields */
export interface PluginFetchOptions extends Omit<RequestInit, 'body'> {
  body?: unknown;
  /** Body encoding: "json" (default) or "form" (application/x-www-form-urlencoded) */
  content_type?: 'json' | 'form';
}

// =============================================================================
// Plugin API Factory
// =============================================================================

/**
 * Create a Plugin API instance for a specific plugin.
 * The API is sandboxed - each plugin gets its own instance with access only to what it's permitted.
 */
export function createPluginAPI(plugin: Plugin): PluginAPI {
  const permissions = new Set<string>(plugin.manifest.permissions);
  const eventHandlers = new Map<PluginEvent, EventHandler[]>();

  // Typed permission check. Accepts non-network permissions
  // directly; for `network:<host>` use `hasNetworkPermission(host)`.
  const hasPermission = (permission: PluginPermission): boolean => {
    return permissions.has(permission);
  };

  // Host-coverage check shared by `api.fetch` (and any future
  // network-using surface). Mirrors the backend proxy's logic so a
  // denied request fails closed at the boundary instead of round-
  // tripping. Single-level wildcard semantics: `*.example.com`
  // covers `example.com` and `<one_label>.example.com`, not deeper.
  const hasNetworkPermission = (host: string): boolean => {
    const target = host.toLowerCase();
    for (const p of permissions) {
      if (!p.startsWith('network:')) continue;
      const pattern = p.slice('network:'.length).toLowerCase();
      if (pattern.startsWith('*.')) {
        const apex = pattern.slice(2);
        if (target === apex) return true;
        if (target.endsWith(`.${apex}`)) {
          const prefix = target.slice(0, target.length - apex.length - 1);
          if (prefix.length > 0 && !prefix.includes('.')) return true;
        }
      } else if (pattern === target) {
        return true;
      }
    }
    return false;
  };

  // Current context (set by the UI slot system)
  let context: PluginContext = {
    ticket: null,
    device: null,
  };

  const api: PluginAPI = {
    version: '1.0.0',

    // === Plugin Info ===
    plugin: {
      uuid: plugin.uuid,
      name: plugin.name,
      displayName: plugin.display_name,
      version: plugin.version,
    },

    // === READ: Access core data ===
    tickets: {
      async get(id: number): Promise<Ticket | null> {
        if (!hasPermission('ticket:read')) {
          logger.warn(`Plugin ${plugin.name} denied ticket:read permission`);
          return null;
        }
        try {
          return await getTicketById(id);
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to get ticket`, { id, error });
          return null;
        }
      },
      async list(): Promise<Ticket[]> {
        if (!hasPermission('ticket:read')) {
          logger.warn(`Plugin ${plugin.name} denied ticket:read permission`);
          return [];
        }
        try {
          return await getTickets();
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to list tickets`, { error });
          return [];
        }
      },
      async addComment(ticketId: number, comment: PluginComment): Promise<boolean> {
        if (!hasPermission('ticket:comment')) {
          logger.warn(`Plugin ${plugin.name} denied ticket:comment permission`);
          return false;
        }
        try {
          await addCommentToTicket(ticketId, comment.content, []);
          return true;
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to add comment`, { ticketId, error });
          return false;
        }
      },
    },

    devices: {
      async get(id: number): Promise<Asset | null> {
        if (!hasPermission('asset:read')) {
          logger.warn(`Plugin ${plugin.name} denied device:read permission`);
          return null;
        }
        try {
          return await getAssetById(id);
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to get device`, { id, error });
          return null;
        }
      },
      async list(): Promise<Asset[]> {
        if (!hasPermission('asset:read')) {
          logger.warn(`Plugin ${plugin.name} denied device:read permission`);
          return [];
        }
        try {
          return await getAssets();
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to list devices`, { error });
          return [];
        }
      },
    },

    // === ATTACHMENTS: Access ticket attachments ===
    attachments: {
      async list(ticketId: number): Promise<PluginAttachment[]> {
        if (!hasPermission('ticket:read')) {
          logger.warn(`Plugin ${plugin.name} denied ticket:read permission`);
          return [];
        }
        try {
          const ticket = await getTicketById(ticketId);
          if (!ticket) return [];
          const result: PluginAttachment[] = [];
          for (const comment of ticket.comments || []) {
            for (const att of comment.attachments || []) {
              result.push({
                id: att.id,
                name: att.name,
                url: att.url,
                mimeType: att.mime_type ?? null,
                size: att.file_size ?? null,
                thumbnailUrl: att.thumbnail_url ?? null,
                ticketId,
                commentId: comment.id,
              });
            }
          }
          return result;
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to list attachments`, { ticketId, error });
          return [];
        }
      },
      async getContent(attachmentId: number, ticketId: number): Promise<{ blob: Blob; name: string; mimeType: string } | null> {
        if (!hasPermission('ticket:read')) {
          logger.warn(`Plugin ${plugin.name} denied ticket:read permission`);
          return null;
        }
        try {
          const attachments = await api.attachments.list(ticketId);
          const att = attachments.find(a => a.id === attachmentId);
          if (!att) return null;
          const response = await fetch(att.url, { credentials: 'same-origin' });
          if (!response.ok) return null;
          const blob = await response.blob();
          return { blob, name: att.name, mimeType: att.mimeType || 'application/octet-stream' };
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to get attachment content`, { attachmentId, ticketId, error });
          return null;
        }
      },
      async getBase64(attachmentId: number, ticketId: number): Promise<{ data: string; name: string; mimeType: string } | null> {
        if (!hasPermission('ticket:read')) {
          logger.warn(`Plugin ${plugin.name} denied ticket:read permission`);
          return null;
        }
        try {
          const content = await api.attachments.getContent(attachmentId, ticketId);
          if (!content) return null;
          const buffer = await content.blob.arrayBuffer();
          const bytes = new Uint8Array(buffer);
          let binary = '';
          for (let i = 0; i < bytes.length; i++) {
            binary += String.fromCharCode(bytes[i]);
          }
          const data = btoa(binary);
          return { data, name: content.name, mimeType: content.mimeType };
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to get attachment base64`, { attachmentId, ticketId, error });
          return null;
        }
      },
    },

    // === INTEGRATE: External services ===
    // The backend proxy is the source of truth for network
    // permission enforcement, but we also fail closed here so a
    // denied plugin doesn't burn round-trips and the rejection
    // pattern is consistent with every other gated method.
    async fetch(url: string, options?: PluginFetchOptions): Promise<Response | null> {
      let parsed: URL;
      try {
        parsed = new URL(url);
      } catch {
        logger.warn(`Plugin ${plugin.name} fetch refused: invalid URL`, { url });
        return null;
      }
      if (!hasNetworkPermission(parsed.host)) {
        logger.warn(
          `Plugin ${plugin.name} fetch refused: no matching network:<host> permission`,
          { host: parsed.host }
        );
        return null;
      }

      try {
        const request: PluginProxyRequest = {
          url,
          method: (options?.method as PluginProxyRequest['method']) || 'GET',
          headers: options?.headers as Record<string, string>,
          body: options?.body ? (typeof options.body === 'string' ? JSON.parse(options.body) : options.body) : undefined,
          content_type: options?.content_type,
        };

        const response = await pluginService.proxyRequest(plugin.uuid, request);

        // Convert to Response-like object
        return new Response(JSON.stringify(response.body), {
          status: response.status,
          headers: response.headers,
        });
      } catch (error) {
        logger.error(`Plugin ${plugin.name} fetch failed`, { url, error });
        return null;
      }
    },

    // === STORE: Plugin data ===
    storage: {
      async get<T>(key: string): Promise<T | null> {
        if (!hasPermission('storage:plugin')) {
          logger.warn(`Plugin ${plugin.name} denied storage:plugin permission`);
          return null;
        }
        try {
          const result = await pluginService.getStorage(plugin.uuid, key);
          return result.value as T;
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to get storage`, { key, error });
          return null;
        }
      },
      async set<T>(key: string, value: T): Promise<boolean> {
        if (!hasPermission('storage:plugin')) {
          logger.warn(`Plugin ${plugin.name} denied storage:plugin permission`);
          return false;
        }
        try {
          await pluginService.setStorage(plugin.uuid, { key, value });
          return true;
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to set storage`, { key, error });
          return false;
        }
      },
      async delete(key: string): Promise<boolean> {
        if (!hasPermission('storage:plugin')) {
          logger.warn(`Plugin ${plugin.name} denied storage:plugin permission`);
          return false;
        }
        try {
          await pluginService.deleteStorage(plugin.uuid, key);
          return true;
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to delete storage`, { key, error });
          return false;
        }
      },
    },

    // === COLLECTIONS: Typed collection data ===
    collections: {
      get(collectionName: string) {
        const hasCollectionRead = hasPermission('collection:read');
        const hasCollectionWrite = hasPermission('collection:write');

        return {
          async create(data: Record<string, unknown>): Promise<CollectionRow | null> {
            if (!hasCollectionWrite) {
              logger.warn(`Plugin ${plugin.name} denied collection:write permission`);
              return null;
            }
            try {
              return await pluginService.createCollectionRow(plugin.uuid, collectionName, data);
            } catch (error) {
              logger.error(`Plugin ${plugin.name} failed to create collection row`, { collectionName, error });
              return null;
            }
          },
          async get(uuid: string): Promise<CollectionRow | null> {
            if (!hasCollectionRead) {
              logger.warn(`Plugin ${plugin.name} denied collection:read permission`);
              return null;
            }
            try {
              return await pluginService.getCollectionRow(plugin.uuid, collectionName, uuid);
            } catch (error) {
              logger.error(`Plugin ${plugin.name} failed to get collection row`, { collectionName, uuid, error });
              return null;
            }
          },
          async update(uuid: string, data: Record<string, unknown>): Promise<CollectionRow | null> {
            if (!hasCollectionWrite) {
              logger.warn(`Plugin ${plugin.name} denied collection:write permission`);
              return null;
            }
            try {
              return await pluginService.updateCollectionRow(plugin.uuid, collectionName, uuid, data);
            } catch (error) {
              logger.error(`Plugin ${plugin.name} failed to update collection row`, { collectionName, uuid, error });
              return null;
            }
          },
          async delete(uuid: string): Promise<boolean> {
            if (!hasCollectionWrite) {
              logger.warn(`Plugin ${plugin.name} denied collection:write permission`);
              return false;
            }
            try {
              await pluginService.deleteCollectionRow(plugin.uuid, collectionName, uuid);
              return true;
            } catch (error) {
              logger.error(`Plugin ${plugin.name} failed to delete collection row`, { collectionName, uuid, error });
              return false;
            }
          },
          async list(params?: { limit?: number; offset?: number; filter?: string; sort_by?: string; sort_order?: string }): Promise<CollectionListResponse> {
            if (!hasCollectionRead) {
              logger.warn(`Plugin ${plugin.name} denied collection:read permission`);
              return { rows: [], total: 0 };
            }
            try {
              return await pluginService.listCollectionRows(plugin.uuid, collectionName, params);
            } catch (error) {
              logger.error(`Plugin ${plugin.name} failed to list collection rows`, { collectionName, error });
              return { rows: [], total: 0 };
            }
          },
        };
      },
    },

    // === OBSERVE: React to events ===
    on(event: PluginEvent, handler: EventHandler): () => void {
      const handlers = eventHandlers.get(event) || [];
      handlers.push(handler);
      eventHandlers.set(event, handlers);

      // Return unsubscribe function
      return () => {
        const currentHandlers = eventHandlers.get(event) || [];
        const index = currentHandlers.indexOf(handler);
        if (index > -1) {
          currentHandlers.splice(index, 1);
          eventHandlers.set(event, currentHandlers);
        }
      };
    },

    // === NOTIFY: User feedback ===
    // Surface plugin notifications through the same toast store
    // the rest of the app uses. Plugin name is the toast title so
    // users can tell which plugin is talking; message is the body.
    // We intentionally don't expose toast actions / undo from the
    // plugin API surface — keeps the affordance simple and prevents
    // a plugin from re-implementing its own confirm flow.
    notify(message: string, type: 'info' | 'success' | 'warning' | 'error' = 'info'): void {
      logger.info(`Plugin notification [${type}]: ${message}`, { plugin: plugin.name });
      const toast = useToastStore();
      toast[type](plugin.name, message);
    },

    // === UI: UI state helpers ===
    ui: {
      isPrinting(): boolean {
        return window.matchMedia?.('print').matches ?? false;
      },
    },

    // === CONTEXT: Current state ===
    get context(): PluginContext {
      return context;
    },

    // Internal: Set context (called by slot system)
    _setContext(newContext: Partial<PluginContext>): void {
      context = { ...context, ...newContext };
    },

    // Internal: Get event handlers for dispatching
    _getEventHandlers(event: PluginEvent): EventHandler[] {
      return eventHandlers.get(event) || [];
    },
  };

  return api;
}

// =============================================================================
// Plugin API Interface
// =============================================================================

export interface PluginAPI {
  /** Plugin runtime API version exposed to plugin code as a
   * semver string. The major component is the breaking-change
   * signal — `engines.plugin_api: "1"` matches every `1.x.y`
   * runtime; bumping to `"2"` requires every plugin to opt in.
   * The minor component advances when we ADD API methods within
   * the v1 family, letting plugins capability-detect:
   *
   *     // semver-aware feature detect
   *     const [major, minor] = api.version.split('.').map(Number);
   *     if (major >= 1 && minor >= 2 && api.notifications) { ... }
   */
  version: string;

  // Plugin info
  plugin: {
    uuid: string;
    name: string;
    displayName: string;
    version: string;
  };

  // Read core data
  tickets: {
    get(id: number): Promise<Ticket | null>;
    list(): Promise<Ticket[]>;
    addComment(ticketId: number, comment: PluginComment): Promise<boolean>;
  };

  devices: {
    get(id: number): Promise<Asset | null>;
    list(): Promise<Asset[]>;
  };

  // Attachment access
  attachments: {
    list(ticketId: number): Promise<PluginAttachment[]>;
    getContent(attachmentId: number, ticketId: number): Promise<{ blob: Blob; name: string; mimeType: string } | null>;
    getBase64(attachmentId: number, ticketId: number): Promise<{ data: string; name: string; mimeType: string } | null>;
  };

  // External requests
  fetch(url: string, options?: PluginFetchOptions): Promise<Response | null>;

  // Plugin storage
  storage: {
    get<T>(key: string): Promise<T | null>;
    set<T>(key: string, value: T): Promise<boolean>;
    delete(key: string): Promise<boolean>;
  };

  // Typed collections
  collections: {
    get(collectionName: string): {
      create(data: Record<string, unknown>): Promise<CollectionRow | null>;
      get(uuid: string): Promise<CollectionRow | null>;
      update(uuid: string, data: Record<string, unknown>): Promise<CollectionRow | null>;
      delete(uuid: string): Promise<boolean>;
      list(params?: { limit?: number; offset?: number; filter?: string; sort_by?: string; sort_order?: string }): Promise<CollectionListResponse>;
    };
  };

  // Event subscription
  on(event: PluginEvent, handler: EventHandler): () => void;

  // User feedback
  notify(message: string, type?: 'info' | 'success' | 'warning' | 'error'): void;

  // UI helpers
  ui: PluginUIHelpers;

  // Current context
  context: PluginContext;

  // Internal methods (not for plugin use)
  _setContext(context: Partial<PluginContext>): void;
  _getEventHandlers(event: PluginEvent): EventHandler[];
}

// =============================================================================
// HostApi factory dispatch
// =============================================================================

/**
 * Build the `PluginAPI` instance a plugin will see, picking the
 * implementation by trust tier.
 *
 * Today every tier returns the in-process implementation built
 * by `createPluginAPI`. The dispatch exists so the eventual
 * community-tier iframe sandbox slots in here without touching
 * the three call sites in `componentLoader`, `eventDispatcher`,
 * and `PluginSlotItem`. When that lands, the `community` arm
 * will return a Comlink-wrapped remote that satisfies the same
 * `PluginAPI` interface; plugin code is unchanged.
 *
 * Keeping the dispatch in one function (rather than inlining the
 * tier check at every call site) is the DRY guarantee that the
 * tier-to-transport policy can't drift between callers.
 */
export function getHostApiForPlugin(plugin: Plugin): PluginAPI {
  switch (plugin.trust_level) {
    case 'official':
    case 'verified':
    case 'local':
      return createPluginAPI(plugin);
    case 'community':
      // Community plugins RENDER in the opaque-origin iframe sandbox
      // (`PluginSlotItem` -> `PluginSandboxFrame`, host API bridged via
      // `createHostApiImpl`), never through this in-process instance.
      // This arm is reached only by the event dispatcher's per-plugin
      // cache; the in-process impl there is inert for community plugins
      // (component rendering is gated off, and event delivery to
      // sandboxed plugins is a committed follow-up), so returning it is
      // harmless and keeps the dispatcher's loop uniform.
      return createPluginAPI(plugin);
    default:
      // Unknown tier (forward-compat for future tiers we haven't
      // taught the frontend about). Fail closed by routing through
      // the in-process impl, which gates on declared permissions
      // anyway. Logged so operators notice.
      logger.warn('Unknown plugin trust_level; defaulting to in-process API', {
        plugin: plugin.name,
        trust_level: plugin.trust_level,
      });
      return createPluginAPI(plugin);
  }
}
