<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import axios from 'axios';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import EnvConfigNotice from '@/components/admin/EnvConfigNotice.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Button from '@/components/common/Button.vue';
import brandingService, { type BrandingConfig } from '@/services/brandingService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';

const toast = useToastStore();
const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

// Define types for our data structures
interface EmailConfig {
  /** Active transport. Always 'smtp'; older backends omit it. */
  provider?: 'smtp';
  smtp_host: string;
  smtp_port: number;
  smtp_username: string;
  smtp_password_configured: boolean;
  from_name: string;
  from_email: string;
  enabled: boolean;
  is_configured: boolean;
  error?: string;
}

// Email config is read-only here (set via .env) and cached by Pinia
// Colada, so a revisit renders it instantly and revalidates silently.
// A skeleton shows only on the genuine first load (empty cache).
const EMAIL_CONFIG_KEY = ['email-config'] as const;
const emailConfigQuery = useQuery({
  key: EMAIL_CONFIG_KEY,
  query: async () => {
    const response = await axios.get('/api/admin/email/config');
    return response.data as EmailConfig;
  },
});
const emailConfig = computed<EmailConfig | null>(() => emailConfigQuery.data.value ?? null);
const isFirstLoad = computed(
  () => emailConfigQuery.status.value === 'pending' && emailConfigQuery.data.value === undefined,
);
const loadError = computed(() =>
  emailConfigQuery.error.value
    ? extractErrorMessage(emailConfigQuery.error.value, t('admin-email-settings-error-load'))
    : '',
);

// Test-send error stays inline; success is a toast (transient action).
const errorMessage = ref('');
const sendingTest = ref(false);
const testEmailAddress = ref('');

// Send a test email
const sendTestEmail = async () => {
  if (!testEmailAddress.value) {
    errorMessage.value = t('admin-email-settings-error-no-address');
    return;
  }

  // Basic email validation
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!emailRegex.test(testEmailAddress.value)) {
    errorMessage.value = t('admin-email-settings-error-bad-address');
    return;
  }

  sendingTest.value = true;
  errorMessage.value = '';

  try {
    const response = await axios.post('/api/admin/email/test', {
      to: testEmailAddress.value
    });

    toast.success(response.data.message || t('admin-email-settings-test-success'));
    testEmailAddress.value = ''; // Clear the input after success
  } catch (error) {
    console.error('Failed to send test email:', error);
    errorMessage.value = extractErrorMessage(error, t('admin-email-settings-error-test'));
    setTimeout(() => { errorMessage.value = ''; }, 5000);
  } finally {
    sendingTest.value = false;
  }
};

// Anti-phishing security note. Workspace-wide (site_settings), off by
// default. Renders in the footer of transactional emails (password
// reset, invitation), so it belongs with the outbound email config
// here rather than the inbound channel/ingestion settings. Stored via
// brandingService; shares the `branding-config` cache key with the
// branding + auto-ack surfaces so edits stay in lockstep.
const queryCache = useQueryCache();
const BRANDING_KEY = ['branding-config'] as const;
const brandingQuery = useQuery({
  key: BRANDING_KEY,
  query: () => brandingService.getBrandingConfig(),
});
const brandingConfig = computed<BrandingConfig | null>(() => brandingQuery.data.value ?? null);

const securityNoteEnabled = ref(false);
const securityNoteTemplate = ref('');
const savingSecurityNote = ref(false);

// One-shot seed from the cached query; later revalidations don't clobber
// in-progress edits.
const securityNoteSeeded = ref(false);
watch(
  brandingQuery.data,
  (data) => {
    if (!data || securityNoteSeeded.value) return;
    securityNoteEnabled.value = data.email_security_note_enabled;
    securityNoteTemplate.value = data.email_security_note_template ?? '';
    securityNoteSeeded.value = true;
  },
  { immediate: true },
);

const securityNoteIsDirty = computed(() => {
  const cfg = brandingConfig.value;
  if (!cfg) return false;
  return (
    securityNoteEnabled.value !== cfg.email_security_note_enabled ||
    securityNoteTemplate.value !== (cfg.email_security_note_template ?? '')
  );
});

async function saveSecurityNote() {
  if (!securityNoteIsDirty.value) return;
  errorMessage.value = '';
  savingSecurityNote.value = true;
  try {
    const updated = await brandingService.updateBrandingConfig({
      email_security_note_enabled: securityNoteEnabled.value,
      // Empty string clears back to the built-in localized default.
      email_security_note_template: securityNoteTemplate.value,
    });
    queryCache.setQueryData(BRANDING_KEY, updated);
    toast.success(t('admin-email-security-note-success-saved'));
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-email-security-note-error-save'));
    setTimeout(() => { errorMessage.value = ''; }, 5000);
  } finally {
    savingSecurityNote.value = false;
  }
}

// The environment variables that configure the SMTP transport.
const getRequiredEnvVars = () => [
  'SMTP_ENABLED',
  'SMTP_HOST',
  'SMTP_PORT',
  'SMTP_USERNAME',
  'SMTP_PASSWORD',
  'SMTP_FROM_NAME',
  'SMTP_FROM_EMAIL'
];

</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-email-settings-title') }}</h1>
        <p class="text-secondary mt-2">
          {{ $t('admin-email-settings-description') }}
        </p>
      </div>

      <!-- Configuration Notice -->
      <EnvConfigNotice>
        {{ $t('admin-email-settings-env-notice-prefix') }}
        <code class="bg-surface px-1 rounded text-primary">.env</code>
        {{ $t('admin-email-settings-env-notice-suffix') }}
      </EnvConfigNotice>

      <!-- Success message -->

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Load error (initial fetch failed with no cached data) -->
      <AlertMessage v-if="loadError && !emailConfig" type="error" :message="loadError" />

      <!-- First-load skeleton: a config-card shell. Cold cache only;
           revisits render the cached config instantly. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-email-settings-loading')"
        class="flex flex-col gap-4"
      >
        <div class="bg-surface border border-default rounded-xl p-4 flex flex-col gap-4">
          <div class="flex items-center gap-3">
            <SkeletonBar class="h-9 w-9 rounded-lg shrink-0" />
            <SkeletonBar class="h-4 w-48 max-w-full" />
          </div>
          <SkeletonBar class="h-4 w-full" />
          <SkeletonBar class="h-4 w-5/6" />
          <SkeletonBar class="h-4 w-2/3" />
        </div>
      </Skeleton>

      <!-- Email configuration display -->
      <div v-else-if="emailConfig" class="flex flex-col gap-4">
        <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors overflow-hidden">

          <!-- Configuration Header -->
          <div class="p-4 flex flex-col gap-3">
            <!-- Header row with icon -->
            <div class="flex items-center gap-3">
              <!-- Email icon -->
              <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
                <Icon name="email" size="md" />
              </div>

              <!-- Title and badges -->
              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">{{ $t('admin-email-settings-service') }}</span>
                <span
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="emailConfig?.is_configured ? 'bg-status-success/20 text-status-success border-status-success/50' : 'bg-surface-alt text-tertiary border-default'"
                >
                  {{ emailConfig?.is_configured ? $t('admin-email-settings-configured') : $t('admin-email-settings-not-configured') }}
                </span>
                <span
                  v-if="emailConfig?.enabled"
                  class="px-1.5 py-0.5 text-xs rounded-full border bg-accent/20 text-accent border-accent/50"
                >
                  {{ $t('admin-email-settings-enabled') }}
                </span>
                <span
                  v-if="emailConfig?.provider"
                  class="px-1.5 py-0.5 text-xs rounded-full border bg-surface-alt text-secondary border-default uppercase"
                >
                  {{ emailConfig.provider }}
                </span>
              </div>
            </div>

            <!-- Current Configuration -->
            <div v-if="emailConfig?.is_configured" class="flex flex-col md:flex-row gap-4 text-sm">
              <!-- Left: Server, Username, From details -->
              <div class="flex-1 flex flex-col gap-2">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-email-settings-server') }}</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all">{{ emailConfig.smtp_host }}:{{ emailConfig.smtp_port }}</span>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-email-settings-username') }}</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ emailConfig.smtp_username }}</span>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-email-settings-from-address') }}</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ emailConfig.from_name }} &lt;{{ emailConfig.from_email }}&gt;</span>
                </div>
              </div>
              <!-- Right: Password status -->
              <div class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">{{ $t('admin-email-settings-password') }}</span>
                  <span :class="emailConfig.smtp_password_configured ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ emailConfig.smtp_password_configured ? $t('admin-email-settings-configured') : $t('admin-email-settings-password-not-set') }}</span>
                </div>
              </div>
            </div>

            <!-- Configuration error -->
            <div v-if="emailConfig?.error" class="p-2 bg-status-error/10 border border-status-error/30 rounded-lg text-sm text-status-error flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor">
                <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
              </svg>
              {{ emailConfig.error }}
            </div>

            <!-- Required environment variables -->
            <div class="flex items-center gap-2 text-xs">
              <span class="text-tertiary">{{ $t('admin-email-settings-env-vars-label') }}</span>
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

          <!-- Test Email Section -->
          <div v-if="emailConfig?.is_configured" class="border-t border-default p-4 bg-surface-alt">
            <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-3">
              <span class="text-sm text-secondary whitespace-nowrap">{{ $t('admin-email-settings-test-send') }}</span>
              <input
                v-model="testEmailAddress"
                type="email"
                :placeholder="$t('admin-email-settings-test-placeholder')"
                class="flex-1 px-2.5 py-1.5 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
                :disabled="sendingTest"
                @keyup.enter="sendTestEmail"
              />
              <button
                @click="sendTestEmail"
                :disabled="sendingTest || !testEmailAddress"
                class="px-3 py-1.5 bg-accent text-on-accent rounded-lg text-sm hover:opacity-90 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-1.5 whitespace-nowrap"
              >
                <Spinner v-if="sendingTest" />
                <!-- Custom paper-plane "send" glyph; not a registry action icon. -->
                <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
                </svg>
                {{ sendingTest ? $t('admin-email-settings-test-sending') : $t('admin-email-settings-test-send-button') }}
              </button>
            </div>
          </div>
        </div>

        <!-- Not configured message -->
        <div v-if="!emailConfig?.is_configured" class="text-center py-12 text-secondary bg-surface rounded-xl border border-default p-6">
          <div class="flex justify-center mb-4">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
            </svg>
          </div>
          <p class="text-lg font-medium">{{ $t('admin-email-settings-empty-title') }}</p>
          <p class="mt-2 text-tertiary">{{ $t('admin-email-settings-empty-description') }}</p>
        </div>
      </div>

      <!-- Anti-phishing security note: footer copy for transactional
           mail. Editable (site_settings), unlike the env-driven config
           above. Shown once branding loads, independent of SMTP setup. -->
      <form
        v-if="brandingConfig"
        class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6"
        @submit.prevent="saveSecurityNote"
      >
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">
            {{ $t('admin-email-security-note-heading') }}
          </h2>
          <p class="text-sm text-secondary">
            {{ $t('admin-email-security-note-subtitle') }}
          </p>
        </div>

        <ToggleSwitch
          v-model="securityNoteEnabled"
          :label="$t('admin-email-security-note-toggle-label')"
          :description="$t('admin-email-security-note-toggle-description')"
        />

        <div class="flex flex-col gap-2">
          <FormTextarea
            v-model="securityNoteTemplate"
            :label="$t('admin-email-security-note-template-label')"
            :placeholder="$t('admin-email-security-note-template-placeholder')"
            :description="$t('admin-email-security-note-template-hint')"
            :rows="4"
            mono
            :disabled="!securityNoteEnabled"
          />
          <p class="text-xs text-tertiary">
            {{ $t('admin-email-security-note-variables-hint') }}
            <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;brand_name&#125;&#125;</code>,
            <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;domain&#125;&#125;</code>
          </p>
        </div>

        <div class="flex justify-end border-t border-default pt-4">
          <Button type="submit" :loading="savingSecurityNote" :disabled="!securityNoteIsDirty">
            {{ savingSecurityNote ? $t('admin-email-security-note-saving') : $t('admin-email-security-note-save') }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
