/**
 * Plugin System
 *
 * Entry point for the Nosdesk plugin system.
 * Exports the loader, API factory, and UI components.
 */

// Plugin Loader
export {
  loadPlugins,
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

// Plugin API
export { createPluginAPI, type PluginAPI } from './api';

// Component Loader
export {
  createPluginComponent,
  canRenderComponent,
  clearPluginCache,
  clearAllPluginCaches,
} from './componentLoader';

// Event Dispatcher
export {
  initializeEventDispatcher,
  refreshPluginApis,
  isEventDispatcherInitialized,
  stopEventDispatcher,
} from './eventDispatcher';

// UI Components
export { default as PluginSlot } from './components/PluginSlot.vue';
export { default as PluginLoading } from './components/PluginLoading.vue';
export { default as PluginError } from './components/PluginError.vue';
