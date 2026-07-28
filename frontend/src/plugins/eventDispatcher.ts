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
import { getLoadedPlugin } from './loader';
import { forEachLiveInstance } from './pluginInstances';
import { effectivePermissions } from './permissions';
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

  logger.info('Event dispatcher initialized');

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
    cleanupFn = null;
    logger.debug('Event dispatcher cleaned up');
  };

  return cleanupFn;
}

/**
 * Dispatch an event to every live plugin instance's handlers.
 *
 * Iterates the live-instance registry (populated by `PluginSandboxFrame` via
 * `createHostApiImpl`), so handlers land on the in-process instance the plugin's
 * `api.on` registered on — its `_getEventHandlers` returns the wrappers that
 * forward each event across the bridge to the sandboxed plugin.
 */
/**
 * The read permission an event requires — event payloads carry the entity's
 * projection in `data`, so subscribing must be gated on the matching read grant
 * (else a zero-permission plugin could observe ticket/asset data). `document:*`
 * has no read permission yet, so it fails closed (returns null -> denied).
 */
function requiredReadPermission(event: PluginEvent): string | null {
  if (event.startsWith('ticket:')) return 'ticket:read';
  if (event.startsWith('asset:')) return 'asset:read';
  return null;
}

function dispatchToPlugins(event: PluginEvent, data: unknown): void {
  const isRestricted = RESTRICTED_EVENTS.includes(event);
  const needed = requiredReadPermission(event);

  forEachLiveInstance((uuid, api) => {
    const loaded = getLoadedPlugin(uuid);
    if (!loaded) return;
    // Skip restricted events for community plugins.
    if (isRestricted && loaded.plugin.trust_level === 'community') {
      return;
    }
    // Gate on the matching read permission from the effective (consented) set,
    // fail closed.
    const granted = effectivePermissions(loaded.plugin);
    if (!needed || !granted.includes(needed)) {
      return;
    }

    for (const handler of api._getEventHandlers(event)) {
      try {
        const result = handler(data);
        if (result instanceof Promise) {
          result.catch((error) => {
            logger.error(`Plugin ${uuid} async handler error for ${event}`, { error });
          });
        }
      } catch (error) {
        logger.error(`Plugin ${uuid} handler error for ${event}`, { error });
      }
    }
  });
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
