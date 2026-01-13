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

// UI Components
export { default as PluginSlot } from './components/PluginSlot.vue';
