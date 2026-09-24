import { defineStore } from 'pinia';
import { ref } from 'vue';
import { logger } from '../utils/logger';
import { translate } from '../i18n';
import {
  featureFlagsService,
  type FeatureFlagMap,
  type FeatureFlagValue,
} from '../services/featureFlagsService';

export const useFeatureFlagsStore = defineStore('featureFlags', () => {
  const flags = ref<FeatureFlagMap>({});
  const loaded = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);

  let inflight: Promise<FeatureFlagMap> | null = null;
  // Bumped by reset(): a load started before a workspace switch must not
  // write its result, nor clear the new workspace's in-flight load.
  let generation = 0;

  async function load(force = false): Promise<FeatureFlagMap> {
    if (loaded.value && !force) return flags.value;
    if (inflight) return inflight;

    loading.value = true;
    error.value = null;

    const gen = generation;
    const run: Promise<FeatureFlagMap> = (async () => {
      try {
        const next = await featureFlagsService.getMine();
        if (gen !== generation) return next;
        flags.value = next;
        loaded.value = true;
        return next;
      } catch (e) {
        if (gen !== generation) return flags.value;
        logger.error('Failed to load feature flags', e);
        error.value =
          e instanceof Error
            ? e.message
            : translate('error-store-feature-flags-load', undefined, 'Failed to load feature flags');
        return flags.value;
      } finally {
        if (gen === generation) {
          inflight = null;
          loading.value = false;
        }
      }
    })();
    inflight = run;

    return run;
  }

  function reset() {
    generation++;
    inflight = null;
    loading.value = false;
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
