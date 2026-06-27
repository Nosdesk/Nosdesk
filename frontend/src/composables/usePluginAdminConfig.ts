/**
 * Operator-controlled admin-UI flags for the plugins surface.
 * Cached at module scope so the list, registry, and sideload views
 * share a single fetch per session instead of each calling the
 * config endpoint on mount.
 */
import { ref } from 'vue';
import pluginService from '@nosdesk/core/services/pluginService';
import { logger } from '@nosdesk/core/utils/logger';

interface PluginAdminConfig {
  web_sideload_enabled: boolean;
  registry_enabled: boolean;
}

const config = ref<PluginAdminConfig | null>(null);
let inflight: Promise<PluginAdminConfig> | null = null;

async function load(): Promise<PluginAdminConfig> {
  if (config.value) return config.value;
  if (inflight) return inflight;
  inflight = pluginService
    .getAdminConfig()
    .then((c) => {
      config.value = c;
      return c;
    })
    .catch((err) => {
      logger.error('Failed to load plugin admin config', { error: err });
      // Fail closed so a transient endpoint failure doesn't
      // accidentally surface gated UI.
      const fallback: PluginAdminConfig = {
        web_sideload_enabled: false,
        registry_enabled: false,
      };
      config.value = fallback;
      return fallback;
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export function usePluginAdminConfig() {
  return { config, load };
}
