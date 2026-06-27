import { defineStore } from 'pinia';
import { ref } from 'vue';
import { publicService, type PublicSiteSettings } from '../services/publicService';
import { translate } from '../i18n';

export const usePublicSettingsStore = defineStore('publicSettings', () => {
  const settings = ref<PublicSiteSettings | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function load(force = false): Promise<PublicSiteSettings | null> {
    if (settings.value && !force) return settings.value;
    loading.value = true;
    error.value = null;
    try {
      settings.value = await publicService.getSettings();
      return settings.value;
    } catch (e: unknown) {
      error.value =
        e instanceof Error
          ? e.message
          : translate('error-store-public-settings-load', undefined, 'Failed to load public settings');
      return null;
    } finally {
      loading.value = false;
    }
  }

  return { settings, loading, error, load };
});
