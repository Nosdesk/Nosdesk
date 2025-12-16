<script setup lang="ts">
import { ref, onMounted } from 'vue';
import axios from 'axios';
import BackButton from '@/components/common/BackButton.vue';
import EnvConfigNotice from '@/components/admin/EnvConfigNotice.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';

// Define types for our data structures
interface BrandingConfig {
  app_name: string;
  app_name_configured: boolean;
  logo_url: string | null;
  logo_configured: boolean;
  favicon_url: string | null;
  favicon_configured: boolean;
  primary_color: string | null;
  primary_color_configured: boolean;
  is_configured: boolean;
}

// State
const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const brandingConfig = ref<BrandingConfig | null>(null);

// Load branding configuration from API
const loadBrandingConfig = async () => {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    const response = await axios.get('/api/admin/branding/config');
    brandingConfig.value = response.data;
  } catch (error: any) {
    console.error('Failed to load branding configuration:', error);
    // If the endpoint doesn't exist yet, show a placeholder config
    if (error.response?.status === 404) {
      brandingConfig.value = {
        app_name: 'Nosdesk',
        app_name_configured: false,
        logo_url: null,
        logo_configured: false,
        favicon_url: null,
        favicon_configured: false,
        primary_color: null,
        primary_color_configured: false,
        is_configured: false
      };
    } else {
      errorMessage.value = error.response?.data?.message || 'Failed to load branding configuration';
    }
  } finally {
    isLoading.value = false;
  }
};

// Helper to get required environment variables
const getRequiredEnvVars = () => {
  return [
    'APP_NAME',
    'APP_LOGO_URL',
    'APP_FAVICON_URL',
    'APP_PRIMARY_COLOR'
  ];
};

onMounted(() => {
  loadBrandingConfig();
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
        <h1 class="text-2xl font-bold text-primary">Branding</h1>
        <p class="text-secondary mt-2">
          Customize the appearance and branding of the application. Branding settings are configured via environment variables.
        </p>
      </div>

      <!-- Configuration Notice -->
      <EnvConfigNotice>
        Branding settings are configured through environment variables in your
        <code class="bg-surface px-1 rounded text-primary">.env</code> file or Docker environment.
        Changes will take effect after restarting the application.
      </EnvConfigNotice>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading branding configuration..." />

      <!-- Branding configuration display -->
      <div v-else class="flex flex-col gap-4">
        <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">

          <!-- Configuration Header -->
          <div class="p-4 flex flex-col gap-3">
            <!-- Header row with icon -->
            <div class="flex items-center gap-3">
              <!-- Paint/branding icon -->
              <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-purple-500/20 flex items-center justify-center text-purple-400">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />
                </svg>
              </div>

              <!-- Title and badges -->
              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">Application Branding</span>
                <span
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="brandingConfig?.is_configured ? 'bg-status-success/20 text-status-success border-status-success/50' : 'bg-surface-alt text-tertiary border-default'"
                >
                  {{ brandingConfig?.is_configured ? 'Configured' : 'Using Defaults' }}
                </span>
              </div>
            </div>

            <!-- Current Configuration -->
            <div class="flex flex-col md:flex-row gap-4 text-sm">
              <!-- Left: App Name, Logo URL, Favicon URL -->
              <div class="flex-1 flex flex-col gap-2">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Application Name</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all">{{ brandingConfig?.app_name || 'Nosdesk' }}</span>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Logo URL</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ brandingConfig?.logo_url || 'Not configured (using default)' }}</span>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Favicon URL</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ brandingConfig?.favicon_url || 'Not configured (using default)' }}</span>
                </div>
              </div>
              <!-- Right: Primary Color status -->
              <div class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-32 md:flex-shrink-0">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Primary Color</span>
                  <div class="flex items-center gap-2">
                    <div
                      v-if="brandingConfig?.primary_color"
                      class="w-6 h-6 rounded border border-default"
                      :style="{ backgroundColor: brandingConfig.primary_color }"
                    ></div>
                    <span class="font-mono text-xs bg-surface-alt px-2 py-1.5 rounded" :class="brandingConfig?.primary_color ? 'text-primary' : 'text-tertiary'">
                      {{ brandingConfig?.primary_color || 'Default' }}
                    </span>
                  </div>
                </div>
              </div>
            </div>

            <!-- Required environment variables -->
            <div class="flex items-center gap-2 text-xs">
              <span class="text-tertiary">Env:</span>
              <div class="flex flex-wrap gap-1">
                <code
                  v-for="envVar in getRequiredEnvVars()"
                  :key="envVar"
                  class="bg-surface-alt text-secondary px-1 py-0.5 rounded"
                >
                  {{ envVar }}
                </code>
              </div>
            </div>
          </div>

          <!-- Preview Section -->
          <div class="border-t border-default p-4 bg-surface-alt">
            <div class="flex items-center gap-3 mb-3">
              <span class="text-sm text-secondary font-medium">Preview</span>
            </div>
            <div class="flex items-center gap-3 p-3 bg-surface rounded-lg border border-default">
              <!-- Logo preview -->
              <div class="flex-shrink-0 h-10 w-10 rounded-lg bg-brand-blue/20 flex items-center justify-center">
                <img
                  v-if="brandingConfig?.logo_url"
                  :src="brandingConfig.logo_url"
                  alt="App Logo"
                  class="h-8 w-8 object-contain"
                />
                <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-brand-blue" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
                </svg>
              </div>
              <!-- App name preview -->
              <span class="text-lg font-semibold text-primary">{{ brandingConfig?.app_name || 'Nosdesk' }}</span>
            </div>
          </div>
        </div>

        <!-- Not configured info message -->
        <div v-if="!brandingConfig?.is_configured" class="text-center py-8 text-secondary bg-surface rounded-xl border border-default p-6">
          <div class="flex justify-center mb-4">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />
            </svg>
          </div>
          <p class="text-lg font-medium">Using default branding</p>
          <p class="mt-2 text-tertiary">Configure branding settings in your environment variables to customize the application appearance</p>
        </div>
      </div>
    </div>
  </div>
</template>
