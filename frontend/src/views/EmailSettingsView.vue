<script setup lang="ts">
import { ref, onMounted } from 'vue';
import axios from 'axios';
import BackButton from '@/components/common/BackButton.vue';
import EnvConfigNotice from '@/components/admin/EnvConfigNotice.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';

// Define types for our data structures
interface EmailConfig {
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

// State
const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const emailConfig = ref<EmailConfig | null>(null);
const sendingTest = ref(false);
const testEmailAddress = ref('');

// Load email configuration from API
const loadEmailConfig = async () => {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    const response = await axios.get('/api/admin/email/config');
    emailConfig.value = response.data;
  } catch (error: any) {
    console.error('Failed to load email configuration:', error);
    errorMessage.value = error.response?.data?.message || 'Failed to load email configuration';
  } finally {
    isLoading.value = false;
  }
};

// Send a test email
const sendTestEmail = async () => {
  if (!testEmailAddress.value) {
    errorMessage.value = 'Please enter an email address';
    return;
  }

  // Basic email validation
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!emailRegex.test(testEmailAddress.value)) {
    errorMessage.value = 'Please enter a valid email address';
    return;
  }

  sendingTest.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    const response = await axios.post('/api/admin/email/test', {
      to: testEmailAddress.value
    });

    successMessage.value = response.data.message || 'Test email sent successfully';
    testEmailAddress.value = ''; // Clear the input after success

    setTimeout(() => { successMessage.value = ''; }, 5000);
  } catch (error: any) {
    console.error('Failed to send test email:', error);
    errorMessage.value = error.response?.data?.message || 'Failed to send test email';
    setTimeout(() => { errorMessage.value = ''; }, 5000);
  } finally {
    sendingTest.value = false;
  }
};

// Helper to get required environment variables
const getRequiredEnvVars = () => {
  return [
    'SMTP_ENABLED',
    'SMTP_HOST',
    'SMTP_PORT',
    'SMTP_USERNAME',
    'SMTP_PASSWORD',
    'SMTP_FROM_NAME',
    'SMTP_FROM_EMAIL'
  ];
};

onMounted(() => {
  loadEmailConfig();
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
        <h1 class="text-2xl font-bold text-primary">Email Configuration</h1>
        <p class="text-secondary mt-2">
          View email configuration status and send test emails. Email settings are configured via environment variables.
        </p>
      </div>

      <!-- Configuration Notice -->
      <EnvConfigNotice>
        Email settings are configured through environment variables in your
        <code class="bg-surface px-1 rounded text-primary">.env</code> file or Docker environment.
        Use the "Send Test Email" feature to verify your configuration is working correctly.
      </EnvConfigNotice>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading email configuration..." />

      <!-- Email configuration display -->
      <div v-else class="flex flex-col gap-4">
        <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">

          <!-- Configuration Header -->
          <div class="p-4 flex flex-col gap-3">
            <!-- Header row with icon -->
            <div class="flex items-center gap-3">
              <!-- Email icon -->
              <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-brand-blue/20 flex items-center justify-center text-brand-blue">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                </svg>
              </div>

              <!-- Title and badges -->
              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">SMTP Email Service</span>
                <span
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="emailConfig?.is_configured ? 'bg-status-success/20 text-status-success border-status-success/50' : 'bg-surface-alt text-tertiary border-default'"
                >
                  {{ emailConfig?.is_configured ? 'Configured' : 'Not Configured' }}
                </span>
                <span
                  v-if="emailConfig?.enabled"
                  class="px-1.5 py-0.5 text-xs rounded-full border bg-brand-blue/20 text-brand-blue border-brand-blue/50"
                >
                  Enabled
                </span>
              </div>
            </div>

            <!-- Current Configuration -->
            <div v-if="emailConfig?.is_configured" class="flex flex-col md:flex-row gap-4 text-sm">
              <!-- Left: Server, Username, From details -->
              <div class="flex-1 flex flex-col gap-2">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Server</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all">{{ emailConfig.smtp_host }}:{{ emailConfig.smtp_port }}</span>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Username</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ emailConfig.smtp_username }}</span>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">From Address</span>
                  <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ emailConfig.from_name }} &lt;{{ emailConfig.from_email }}&gt;</span>
                </div>
              </div>
              <!-- Right: Password status -->
              <div class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Password</span>
                  <span :class="emailConfig.smtp_password_configured ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ emailConfig.smtp_password_configured ? 'Configured' : 'Not Set' }}</span>
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

          <!-- Test Email Section -->
          <div v-if="emailConfig?.is_configured" class="border-t border-default p-4 bg-surface-alt">
            <div class="flex items-center gap-3">
              <span class="text-sm text-secondary whitespace-nowrap">Send test:</span>
              <input
                v-model="testEmailAddress"
                type="email"
                placeholder="recipient@example.com"
                class="flex-1 px-2.5 py-1.5 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-brand-blue focus:border-transparent text-sm"
                :disabled="sendingTest"
                @keyup.enter="sendTestEmail"
              />
              <button
                @click="sendTestEmail"
                :disabled="sendingTest || !testEmailAddress"
                class="px-3 py-1.5 bg-brand-blue text-white rounded-lg text-sm hover:opacity-90 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5 whitespace-nowrap"
              >
                <svg v-if="sendingTest" class="animate-spin h-3.5 w-3.5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
                </svg>
                {{ sendingTest ? 'Sending...' : 'Send' }}
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
          <p class="text-lg font-medium">Email is not configured</p>
          <p class="mt-2 text-tertiary">Configure email settings in your environment variables to enable email functionality</p>
        </div>
      </div>
    </div>
  </div>
</template>
