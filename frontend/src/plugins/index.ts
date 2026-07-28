/**
 * Plugin System
 *
 * Entry point for the Nosdesk plugin system: the loader, the API factory, and
 * the event dispatcher.
 */

// Plugin Loader
export {
  loadPlugins,
  reconcileEnabledPlugins,
  startPluginLifecycleSync,
  getSlotRegistrations,
  getLoadedPlugin,
  getLoadedPlugins,
  isPluginsLoading,
  getLoadError,
  loadedPlugins,
  slotRegistrations,
  isLoading,
  loadError,
} from './loader';

// Plugin API (createPluginAPI backs the sandbox host bridge; there is no
// in-process render path any more, so getHostApiForPlugin + the component loader
// were removed in the sandbox-all migration).
export { createPluginAPI, type PluginAPI } from './api';

// Event Dispatcher
export {
  initializeEventDispatcher,
  isEventDispatcherInitialized,
  stopEventDispatcher,
} from './eventDispatcher';

// PluginSlot is imported directly from './components/PluginSlot.vue' at its two
// mount sites; it isn't re-exported here to avoid an unused barrel entry.
