/**
 * Plugin API
 *
 * The API exposed to plugins for interacting with Nosdesk.
 * Each plugin gets its own sandboxed instance of this API.
 */

import pluginService from '@/services/pluginService';
import { getTicketById, getTickets, addCommentToTicket } from '@/services/ticketService';
import { getDeviceById, getDevices } from '@/services/deviceService';
import { logger } from '@/utils/logger';
import type { Plugin, PluginProxyRequest, PluginEvent, CollectionRow, CollectionListResponse } from '@/types/plugin';
import type { Ticket } from '@/types/ticket';
import type { Device } from '@/types/device';

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
  device: Device | null;
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
  const permissions = new Set(plugin.manifest.permissions);
  const eventHandlers = new Map<PluginEvent, EventHandler[]>();

  // Check if plugin has a specific permission
  const hasPermission = (permission: string): boolean => {
    return permissions.has(permission);
  };

  // Current context (set by the UI slot system)
  let context: PluginContext = {
    ticket: null,
    device: null,
  };

  const api: PluginAPI = {
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
        if (!hasPermission('tickets:read')) {
          logger.warn(`Plugin ${plugin.name} denied tickets:read permission`);
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
        if (!hasPermission('tickets:read')) {
          logger.warn(`Plugin ${plugin.name} denied tickets:read permission`);
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
        if (!hasPermission('tickets:comment')) {
          logger.warn(`Plugin ${plugin.name} denied tickets:comment permission`);
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
      async get(id: number): Promise<Device | null> {
        if (!hasPermission('devices:read')) {
          logger.warn(`Plugin ${plugin.name} denied devices:read permission`);
          return null;
        }
        try {
          return await getDeviceById(id);
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to get device`, { id, error });
          return null;
        }
      },
      async list(): Promise<Device[]> {
        if (!hasPermission('devices:read')) {
          logger.warn(`Plugin ${plugin.name} denied devices:read permission`);
          return [];
        }
        try {
          return await getDevices();
        } catch (error) {
          logger.error(`Plugin ${plugin.name} failed to list devices`, { error });
          return [];
        }
      },
    },

    // === ATTACHMENTS: Access ticket attachments ===
    attachments: {
      async list(ticketId: number): Promise<PluginAttachment[]> {
        if (!hasPermission('tickets:read')) {
          logger.warn(`Plugin ${plugin.name} denied tickets:read permission`);
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
        if (!hasPermission('tickets:read')) {
          logger.warn(`Plugin ${plugin.name} denied tickets:read permission`);
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
        if (!hasPermission('tickets:read')) {
          logger.warn(`Plugin ${plugin.name} denied tickets:read permission`);
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
    // Note: Permission validation is handled by the backend proxy service
    async fetch(url: string, options?: PluginFetchOptions): Promise<Response | null> {
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
        if (!hasPermission('storage')) {
          logger.warn(`Plugin ${plugin.name} denied storage permission`);
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
        if (!hasPermission('storage')) {
          logger.warn(`Plugin ${plugin.name} denied storage permission`);
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
        if (!hasPermission('storage')) {
          logger.warn(`Plugin ${plugin.name} denied storage permission`);
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
        const hasCollectionRead = hasPermission('collections') || hasPermission('collections:read');
        const hasCollectionWrite = hasPermission('collections') || hasPermission('collections:write');

        return {
          async create(data: Record<string, unknown>): Promise<CollectionRow | null> {
            if (!hasCollectionWrite) {
              logger.warn(`Plugin ${plugin.name} denied collections:write permission`);
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
              logger.warn(`Plugin ${plugin.name} denied collections:read permission`);
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
              logger.warn(`Plugin ${plugin.name} denied collections:write permission`);
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
              logger.warn(`Plugin ${plugin.name} denied collections:write permission`);
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
              logger.warn(`Plugin ${plugin.name} denied collections:read permission`);
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
    notify(message: string, type: 'info' | 'success' | 'warning' | 'error' = 'info'): void {
      // TODO: Integrate with notification toast system
      logger.info(`Plugin notification [${type}]: ${message}`, { plugin: plugin.name });
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
    get(id: number): Promise<Device | null>;
    list(): Promise<Device[]>;
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
