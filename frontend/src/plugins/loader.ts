/**
 * Plugin Loader
 *
 * Loads enabled plugins and manages their lifecycle.
 * Plugins are loaded from the backend and their components are registered with the UI slot system.
 */

import { ref, shallowRef, reactive, type ShallowRef } from 'vue';
import pluginService from '@/services/pluginService';
import { logger } from '@/utils/logger';
import type { Plugin, PluginSlot, PluginManifest } from '@/types/plugin';

// =============================================================================
// Types
// =============================================================================

export interface LoadedPlugin {
  plugin: Plugin;
  manifest: PluginManifest;
}

export interface PluginSlotRegistration {
  pluginUuid: string;
  pluginName: string;
  componentName: string;
  label?: string;
  icon?: string;
  context: string[];
}

// =============================================================================
// Plugin Loader State
// =============================================================================

// Loaded plugins
const loadedPlugins: ShallowRef<Map<string, LoadedPlugin>> = shallowRef(new Map());

// Slot registrations (slot name -> array of registered components)
// Using reactive() for deep reactivity with Map operations
const slotRegistrations = reactive(new Map<PluginSlot, PluginSlotRegistration[]>());

// Loading state
const isLoading = ref(false);
const loadError = ref<string | null>(null);

// =============================================================================
// Plugin Loader Functions
// =============================================================================

/**
 * Load all enabled plugins from the backend
 */
export async function loadPlugins(): Promise<void> {
  if (isLoading.value) {
    logger.warn('Plugin loader already loading');
    return;
  }

  isLoading.value = true;
  loadError.value = null;

  try {
    const enabledPlugins = await pluginService.listEnabledPlugins();

    // Clear existing registrations
    loadedPlugins.value = new Map();
    slotRegistrations.clear();

    for (const plugin of enabledPlugins) {
      try {
        await loadPlugin(plugin);
      } catch (error) {
        logger.error(`Failed to load plugin: ${plugin.name}`, { error });
        // Continue loading other plugins
      }
    }

    logger.info(`Loaded ${loadedPlugins.value.size} plugins`, {
      plugins: Array.from(loadedPlugins.value.keys()),
    });
  } catch (error) {
    logger.error('Failed to load plugins', { error });
    loadError.value = 'Failed to load plugins';
  } finally {
    isLoading.value = false;
  }
}

/**
 * Load a single plugin
 */
async function loadPlugin(plugin: Plugin): Promise<void> {
  const manifest = plugin.manifest;

  // Store the loaded plugin
  loadedPlugins.value.set(plugin.uuid, {
    plugin,
    manifest,
  });

  // Register components in slots
  for (const [componentName, config] of Object.entries(manifest.components)) {
    const slot = config.slot as PluginSlot;

    const registration: PluginSlotRegistration = {
      pluginUuid: plugin.uuid,
      pluginName: plugin.name,
      componentName,
      label: config.label,
      icon: config.icon,
      context: config.context || [],
    };

    const existing = slotRegistrations.get(slot) || [];
    slotRegistrations.set(slot, [...existing, registration]);

    logger.info(`Registered component in slot: ${slot}`, {
      pluginName: plugin.name,
      componentName,
      totalInSlot: slotRegistrations.get(slot)?.length,
    });
  }

  logger.debug(`Loaded plugin: ${plugin.name}`, {
    uuid: plugin.uuid,
    version: plugin.version,
    components: Object.keys(manifest.components),
  });
}

/**
 * Get all registrations for a slot
 */
export function getSlotRegistrations(slot: PluginSlot): PluginSlotRegistration[] {
  return slotRegistrations.get(slot) || [];
}

/**
 * Get a loaded plugin by UUID
 */
export function getLoadedPlugin(uuid: string): LoadedPlugin | undefined {
  return loadedPlugins.value.get(uuid);
}

/**
 * Get all loaded plugins
 */
export function getLoadedPlugins(): LoadedPlugin[] {
  return Array.from(loadedPlugins.value.values());
}

/**
 * Check if plugins are currently loading
 */
export function isPluginsLoading(): boolean {
  return isLoading.value;
}

/**
 * Get the load error if any
 */
export function getLoadError(): string | null {
  return loadError.value;
}

// =============================================================================
// Reactive State Exports
// =============================================================================

export {
  loadedPlugins,
  slotRegistrations,
  isLoading,
  loadError,
};
