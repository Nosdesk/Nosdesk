<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import axios from 'axios';
import BackButton from '@/components/common/BackButton.vue';
import EnvConfigNotice from '@/components/admin/EnvConfigNotice.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import { AdminIcons, isBrandIcon } from '@/components/admin/AdminIcons';

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

const router = useRouter();

// State for providers
const isLoading = ref(false);
const errorMessage = ref('');
const providers = ref<Provider[]>([]);
const successMessage = ref('');
const configValidations = ref<Record<number, ConfigValidation>>({});

// Load providers from API
const loadProviders = async () => {
  isLoading.value = true;
  errorMessage.value = '';
  
  try {
    const response = await axios.get('/api/admin/auth/providers');
    providers.value = response.data;
  } catch (error: any) {
    console.error('Failed to load auth providers:', error);
    errorMessage.value = error.response?.data?.message || 'Failed to load authentication providers';
  } finally {
    isLoading.value = false;
  }
};

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
  } catch (error: any) {
    const errorDetails = error.response?.data;
    configValidations.value[provider.id] = {
      valid: false,
      error: errorDetails?.message || `Configuration validation failed`
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

// Helper function to render SVG icons
const getProviderIcon = (providerType: string) => {
  // Map provider types to icon names
  const iconMap: Record<string, string> = {
    microsoft: 'microsoft',
    google: 'google',
    local: 'user',
    oidc: 'key'
  };
  const iconName = iconMap[providerType] || 'lock';
  return AdminIcons[iconName as keyof typeof AdminIcons] || AdminIcons.lock;
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

// Check if provider uses brand icon
const isProviderBrandIcon = (providerType: string) => {
  return ['google'].includes(providerType);
};

// Helper to get provider status description
const getProviderDescription = (provider: Provider) => {
  switch (provider.provider_type) {
    case 'microsoft':
      return 'Microsoft Entra ID (Azure AD) Single Sign-On';
    case 'local':
      return 'Email and password authentication';
    case 'google':
      return 'Google Workspace Single Sign-On';
    default:
      return 'External authentication provider';
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

onMounted(async () => {
  await loadProviders();
  await validateAllProviders();
});
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>
    
    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-primary">Authentication Providers</h1>
      </div>

      <!-- Configuration Notice -->
      <EnvConfigNotice>
        Authentication providers are configured through environment variables in your
        <code class="bg-surface px-1 rounded text-primary">.env</code> file.
        Use the "Validate Config" button to check if each provider is properly configured.
      </EnvConfigNotice>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
      
      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading providers..." />

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
                <span v-if="isProviderBrandIcon(provider.provider_type)" v-html="getProviderIcon(provider.provider_type)"></span>
                <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="getProviderIcon(provider.provider_type)"></svg>
              </div>

              <!-- Title and badges -->
              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">{{ provider.name }}</span>
                <span v-if="provider.is_default"
                      class="px-1.5 py-0.5 text-xs bg-accent/20 text-accent rounded-full border border-accent/50">
                  Default
                </span>
                <span
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="provider.enabled ? 'bg-status-success/20 text-status-success border-status-success/50' : 'bg-surface-alt text-tertiary border-default'"
                >
                  {{ provider.enabled ? 'Configured' : 'Not Configured' }}
                </span>
                <span
                  v-if="provider.enabled"
                  class="px-1.5 py-0.5 text-xs rounded-full border bg-accent/20 text-accent border-accent/50"
                >
                  Enabled
                </span>
              </div>
            </div>

            <!-- Current Configuration -->
            <div v-if="configValidations[provider.id]?.valid" class="flex flex-col md:flex-row gap-4 text-sm">
              <!-- Left: Client ID and Tenant ID (full values) -->
              <div class="flex-1 flex flex-col gap-2">
                <div v-if="configValidations[provider.id].client_id" class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Client ID</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidations[provider.id].client_id }}</span>
                </div>
                <div v-if="configValidations[provider.id].tenant_id" class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Tenant ID</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidations[provider.id].tenant_id }}</span>
                </div>
                <div v-if="configValidations[provider.id].redirect_uri" class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Redirect URI</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidations[provider.id].redirect_uri }}</span>
                </div>
              </div>
              <!-- Right: Secret status -->
              <div v-if="configValidations[provider.id].client_secret_configured !== undefined" class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Secret</span>
                  <span :class="configValidations[provider.id].client_secret_configured ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ configValidations[provider.id].client_secret_configured ? 'Configured' : 'Not Set' }}</span>
                </div>
              </div>
            </div>

            <!-- Configuration error -->
            <div v-if="configValidations[provider.id] && !configValidations[provider.id].valid" class="p-2 bg-status-error/10 border border-status-error/30 rounded-lg text-sm text-status-error flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor">
                <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
              </svg>
              {{ configValidations[provider.id].error }}
            </div>

            <!-- Required environment variables -->
            <div v-if="getConfigRequirements(provider).length > 0" class="flex items-center gap-2 text-xs">
              <span class="text-tertiary">Env:</span>
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

        <div v-if="providers.length === 0 && !isLoading" class="text-center py-12 text-secondary bg-surface rounded-xl border border-default p-6">
          <div class="flex justify-center mb-4">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
          </div>
          <p class="text-lg font-medium">No authentication providers found</p>
          <p class="mt-2 text-tertiary">Configure authentication providers in your environment variables</p>
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