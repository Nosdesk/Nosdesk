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
import type { Plugin, PluginProxyRequest, PluginEvent } from '@/types/plugin';
import type { Ticket } from '@/types/ticket';
import type { Device } from '@/types/device';

// =============================================================================
// Types
// =============================================================================

export interface PluginComment {
  content: string;
  metadata?: Record<string, unknown>;
}

export interface PluginContext {
  ticket: Ticket | null;
  device: Device | null;
}

export type EventHandler = (data: unknown) => void | Promise<void>;

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

    // === INTEGRATE: External services ===
    // Note: Permission validation is handled by the backend proxy service
    async fetch(url: string, options?: RequestInit): Promise<Response | null> {
      try {
        const request: PluginProxyRequest = {
          url,
          method: (options?.method as PluginProxyRequest['method']) || 'GET',
          headers: options?.headers as Record<string, string>,
          body: options?.body ? JSON.parse(options.body as string) : undefined,
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

  // External requests
  fetch(url: string, options?: RequestInit): Promise<Response | null>;

  // Plugin storage
  storage: {
    get<T>(key: string): Promise<T | null>;
    set<T>(key: string, value: T): Promise<boolean>;
    delete(key: string): Promise<boolean>;
  };

  // Event subscription
  on(event: PluginEvent, handler: EventHandler): () => void;

  // User feedback
  notify(message: string, type?: 'info' | 'success' | 'warning' | 'error'): void;

  // Current context
  context: PluginContext;

  // Internal methods (not for plugin use)
  _setContext(context: Partial<PluginContext>): void;
  _getEventHandlers(event: PluginEvent): EventHandler[];
}
