/**
 * Plugin Event Dispatcher
 *
 * Bridges the sync change-stream to plugin event handlers. Subscribes
 * to the object pool's `onSyncActions` observer (the same stream that
 * drives the pool) rather than discrete SSE events, so the plugin
 * surface rides one canonical event source. Maps each sync action's
 * `event_type` to the plugin event format.
 */

import { onSyncActions } from '@nosdesk/core/sync/observers';
import type { SyncAction } from '@nosdesk/core/sync/types';
import { getLoadedPlugins } from './loader';
import { getHostApiForPlugin } from './api';
import { logger } from '@nosdesk/core/utils/logger';
import type { PluginEvent } from '@nosdesk/core/types/plugin';

// =============================================================================
// Event Mapping
// =============================================================================

/**
 * Map a sync action's `event_type` to the plugin event(s) it fires.
 * State/assignee changes fire both their specialized event and the
 * generic `ticket:updated`, preserving the pre-sync dispatcher's
 * dual-fire behaviour.
 */
function pluginEventsFor(eventType: string): PluginEvent[] {
  switch (eventType) {
    case 'ticket.created':
      return ['ticket:created'];
    case 'ticket.workflow_state_changed':
      return ['ticket:status_changed', 'ticket:updated'];
    case 'ticket.assignee_changed':
      return ['ticket:assigned', 'ticket:updated'];
    case 'ticket.updated':
    case 'ticket.priority_changed':
    case 'ticket.title_changed':
    case 'ticket.category_changed':
    case 'ticket.verification_changed':
      return ['ticket:updated'];
    case 'comment.created':
      return ['ticket:comment_added'];
    case 'asset.created':
      return ['asset:created'];
    case 'asset.updated':
      return ['asset:updated'];
    case 'documentation_page.created':
      return ['document:created'];
    case 'documentation_page.metadata_changed':
    case 'documentation_page.verified':
      return ['document:updated'];
    default:
      return [];
  }
}

/**
 * Plugin events restricted from community plugins
 */
const RESTRICTED_EVENTS: PluginEvent[] = [
  'asset:created',
  'asset:updated',
];

// =============================================================================
// Event Dispatcher
// =============================================================================

// Cached plugin APIs for event dispatch
const pluginApis = new Map<string, ReturnType<typeof getHostApiForPlugin>>();

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

  // Build plugin API cache
  pluginApis.clear();
  for (const { plugin } of getLoadedPlugins()) {
    pluginApis.set(plugin.uuid, getHostApiForPlugin(plugin));
  }

  logger.info('Event dispatcher initialized', {
    pluginCount: pluginApis.size,
    plugins: Array.from(pluginApis.keys()),
  });

  // Subscribe to the sync change-stream. Each action maps to zero or
  // more plugin events; the action itself is the payload so handlers
  // get event_type + aggregate_id + the entity projection in `data`.
  const unsubscribe = onSyncActions((actions: SyncAction[]) => {
    for (const action of actions) {
      for (const pluginEvent of pluginEventsFor(action.event_type)) {
        dispatchToPlugins(pluginEvent, action);
      }
    }
  });

  cleanupFn = () => {
    unsubscribe();
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
    pluginApis.set(plugin.uuid, getHostApiForPlugin(plugin));
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
