/**
 * Plugin API
 *
 * Defines the `PluginAPI` interface (the host-side contract) and its
 * implementation, `createPluginAPI`. Every plugin now runs in the opaque-origin
 * sandbox, so this impl is never handed to a plugin directly: `createHostApiImpl`
 * (sandboxHostApi.ts) wraps it behind the Comlink bridge, and every permission
 * check + service call still runs here, in-process, on the host side. The old
 * in-process render path (and the `getHostApiForPlugin` tier dispatch) was
 * removed in the sandbox-all migration.
 */

import pluginService from '@nosdesk/core/services/pluginService';
import { getTicketById, getTickets, addCommentToTicket, updateTicket, deleteTicket } from '@nosdesk/core/services/ticketService';
import { getAssetById, getAssets, updateAsset } from '@/services/assetService';
import userService from '@/services/userService';
import { useAuthStore } from '@/stores/auth';
import { logger } from '@nosdesk/core/utils/logger';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates';
import { PRIORITY_OPTIONS } from '@nosdesk/core/constants/ticketOptions';
import { PluginApiError } from '@nosdesk/plugin-sdk';
import type {
  PluginUserQuery,
  PluginUserList,
  PluginWorkflowState,
  PluginPriority,
} from '@nosdesk/plugin-sdk';
import type { Plugin, PluginPermission, PluginProxyRequest, PluginEvent, CollectionRow, CollectionListResponse } from '@nosdesk/core/types/plugin';
import type { Ticket } from '@nosdesk/core/types/ticket';
import type { Asset } from '@nosdesk/core/types/asset';
import type { User } from '@nosdesk/core/types/user';

// =============================================================================
// Types
// =============================================================================

export interface PluginComment {
  content: string;
  /** Post as an internal note (hidden from the requester, not relayed). */
  is_internal?: boolean;
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

/** Writable-field subset a plugin may patch on a ticket (`ticket:write`). */
export interface PluginTicketPatch {
  title?: string;
  priority?: string;
  workflow_state_id?: number;
  assignee?: string | null;
}

/** Writable-field subset a plugin may patch on an asset (`asset:write`). */
export interface PluginAssetPatch {
  name?: string;
  status?: string;
  location?: string | null;
}

/** Safe user projection for `user:read` (identity only). */
export interface PluginUser {
  uuid: string;
  name: string;
  email: string;
  avatarUrl: string | null;
  role: string;
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
  // The effective grant is the CONSENTED set, not the raw manifest — an admin
  // may have approved a narrower scope, and the manifest can widen on update
  // ahead of re-consent. Fall back to the manifest only for legacy rows with no
  // consent recorded (`consented_permissions === null`). This mirrors the
  // backend's `Plugin::effective_permission_set`.
  const permissions = new Set<string>(
    plugin.consented_permissions ?? plugin.manifest.permissions,
  );
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

  // Attribution header on plugin-initiated writes: the backend records
  // `actor_ref = plugin:<uuid>` in the audit / sync trail so a plugin's writes
  // are traceable (the actor stays the user — the plugin acts as them).
  const pluginAttribution = { headers: { 'X-Nosdesk-Plugin': plugin.uuid } };

  // Unified failure reporting: denial and upstream failure THROW a typed
  // PluginApiError (the plugin catches + inspects `code`); `null` is reserved for
  // a genuine not-found. Both are `never` so callers don't need a trailing return.
  const denied = (perm: string): never => {
    logger.warn(`Plugin ${plugin.name} denied ${perm}`);
    throw new PluginApiError('denied', `${perm} not granted`);
  };
  const upstream = (what: string, error: unknown): never => {
    logger.error(`Plugin ${plugin.name} ${what}`, { error });
    throw new PluginApiError('upstream', what);
  };

  // Identity-only projection: never expose more of a user than name/email/avatar/
  // role, whatever the underlying record carries.
  const toPluginUser = (u: User): PluginUser => ({
    uuid: u.uuid,
    name: u.name,
    email: u.email,
    avatarUrl: u.avatar_url ?? null,
    role: (u.workspace_role ?? u.platform_role) as string,
  });

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
        if (!hasPermission('ticket:read')) denied('ticket:read');
        try {
          return await getTicketById(id);
        } catch (error) {
          throw upstream('failed to get ticket', error);
        }
      },
      async list(): Promise<Ticket[]> {
        if (!hasPermission('ticket:read')) denied('ticket:read');
        try {
          return await getTickets();
        } catch (error) {
          throw upstream('failed to list tickets', error);
        }
      },
      async addComment(ticketId: number, comment: PluginComment): Promise<boolean> {
        if (!hasPermission('ticket:comment')) denied('ticket:comment');
        try {
          await addCommentToTicket(
            ticketId,
            comment.content,
            [],
            comment.is_internal ?? false,
            undefined,
            pluginAttribution,
          );
          return true;
        } catch (error) {
          throw upstream('failed to add comment', error);
        }
      },
      // Acts as the current user: the write hits the same endpoint the app uses,
      // bounded by the user's own perms + RLS. Restrict to the patch's safe field
      // subset (never a full ticket).
      async update(id: number, patch: PluginTicketPatch): Promise<Ticket | null> {
        if (!hasPermission('ticket:write')) denied('ticket:write');
        try {
          return await updateTicket(id, patch as Partial<Ticket>, pluginAttribution);
        } catch (error) {
          throw upstream('failed to update ticket', error);
        }
      },
      async delete(id: number): Promise<boolean> {
        if (!hasPermission('ticket:delete')) denied('ticket:delete');
        try {
          await deleteTicket(id, pluginAttribution);
          return true;
        } catch (error) {
          throw upstream('failed to delete ticket', error);
        }
      },
      // Reference data so a plugin can build a valid `update({ workflow_state_id })`
      // without hardcoding ints. `ticket:read` gates it (it's workspace config).
      async workflowStates(): Promise<PluginWorkflowState[]> {
        if (!hasPermission('ticket:read')) denied('ticket:read');
        try {
          const states = await useWorkflowStatesStore().load();
          return states.map((s) => ({
            id: s.id,
            name: s.name,
            category: s.category,
            color: s.color,
            position: s.position,
            is_default: s.is_default,
          }));
        } catch (error) {
          throw upstream('failed to list workflow states', error);
        }
      },
      // The fixed priority scale (static; no gate).
      async priorities(): Promise<PluginPriority[]> {
        return PRIORITY_OPTIONS.map((o) => o.value);
      },
    },

    assets: {
      async get(id: number): Promise<Asset | null> {
        if (!hasPermission('asset:read')) denied('asset:read');
        try {
          return await getAssetById(id);
        } catch (error) {
          throw upstream('failed to get asset', error);
        }
      },
      async list(): Promise<Asset[]> {
        if (!hasPermission('asset:read')) denied('asset:read');
        try {
          return await getAssets();
        } catch (error) {
          throw upstream('failed to list assets', error);
        }
      },
      async update(id: number, patch: PluginAssetPatch): Promise<Asset | null> {
        if (!hasPermission('asset:write')) denied('asset:write');
        try {
          return await updateAsset(id, patch as Partial<Asset>, pluginAttribution);
        } catch (error) {
          throw upstream('failed to update asset', error);
        }
      },
    },

    // === READ: Users (identity projection) ===
    users: {
      // Ambient: the current user's own identity (the plugin runs in their
      // session). No permission gate — no more than `api.plugin` already implies.
      async me(): Promise<PluginUser | null> {
        const u = useAuthStore().user;
        return u ? toPluginUser(u as User) : null;
      },
      async get(uuid: string): Promise<PluginUser | null> {
        if (!hasPermission('user:read')) denied('user:read');
        try {
          const u = await userService.getUserByUuid(uuid);
          return u ? toPluginUser(u) : null;
        } catch (error) {
          throw upstream('failed to get user', error);
        }
      },
      async list(query: PluginUserQuery = {}): Promise<PluginUserList> {
        if (!hasPermission('user:read')) denied('user:read');
        try {
          const page = await userService.getPaginatedUsers({
            page: query.page ?? 1,
            pageSize: Math.min(query.limit ?? 25, 100),
            search: query.search || undefined,
            role: query.role,
            sortField: query.sortBy,
            sortDirection: query.sortOrder,
          });
          return { users: page.data.map(toPluginUser), total: page.total };
        } catch (error) {
          throw upstream('failed to list users', error);
        }
      },
    },

    // === ATTACHMENTS: Access ticket attachments ===
    attachments: {
      async list(ticketId: number): Promise<PluginAttachment[]> {
        if (!hasPermission('ticket:read')) denied('ticket:read');
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
          throw upstream('failed to list attachments', error);
        }
      },
      async getContent(attachmentId: number, ticketId: number): Promise<{ blob: Blob; name: string; mimeType: string } | null> {
        if (!hasPermission('ticket:read')) denied('ticket:read');
        try {
          const attachments = await api.attachments.list(ticketId);
          const att = attachments.find(a => a.id === attachmentId);
          if (!att) return null;
          const response = await fetch(att.url, { credentials: 'same-origin' });
          if (!response.ok) return null;
          const blob = await response.blob();
          return { blob, name: att.name, mimeType: att.mimeType || 'application/octet-stream' };
        } catch (error) {
          throw upstream('failed to get attachment content', error);
        }
      },
      async getBase64(attachmentId: number, ticketId: number): Promise<{ data: string; name: string; mimeType: string } | null> {
        if (!hasPermission('ticket:read')) denied('ticket:read');
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
          throw upstream('failed to get attachment base64', error);
        }
      },
    },

    // === INTEGRATE: External services ===
    // The backend proxy is the source of truth for network
    // permission enforcement, but we also fail closed here so a
    // denied plugin doesn't burn round-trips and the rejection
    // pattern is consistent with every other gated method.
    async fetch(url: string, options?: PluginFetchOptions): Promise<Response> {
      let parsed: URL;
      try {
        parsed = new URL(url);
      } catch {
        logger.warn(`Plugin ${plugin.name} fetch refused: invalid URL`, { url });
        throw new PluginApiError('invalid', `invalid fetch URL: ${url}`);
      }
      if (!hasNetworkPermission(parsed.host)) {
        denied(`network:${parsed.host}`);
      }

      try {
        const request: PluginProxyRequest = {
          url,
          method: (options?.method as PluginProxyRequest['method']) || 'GET',
          headers: options?.headers as Record<string, string>,
          // Pass the body through as-is; the backend encodes per `content_type`
          // ('json' default, or 'form'). Never JSON.parse a string body — a
          // pre-encoded form/text/XML string is not JSON and would throw. Only
          // null/undefined mean "no body" (so '' and 0 are preserved).
          body: options?.body ?? undefined,
          content_type: options?.content_type,
        };

        const response = await pluginService.proxyRequest(plugin.uuid, request);

        // Convert to Response-like object
        return new Response(JSON.stringify(response.body), {
          status: response.status,
          headers: response.headers,
        });
      } catch (error) {
          throw upstream('fetch failed', error);
        }
    },

    // === STORE: Plugin data ===
    storage: {
      async get<T>(key: string): Promise<T | null> {
        if (!hasPermission('storage:plugin')) denied('storage:plugin');
        try {
          const result = await pluginService.getStorage(plugin.uuid, key);
          return result.value as T;
        } catch (error) {
          throw upstream('failed to get storage', error);
        }
      },
      async set<T>(key: string, value: T): Promise<boolean> {
        if (!hasPermission('storage:plugin')) denied('storage:plugin');
        try {
          await pluginService.setStorage(plugin.uuid, { key, value });
          return true;
        } catch (error) {
          throw upstream('failed to set storage', error);
        }
      },
      async delete(key: string): Promise<boolean> {
        if (!hasPermission('storage:plugin')) denied('storage:plugin');
        try {
          await pluginService.deleteStorage(plugin.uuid, key);
          return true;
        } catch (error) {
          throw upstream('failed to delete storage', error);
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
            if (!hasCollectionWrite) denied('collection:write');
            try {
              return await pluginService.createCollectionRow(plugin.uuid, collectionName, data);
            } catch (error) {
          throw upstream('failed to create collection row', error);
        }
          },
          async get(uuid: string): Promise<CollectionRow | null> {
            if (!hasCollectionRead) denied('collection:read');
            try {
              return await pluginService.getCollectionRow(plugin.uuid, collectionName, uuid);
            } catch (error) {
          throw upstream('failed to get collection row', error);
        }
          },
          async update(uuid: string, data: Record<string, unknown>): Promise<CollectionRow | null> {
            if (!hasCollectionWrite) denied('collection:write');
            try {
              return await pluginService.updateCollectionRow(plugin.uuid, collectionName, uuid, data);
            } catch (error) {
          throw upstream('failed to update collection row', error);
        }
          },
          async delete(uuid: string): Promise<boolean> {
            if (!hasCollectionWrite) denied('collection:write');
            try {
              await pluginService.deleteCollectionRow(plugin.uuid, collectionName, uuid);
              return true;
            } catch (error) {
          throw upstream('failed to delete collection row', error);
        }
          },
          async list(params?: { limit?: number; offset?: number; filter?: string; sort_by?: string; sort_order?: string }): Promise<CollectionListResponse> {
            if (!hasCollectionRead) denied('collection:read');
            try {
              return await pluginService.listCollectionRows(plugin.uuid, collectionName, params);
            } catch (error) {
          throw upstream('failed to list collection rows', error);
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

    // === SETTINGS: The plugin's own admin-configured config ===
    settings: {
      async get(key: string): Promise<unknown | null> {
        try {
          const settings = await pluginService.getRuntimeSettings(plugin.uuid);
          const s = settings.find((x) => x.key === key);
          // A secret's value is redacted to null server-side; unset keys are
          // absent. Either way the plugin gets null, never the secret.
          return s ? (s.value ?? null) : null;
        } catch (error) {
          throw upstream('failed to get setting', error);
        }
      },
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
    update(id: number, patch: PluginTicketPatch): Promise<Ticket | null>;
    delete(id: number): Promise<boolean>;
    workflowStates(): Promise<PluginWorkflowState[]>;
    priorities(): Promise<PluginPriority[]>;
  };

  assets: {
    get(id: number): Promise<Asset | null>;
    list(): Promise<Asset[]>;
    update(id: number, patch: PluginAssetPatch): Promise<Asset | null>;
  };

  users: {
    me(): Promise<PluginUser | null>;
    get(uuid: string): Promise<PluginUser | null>;
    list(query?: PluginUserQuery): Promise<PluginUserList>;
  };

  // Attachment access
  attachments: {
    list(ticketId: number): Promise<PluginAttachment[]>;
    getContent(attachmentId: number, ticketId: number): Promise<{ blob: Blob; name: string; mimeType: string } | null>;
    getBase64(attachmentId: number, ticketId: number): Promise<{ data: string; name: string; mimeType: string } | null>;
  };

  // External requests
  fetch(url: string, options?: PluginFetchOptions): Promise<Response>;

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

  // The plugin's own admin-configured settings (secrets redacted)
  settings: {
    get(key: string): Promise<unknown | null>;
  };

  // Event subscription
  on(event: PluginEvent, handler: EventHandler): () => void;

  // User feedback
  notify(message: string, type?: 'info' | 'success' | 'warning' | 'error'): void;

  // UI helpers
  ui: PluginUIHelpers;

  // Internal: event handlers the dispatcher reads. Context is delivered to
  // sandboxed plugins by the frame (postInit/postContext), not through this
  // object, so there's no context getter / setter here any more.
  _getEventHandlers(event: PluginEvent): EventHandler[];
}

// `getHostApiForPlugin` (the tier-dispatch) was removed in the sandbox-all
// migration: every tier runs in the iframe sandbox, so the only consumer of
// `createPluginAPI` is `createHostApiImpl`, which wraps it behind the bridge.
