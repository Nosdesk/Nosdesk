import { defineStore } from 'pinia';
import { ref } from 'vue';
import { logger } from '@/utils/logger';
import { translate } from '@/i18n';
import {
  featureFlagsService,
  type FeatureFlagMap,
  type FeatureFlagValue,
} from '@/services/featureFlagsService';

export const useFeatureFlagsStore = defineStore('featureFlags', () => {
  const flags = ref<FeatureFlagMap>({});
  const loaded = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);

  let inflight: Promise<FeatureFlagMap> | null = null;

  async function load(force = false): Promise<FeatureFlagMap> {
    if (loaded.value && !force) return flags.value;
    if (inflight) return inflight;

    loading.value = true;
    error.value = null;

    inflight = (async () => {
      try {
        const next = await featureFlagsService.getMine();
        flags.value = next;
        loaded.value = true;
        return next;
      } catch (e) {
        logger.error('Failed to load feature flags', e);
        error.value =
          e instanceof Error
            ? e.message
            : translate('error-store-feature-flags-load', undefined, 'Failed to load feature flags');
        return flags.value;
      } finally {
        loading.value = false;
        inflight = null;
      }
    })();

    return inflight;
  }

  function reset() {
    flags.value = {};
    loaded.value = false;
    error.value = null;
  }

  function get(name: string): FeatureFlagValue {
    return Object.prototype.hasOwnProperty.call(flags.value, name)
      ? flags.value[name]
      : null;
  }

  function isEnabled(name: string): boolean {
    return get(name) === true;
  }

  return { flags, loaded, loading, error, load, reset, get, isEnabled };
});
