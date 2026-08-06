/**
 * Plugin Loader
 *
 * Loads enabled plugins and manages their lifecycle.
 * Plugins are loaded from the backend and their components are registered with the UI slot system.
 */

import { ref, shallowRef, reactive, type ShallowRef } from 'vue';
import pluginService from '@nosdesk/core/services/pluginService';
import { useDateStore } from '@nosdesk/core/stores/dateStore';
import { resolvePluginI18n } from '@nosdesk/core/utils/pluginI18n';
import { onSyncActions } from '@nosdesk/core/sync/observers';
import type { SyncAction } from '@nosdesk/core/sync/types';
import { logger } from '@nosdesk/core/utils/logger';
import { translate } from '@/i18n';
import type { Plugin, PluginSlot, PluginManifest, SlotChrome } from '@nosdesk/core/types/plugin';
import { canonicalSlotName, getSlot } from '@nosdesk/core/types/plugin';

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
  /** Resolved host chrome: the component's override, else the slot default. */
  chrome: SlotChrome;
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
    // Normalize the manifest's slot (canonical dotted name or legacy alias) to
    // its canonical name so registrations key consistently and mounts query by
    // the dotted target regardless of which form the manifest declared.
    const slot = canonicalSlotName(config.slot);
    if (!slot) {
      logger.warn(
        `Skipping plugin component with unknown slot: ${config.slot}`,
        { plugin: plugin.name, componentName }
      );
      continue;
    }

    // Resolve %key% labels against the manifest's i18n tables at load time.
    const l10n = (v: string | undefined): string | undefined =>
      v == null ? v : resolvePluginI18n(v, plugin.manifest.i18n, useDateStore().locale);

    // Resolve the chrome once, here, so every mount reads one field instead
    // of re-deriving the component-override-else-slot-default rule. `getSlot`
    // is total for a canonicalised name, but fall back to 'none' rather than
    // assert: an unwrapped panel is a smaller failure than a thrown mount.
    const chrome: SlotChrome = config.chrome ?? getSlot(slot)?.chrome ?? 'none';

    const registration: PluginSlotRegistration = {
      pluginUuid: plugin.uuid,
      pluginName: plugin.name,
      componentName,
      label: l10n(config.label),
      icon: config.icon,
      context: config.context || [],
      chrome,
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
        label: l10n(config.action.label) ?? config.action.label,
        // Per-component icon comes from the manifest's component
        // config (small inline glyph or override). Plugin-level
        // identity icon is served from /api/plugins/<uuid>/icon and
        // referenced separately by the plugin list views.
        icon: config.icon,
        componentLabel: l10n(config.label),
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
 * Reconcile the loaded set against the server's enabled list: unload plugins that
 * are no longer enabled (disabled / quarantined / uninstalled) and load any newly
 * enabled ones. `/plugins/enabled` returns only `installed` plugins, so a single
 * refetch + diff covers every lifecycle transition. Idempotent; safe to call
 * repeatedly.
 */
export async function reconcileEnabledPlugins(): Promise<void> {
  let enabled: Plugin[];
  try {
    enabled = await pluginService.listEnabledPlugins();
  } catch (error) {
    logger.warn('Plugin reconcile: failed to fetch enabled list', { error });
    return;
  }
  const enabledByUuid = new Set(enabled.map(p => p.uuid));

  // Unload anything no longer enabled — this tears down its sandbox frame in
  // EVERY open session, not just the admin tab that flipped the state, so a
  // disabled or signer-revoked (quarantined) plugin actually stops running.
  for (const uuid of [...loadedPlugins.value.keys()]) {
    if (!enabledByUuid.has(uuid)) {
      unloadPlugin(uuid);
    }
  }
  // Load anything newly enabled (re-enabled or freshly installed).
  for (const plugin of enabled) {
    if (!loadedPlugins.value.has(plugin.uuid)) {
      await loadPlugin(plugin);
    }
  }
}

const PLUGIN_LIFECYCLE_EVENTS = new Set([
  'plugin.installed',
  'plugin.updated',
  'plugin.uninstalled',
]);

let lifecycleUnsub: (() => void) | null = null;
let reconcileTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Subscribe to plugin lifecycle sync actions so every session reconciles its
 * loaded plugins as their state changes server-side — not only the acting admin's
 * tab. Debounced to coalesce bursts. Returns a cleanup fn; idempotent.
 */
export function startPluginLifecycleSync(): () => void {
  if (lifecycleUnsub) return lifecycleUnsub;
  const unsub = onSyncActions((actions: SyncAction[]) => {
    if (!actions.some(a => PLUGIN_LIFECYCLE_EVENTS.has(a.event_type))) return;
    if (reconcileTimer) clearTimeout(reconcileTimer);
    reconcileTimer = setTimeout(() => {
      reconcileTimer = null;
      void reconcileEnabledPlugins();
    }, 400);
  });
  lifecycleUnsub = () => {
    unsub();
    if (reconcileTimer) clearTimeout(reconcileTimer);
    reconcileTimer = null;
    lifecycleUnsub = null;
  };
  return lifecycleUnsub;
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
