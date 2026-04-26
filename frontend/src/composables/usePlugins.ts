/**
 * Plugin list state for the admin views.
 *
 * Owns the installed-plugin list, exposes the lifecycle
 * mutations (toggle, uninstall) as async actions, and tears
 * down the loader's slot/event registrations on state changes
 * so the UI never lags behind the DB after a transition.
 *
 * Returns reactive refs the views can bind directly. Keeps the
 * views focused on layout and presentation; the data plumbing
 * lives here.
 */
import { ref, readonly } from 'vue';
import pluginService from '@/services/pluginService';
import { unloadPlugin } from '@/plugins/loader';
import { logger } from '@/utils/logger';
import type { Plugin } from '@/types/plugin';

export function usePlugins() {
  const plugins = ref<Plugin[]>([]);
  const loading = ref(true);
  const error = ref<string | null>(null);

  async function load(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      plugins.value = await pluginService.listPlugins();
    } catch (e) {
      error.value = 'Failed to load plugins';
      logger.error('Failed to load plugins', { error: e });
    } finally {
      loading.value = false;
    }
  }

  function replace(updated: Plugin): void {
    const i = plugins.value.findIndex((p) => p.uuid === updated.uuid);
    if (i !== -1) plugins.value[i] = updated;
  }

  function remove(uuid: string): void {
    plugins.value = plugins.value.filter((p) => p.uuid !== uuid);
  }

  /**
   * Toggle a plugin between Installed and Disabled. Quarantined
   * and Uninstalled rows are not reachable through this path
   * (the backend lifecycle gate would refuse the transition with
   * a 409); the UI hides the toggle for those states upstream.
   */
  async function toggle(plugin: Plugin): Promise<void> {
    if (plugin.state !== 'installed' && plugin.state !== 'disabled') return;
    const enable = plugin.state === 'disabled';
    try {
      const updated = await pluginService.updatePlugin(plugin.uuid, { enabled: enable });
      replace(updated);
      // Disable always tears down the loader's slot/event/action
      // registrations so the UI stops surfacing the plugin
      // immediately. Re-enable runs through the next loader pass
      // on view mount; live re-add would need slot-aware reload.
      if (updated.state !== 'installed') {
        unloadPlugin(updated.uuid);
      }
    } catch (e) {
      logger.error('Failed to toggle plugin', { error: e, plugin: plugin.uuid });
      throw e;
    }
  }

  async function uninstall(plugin: Plugin): Promise<void> {
    try {
      await pluginService.uninstallPlugin(plugin.uuid);
      remove(plugin.uuid);
      unloadPlugin(plugin.uuid);
    } catch (e) {
      logger.error('Failed to uninstall plugin', { error: e, plugin: plugin.uuid });
      throw e;
    }
  }

  return {
    plugins: readonly(plugins),
    loading: readonly(loading),
    error: readonly(error),
    load,
    toggle,
    uninstall,
    replace,
  };
}
