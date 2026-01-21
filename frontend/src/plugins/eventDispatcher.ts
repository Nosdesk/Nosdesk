/**
 * Plugin Event Dispatcher
 *
 * Bridges SSE events to plugin event handlers.
 * Maps backend event types to plugin event format.
 */

import { useSSE, type SSEEventType } from '@/services/sseService';
import { getLoadedPlugins } from './loader';
import { createPluginAPI } from './api';
import { logger } from '@/utils/logger';
import type { PluginEvent } from '@/types/plugin';

// =============================================================================
// Event Mapping
// =============================================================================

/**
 * Map SSE event types to plugin event types
 */
const SSE_TO_PLUGIN_EVENT: Partial<Record<SSEEventType, PluginEvent>> = {
  'ticket-created': 'ticket:created',
  'ticket-updated': 'ticket:updated',
  'comment-added': 'ticket:comment_added',
  'device-created': 'device:created',
  'device-updated': 'device:updated',
  'documentation-created': 'document:created',
  'documentation-updated': 'document:updated',
};

/**
 * Map ticket update fields to specialized plugin events
 * These are derived from ticket-updated events based on the field changed
 */
const TICKET_FIELD_TO_EVENT: Record<string, PluginEvent> = {
  'status': 'ticket:status_changed',
  'assignee': 'ticket:assigned',
  'assigned_to': 'ticket:assigned',
};

/**
 * Plugin events restricted from community plugins
 */
const RESTRICTED_EVENTS: PluginEvent[] = [
  'device:created',
  'device:updated',
];

// =============================================================================
// Event Dispatcher
// =============================================================================

// Cached plugin APIs for event dispatch
const pluginApis = new Map<string, ReturnType<typeof createPluginAPI>>();

// Cleanup function reference
let cleanupFn: (() => void) | null = null;

/**
 * Initialize the event dispatcher.
 * Call this after plugins are loaded.
 *
 * @returns Cleanup function to stop dispatching
 */
export function initializeEventDispatcher(): () => void {
  // Prevent double initialization
  if (cleanupFn) {
    logger.warn('Event dispatcher already initialized');
    return cleanupFn;
  }

  const { addEventListener, removeEventListener } = useSSE();

  // Build plugin API cache
  pluginApis.clear();
  for (const { plugin } of getLoadedPlugins()) {
    pluginApis.set(plugin.uuid, createPluginAPI(plugin));
  }

  logger.info('Event dispatcher initialized', {
    pluginCount: pluginApis.size,
    plugins: Array.from(pluginApis.keys()),
  });

  // Create handlers for each mapped event type
  const handlers = new Map<SSEEventType, (data: unknown) => void>();

  for (const [sseEvent, pluginEvent] of Object.entries(SSE_TO_PLUGIN_EVENT)) {
    const handler = (data: unknown) => {
      dispatchToPlugins(pluginEvent as PluginEvent, data);

      // For ticket-updated, also check if we need to dispatch specialized events
      if (sseEvent === 'ticket-updated' && data && typeof data === 'object') {
        const eventData = data as { data?: { field?: string } };
        const field = eventData.data?.field;
        if (field && TICKET_FIELD_TO_EVENT[field]) {
          dispatchToPlugins(TICKET_FIELD_TO_EVENT[field], data);
        }
      }
    };

    handlers.set(sseEvent as SSEEventType, handler);
    addEventListener(sseEvent as SSEEventType, handler);
  }

  // Return cleanup function
  cleanupFn = () => {
    for (const [sseEvent, handler] of handlers) {
      removeEventListener(sseEvent, handler);
    }
    handlers.clear();
    pluginApis.clear();
    cleanupFn = null;
    logger.debug('Event dispatcher cleaned up');
  };

  return cleanupFn;
}

/**
 * Dispatch an event to all plugin handlers
 */
function dispatchToPlugins(event: PluginEvent, data: unknown): void {
  const isRestricted = RESTRICTED_EVENTS.includes(event);

  for (const [uuid, api] of pluginApis) {
    // Skip restricted events for community plugins
    if (isRestricted) {
      const loadedPlugins = getLoadedPlugins();
      const loadedPlugin = loadedPlugins.find(p => p.plugin.uuid === uuid);
      if (loadedPlugin?.plugin.trust_level === 'community') {
        continue;
      }
    }

    // Get and call all handlers for this event
    const handlers = api._getEventHandlers(event);

    for (const handler of handlers) {
      try {
        const result = handler(data);
        // Handle async handlers
        if (result instanceof Promise) {
          result.catch((error) => {
            logger.error(`Plugin ${uuid} async handler error for ${event}`, { error });
          });
        }
      } catch (error) {
        logger.error(`Plugin ${uuid} handler error for ${event}`, { error });
      }
    }
  }
}

/**
 * Refresh the plugin API cache (call after plugins are reloaded)
 */
export function refreshPluginApis(): void {
  pluginApis.clear();
  for (const { plugin } of getLoadedPlugins()) {
    pluginApis.set(plugin.uuid, createPluginAPI(plugin));
  }
  logger.debug('Plugin APIs refreshed', { count: pluginApis.size });
}

/**
 * Check if the event dispatcher is initialized
 */
export function isEventDispatcherInitialized(): boolean {
  return cleanupFn !== null;
}

/**
 * Stop the event dispatcher
 */
export function stopEventDispatcher(): void {
  if (cleanupFn) {
    cleanupFn();
  }
}
