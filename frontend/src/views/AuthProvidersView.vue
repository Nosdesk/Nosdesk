<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import axios from 'axios';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';

import EnvConfigNotice from '@/components/admin/EnvConfigNotice.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import Icon from '@/components/common/Icon.vue';
import BrandIcon from '@/components/common/BrandIcon.vue';
import type { IconName } from '@/components/common/icons';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

// Define types for our data structures
interface Provider {
  id: number;
  name: string;
  provider_type: string;
  enabled: boolean;
  is_default: boolean;
}

interface ConfigValidation {
  valid: boolean;
  client_id?: string;
  tenant_id?: string;
  client_secret_configured?: boolean;
  redirect_uri?: string;
  error?: string;
}

// Provider list is read-only here (configured via .env) and cached by
// Pinia Colada, so navigating away and back renders it instantly and
// revalidates silently. A skeleton shows only on the genuine first
// load (empty cache); see `isFirstLoad`.
const AUTH_PROVIDERS_KEY = ['auth-providers'] as const;
const providersQuery = useQuery({
  key: AUTH_PROVIDERS_KEY,
  query: async () => {
    const response = await axios.get('/api/admin/auth/providers');
    return response.data as Provider[];
  },
});
const providers = computed<Provider[]>(() =>
  Array.isArray(providersQuery.data.value) ? providersQuery.data.value : [],
);
const isFirstLoad = computed(
  () => providersQuery.status.value === 'pending' && providersQuery.data.value === undefined,
);
const loadError = computed(() =>
  providersQuery.error.value ? t('admin-auth-providers-error-load') : '',
);
const successMessage = ref('');
const configValidations = ref<Record<number, ConfigValidation>>({});

// Validate provider configuration
const validateProviderConfig = async (provider: Provider) => {
  try {
    if (provider.provider_type === 'microsoft') {
      const response = await axios.get(`/api/integrations/graph/config`);

      configValidations.value[provider.id] = {
        valid: true,
        client_id: response.data.client_id,
        tenant_id: response.data.tenant_id,
        client_secret_configured: response.data.client_secret_configured,
        redirect_uri: response.data.redirect_uri
      };
    }
  } catch (error) {
    configValidations.value[provider.id] = {
      valid: false,
      error: extractErrorMessage(error, t('admin-auth-providers-error-validate'))
    };
  }
};

// Validate all enabled providers
const validateAllProviders = async () => {
  for (const provider of providers.value) {
    if (provider.enabled && provider.provider_type !== 'local') {
      await validateProviderConfig(provider);
    }
  }
};

/**
 * Provider type → icon registry name. Brand icons (`google`)
 * fall through this map and render via `<BrandIcon>` instead;
 * `getProviderIcon` only owns the monochrome-stroke side.
 */
const getProviderIcon = (providerType: string): IconName => {
  const iconMap: Record<string, IconName> = {
    microsoft: 'microsoft',
    local: 'user',
    oidc: 'key',
  };
  return iconMap[providerType] ?? 'lock';
};

// Helper to get icon background class
const getProviderIconBgClass = (providerType: string) => {
  switch (providerType) {
    case 'microsoft':
      return 'bg-accent/20 text-accent';
    case 'google':
      return 'bg-surface-alt';
    case 'local':
      return 'bg-accent/20 text-accent';
    case 'oidc':
      return 'bg-status-warning/20 text-status-warning';
    default:
      return 'bg-accent/20 text-accent';
  }
};

// Helper to get configuration requirements
const getConfigRequirements = (provider: Provider) => {
  switch (provider.provider_type) {
    case 'microsoft':
      return [
        'MICROSOFT_CLIENT_ID',
        'MICROSOFT_CLIENT_SECRET', 
        'MICROSOFT_TENANT_ID',
        'MICROSOFT_REDIRECT_URI'
      ];
    case 'google':
      return [
        'GOOGLE_CLIENT_ID',
        'GOOGLE_CLIENT_SECRET',
        'GOOGLE_REDIRECT_URI'
      ];
    default:
      return [];
  }
};

// Re-run per-provider config validation whenever the provider list
// arrives — on first fetch, on a cache-hit remount, and on background
// refetch. Guarded so the initial undefined value is a no-op.
watch(
  () => providersQuery.data.value,
  (list) => {
    if (list) validateAllProviders();
  },
  { immediate: true },
);
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-auth-providers-title') }}</h1>
      </div>

      <!-- Configuration Notice -->
      <EnvConfigNotice>
        {{ $t('admin-auth-providers-env-notice-prefix') }}
        <code class="bg-surface px-1 rounded text-primary">.env</code>
        {{ $t('admin-auth-providers-env-notice-suffix') }}
      </EnvConfigNotice>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Load error (initial fetch failed with no cached data) -->
      <AlertMessage v-if="loadError && providers.length === 0" type="error" :message="loadError" />

      <!-- First-load skeleton: mirrors the provider-card layout so the
           shell doesn't shift when data arrives. Cold cache only;
           remounts render cached cards instantly and revalidate
           silently in the background. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-auth-providers-loading')"
        class="flex flex-col gap-4"
      >
        <div
          v-for="n in 2"
          :key="n"
          class="bg-surface border border-default rounded-xl p-4 flex items-center gap-3"
        >
          <SkeletonBar class="w-10 h-10 rounded-lg shrink-0" />
          <div class="flex-1 flex flex-col gap-2">
            <SkeletonBar class="h-4 w-40 max-w-full" />
            <SkeletonBar class="h-3 w-1/2" />
          </div>
        </div>
      </Skeleton>

      <!-- Provider list -->
      <div v-else class="flex flex-col gap-4">
        <div v-for="provider in providers" :key="provider.id"
             class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">

          <!-- Provider Header -->
          <div class="p-4 flex flex-col gap-3">
            <!-- Header row with icon -->
            <div class="flex items-center gap-3">
              <!-- Provider icon -->
              <div
                class="flex-shrink-0 h-9 w-9 rounded-lg flex items-center justify-center"
                :class="getProviderIconBgClass(provider.provider_type)"
              >
                <BrandIcon v-if="provider.provider_type === 'google'" brand="google" />
                <Icon v-else :name="getProviderIcon(provider.provider_type)" size="md" />
              </div>

              <!-- Title and badges -->
              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">{{ provider.name }}</span>
                <span v-if="provider.is_default"
                      class="px-1.5 py-0.5 text-xs bg-accent/20 text-accent rounded-full border border-accent/50">
                  {{ $t('admin-auth-providers-default-badge') }}
                </span>
                <span
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="provider.enabled ? 'bg-status-success/20 text-status-success border-status-success/50' : 'bg-surface-alt text-tertiary border-default'"
                >
                  {{ provider.enabled ? $t('admin-auth-providers-configured') : $t('admin-auth-providers-not-configured') }}
                </span>
                <span
                  v-if="provider.enabled"
                  class="px-1.5 py-0.5 text-xs rounded-full border bg-accent/20 text-accent border-accent/50"
                >
                  {{ $t('admin-auth-providers-enabled') }}
                </span>
              </div>
            </div>

            <!-- Current Configuration -->
            <div v-if="configValidations[provider.id]?.valid" class="flex flex-col md:flex-row gap-4 text-sm">
              <!-- Left: Client ID and Tenant ID (full values) -->
              <div class="flex-1 flex flex-col gap-2">
                <div v-if="configValidations[provider.id].client_id" class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-auth-providers-client-id') }}</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidations[provider.id].client_id }}</span>
                </div>
                <div v-if="configValidations[provider.id].tenant_id" class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-auth-providers-tenant-id') }}</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidations[provider.id].tenant_id }}</span>
                </div>
                <div v-if="configValidations[provider.id].redirect_uri" class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-auth-providers-redirect-uri') }}</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidations[provider.id].redirect_uri }}</span>
                </div>
              </div>
              <!-- Right: Secret status -->
              <div v-if="configValidations[provider.id].client_secret_configured !== undefined" class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-auth-providers-secret') }}</span>
                  <span :class="configValidations[provider.id].client_secret_configured ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ configValidations[provider.id].client_secret_configured ? $t('admin-auth-providers-configured') : $t('admin-auth-providers-secret-not-set') }}</span>
                </div>
              </div>
            </div>

            <!-- Configuration error -->
            <div v-if="configValidations[provider.id] && !configValidations[provider.id].valid" class="p-2 bg-status-error/10 border border-status-error/30 rounded-lg text-sm text-status-error flex items-center gap-2">
              <Icon name="warning" class="flex-shrink-0" />
              <span>{{ configValidations[provider.id].error }}</span>
            </div>

            <!-- Required environment variables -->
            <div v-if="getConfigRequirements(provider).length > 0" class="flex items-center gap-2 text-xs">
              <span class="text-tertiary">{{ $t('admin-auth-providers-env-label') }}</span>
              <div class="flex flex-wrap gap-1">
                <code
                  v-for="envVar in getConfigRequirements(provider)"
                  :key="envVar"
                  class="bg-surface-alt text-secondary px-1 py-0.5 rounded"
                >
                  {{ envVar }}
                </code>
              </div>
            </div>
          </div>
        </div>

        <div v-if="providers.length === 0 && !isFirstLoad" class="text-center py-12 text-secondary bg-surface rounded-xl border border-default p-6">
          <div class="flex justify-center mb-4">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
          </div>
          <p class="text-lg font-medium">{{ $t('admin-auth-providers-empty-title') }}</p>
          <p class="mt-2 text-tertiary">{{ $t('admin-auth-providers-empty-description') }}</p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.auth-providers-view {
  max-width: 1200px;
  margin: 0 auto;
}
</style> 