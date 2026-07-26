/**
 * Plugin System
 *
 * Entry point for the Nosdesk plugin system.
 * Exports the loader, API factory, and UI components.
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

// UI Components
export { default as PluginSlot } from './components/PluginSlot.vue';
export { default as PluginLoading } from './components/PluginLoading.vue';
export { default as PluginError } from './components/PluginError.vue';
