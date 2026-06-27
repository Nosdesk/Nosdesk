import { computed, type ComputedRef } from 'vue';
import { useFeatureFlagsStore } from '@nosdesk/core/stores/featureFlags';
import type { FeatureFlagValue } from '@nosdesk/core/services/featureFlagsService';

/**
 * Reactive boolean for a feature flag. The value tracks the
 * resolved-flag map in the Pinia store, which is loaded once
 * after auth and refreshed when the backend broadcasts a
 * feature_flags_changed SSE event.
 */
export function useFeatureFlag(name: string): ComputedRef<boolean> {
  const store = useFeatureFlagsStore();
  return computed(() => store.isEnabled(name));
}

/**
 * Reactive raw value for a feature flag. Use when the flag
 * carries config beyond a boolean (e.g. a string variant or a
 * numeric threshold).
 */
export function useFeatureFlagValue(name: string): ComputedRef<FeatureFlagValue> {
  const store = useFeatureFlagsStore();
  return computed(() => store.get(name));
}
