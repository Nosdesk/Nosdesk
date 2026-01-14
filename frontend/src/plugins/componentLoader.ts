/**
 * Plugin Component Loader
 *
 * Dynamically loads and caches plugin bundles (ES modules) from the backend.
 * Creates async Vue components that render plugin UI in slots.
 */

import { defineAsyncComponent, type Component } from 'vue';
import { getLoadedPlugin } from './loader';
import { logger } from '@/utils/logger';
import PluginLoading from './components/PluginLoading.vue';
import PluginError from './components/PluginError.vue';

// =============================================================================
// Types
// =============================================================================

export interface PluginModule {
  [componentName: string]: Component;
}

// =============================================================================
// Module Cache
// =============================================================================

// Cache plugin bundles to avoid re-fetching
const moduleCache = new Map<string, Promise<PluginModule>>();

// Track which plugins have failed to load
const failedPlugins = new Set<string>();

// =============================================================================
// Bundle Loading
// =============================================================================

/**
 * Load a plugin's bundle from the backend
 */
async function loadPluginBundle(pluginUuid: string): Promise<PluginModule> {
  const url = `/api/plugins/${pluginUuid}/bundle`;

  logger.debug(`Loading plugin bundle: ${pluginUuid}`, { url });

  try {
    // Dynamic import of the ES module bundle
    // @vite-ignore tells Vite to skip static analysis of this import
    const module = await import(/* @vite-ignore */ url);

    if (!module.default) {
      throw new Error('Plugin bundle must have a default export');
    }

    logger.info(`Loaded plugin bundle: ${pluginUuid}`, {
      components: Object.keys(module.default),
    });

    return module.default as PluginModule;
  } catch (error) {
    logger.error(`Failed to load plugin bundle: ${pluginUuid}`, { error });
    failedPlugins.add(pluginUuid);
    throw error;
  }
}

/**
 * Get a cached or fresh plugin bundle
 */
function getPluginBundle(pluginUuid: string): Promise<PluginModule> {
  if (!moduleCache.has(pluginUuid)) {
    moduleCache.set(pluginUuid, loadPluginBundle(pluginUuid));
  }
  return moduleCache.get(pluginUuid)!;
}

// =============================================================================
// Component Creation
// =============================================================================

/**
 * Create an async Vue component that loads from a plugin bundle.
 *
 * @param pluginUuid - The UUID of the plugin
 * @param componentName - The name of the component to load from the bundle
 * @returns An async Vue component
 */
export function createPluginComponent(
  pluginUuid: string,
  componentName: string
): Component {
  logger.debug(`Creating async component: ${componentName} for plugin ${pluginUuid}`);

  return defineAsyncComponent({
    loader: async () => {
      const loadedPlugin = getLoadedPlugin(pluginUuid);

      if (!loadedPlugin) {
        throw new Error(`Plugin not loaded: ${pluginUuid}`);
      }

      // Security: community plugins cannot load components
      if (loadedPlugin.plugin.trust_level === 'community') {
        throw new Error('Community plugins cannot render components');
      }

      if (!loadedPlugin.plugin.bundle_uploaded_at) {
        throw new Error('Plugin has no uploaded bundle');
      }

      const module = await getPluginBundle(pluginUuid);

      if (!module[componentName]) {
        throw new Error(`Component not found in bundle: ${componentName}`);
      }

      return module[componentName];
    },
    loadingComponent: PluginLoading,
    errorComponent: PluginError,
    timeout: 10000,
    delay: 200,
  });
}

/**
 * Check if a plugin can render components
 *
 * @param pluginUuid - The UUID of the plugin
 * @returns Whether the plugin can render components
 */
export function canRenderComponent(pluginUuid: string): boolean {
  const loadedPlugin = getLoadedPlugin(pluginUuid);

  if (!loadedPlugin) {
    return false;
  }

  // Must be official or verified
  if (loadedPlugin.plugin.trust_level === 'community') {
    return false;
  }

  // Must have a bundle
  if (!loadedPlugin.plugin.bundle_uploaded_at) {
    return false;
  }

  // Must not have previously failed
  if (failedPlugins.has(pluginUuid)) {
    return false;
  }

  return true;
}

/**
 * Clear the module cache for a specific plugin (e.g., after bundle update)
 */
export function clearPluginCache(pluginUuid: string): void {
  moduleCache.delete(pluginUuid);
  failedPlugins.delete(pluginUuid);
  logger.debug(`Cleared plugin cache: ${pluginUuid}`);
}

/**
 * Clear all cached plugin bundles
 */
export function clearAllPluginCaches(): void {
  moduleCache.clear();
  failedPlugins.clear();
  logger.debug('Cleared all plugin caches');
}
