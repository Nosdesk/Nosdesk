/**
 * Plugin Loader
 *
 * Loads enabled plugins and manages their lifecycle.
 * Plugins are loaded from the backend and their components are registered with the UI slot system.
 */

import { ref, shallowRef, reactive, type ShallowRef } from 'vue';
import pluginService from '@/services/pluginService';
import { logger } from '@nosdesk/core/utils/logger';
import { translate } from '@/i18n';
import { preloadPluginBundle } from './componentLoader';
import type { Plugin, PluginSlot, PluginManifest } from '@nosdesk/core/types/plugin';
import { PLUGIN_SLOTS } from '@nosdesk/core/types/plugin';

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

export interface PluginActionRegistration {
  pluginUuid: string;
  pluginName: string;
  componentName: string;
  slot: PluginSlot;
  label: string;
  icon?: string;
  componentLabel?: string;
}

// =============================================================================
// Plugin Loader State
// =============================================================================

// Loaded plugins
const loadedPlugins: ShallowRef<Map<string, LoadedPlugin>> = shallowRef(new Map());

// Slot registrations (slot name -> array of registered components)
// Using reactive() for deep reactivity with Map operations
const slotRegistrations = reactive(new Map<PluginSlot, PluginSlotRegistration[]>());

// Action registrations (slot name -> array of plugin actions for the "+ Add" menu)
const actionRegistrations = reactive(new Map<PluginSlot, PluginActionRegistration[]>());

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
    actionRegistrations.clear();

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
    loadError.value = translate('plugin-loader-error', undefined, 'Failed to load plugins');
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

  // Register components in slots. v1 only honors `kind === "slot"`
  // (default when omitted); reserved kinds like `admin_page` /
  // `worker` are quietly skipped here so a forward-looking manifest
  // doesn't break the dispatcher. The backend already refuses to
  // INSTALL such plugins on this version, so this is a belt-and-
  // suspenders skip in case we ever loosen the install gate.
  for (const [componentName, config] of Object.entries(manifest.components)) {
    if (config.kind && config.kind !== 'slot') {
      logger.debug(
        `Skipping plugin component with unsupported kind: ${config.kind}`,
        { plugin: plugin.name, componentName }
      );
      continue;
    }
    // Defence-in-depth: the manifest validator already gates this
    // server-side, but we don't want a typoed slot to register a
    // garbage entry that no host template will ever mount. Skip
    // and log instead.
    if (!(config.slot in PLUGIN_SLOTS)) {
      logger.warn(
        `Skipping plugin component with unknown slot: ${config.slot}`,
        { plugin: plugin.name, componentName }
      );
      continue;
    }
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

    // Register action if the component defines one
    if (config.action) {
      const actionReg: PluginActionRegistration = {
        pluginUuid: plugin.uuid,
        pluginName: plugin.name,
        componentName,
        slot,
        label: config.action.label,
        // Per-component icon comes from the manifest's component
        // config (small inline glyph or override). Plugin-level
        // identity icon is served from /api/plugins/<uuid>/icon and
        // referenced separately by the plugin list views.
        icon: config.icon,
        componentLabel: config.label,
      };

      const existingActions = actionRegistrations.get(slot) || [];
      actionRegistrations.set(slot, [...existingActions, actionReg]);
    }

    logger.info(`Registered component in slot: ${slot}`, {
      pluginName: plugin.name,
      componentName,
      totalInSlot: slotRegistrations.get(slot)?.length,
    });
  }

  // Preload bundle so components render instantly (no async loading flash)
  if (plugin.trust_level !== 'community' && plugin.bundle_uploaded_at) {
    await preloadPluginBundle(plugin.uuid);
  }

  logger.debug(`Loaded plugin: ${plugin.name}`, {
    uuid: plugin.uuid,
    version: plugin.version,
    components: Object.keys(manifest.components),
  });
}

/**
 * Tear down everything we registered for a plugin uuid: slot
 * contributions, action contributions, and the loaded-plugin
 * entry. Called when a plugin is disabled or uninstalled so the
 * UI stops surfacing it without a full page reload. Idempotent;
 * safe to call on a plugin that wasn't loaded.
 */
export function unloadPlugin(uuid: string): void {
  if (!loadedPlugins.value.has(uuid)) {
    return;
  }

  const next = new Map(loadedPlugins.value);
  next.delete(uuid);
  loadedPlugins.value = next;

  for (const [slot, regs] of slotRegistrations.entries()) {
    const filtered = regs.filter(r => r.pluginUuid !== uuid);
    if (filtered.length === 0) {
      slotRegistrations.delete(slot);
    } else if (filtered.length !== regs.length) {
      slotRegistrations.set(slot, filtered);
    }
  }

  for (const [slot, regs] of actionRegistrations.entries()) {
    const filtered = regs.filter(r => r.pluginUuid !== uuid);
    if (filtered.length === 0) {
      actionRegistrations.delete(slot);
    } else if (filtered.length !== regs.length) {
      actionRegistrations.set(slot, filtered);
    }
  }

  logger.info(`Unloaded plugin: ${uuid}`);
}

/**
 * Get all registrations for a slot
 */
export function getSlotRegistrations(slot: PluginSlot): PluginSlotRegistration[] {
  return slotRegistrations.get(slot) || [];
}

/**
 * Get all action registrations for a slot (for the unified "+ Add" menu)
 */
export function getActionRegistrations(slot: PluginSlot): PluginActionRegistration[] {
  return actionRegistrations.get(slot) || [];
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
  actionRegistrations,
  isLoading,
  loadError,
};
