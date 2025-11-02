<script setup lang="ts">
import { ref, onMounted } from 'vue';
import axios from 'axios';
import BackButton from '@/components/common/BackButton.vue';

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
      <div class="p-4 bg-brand-blue/30 text-brand-blue rounded-xl border border-brand-blue/50 mb-4 flex items-start">
        <div class="mr-3 mt-0.5 text-brand-blue flex-shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z" clip-rule="evenodd" />
          </svg>
        </div>
        <div>
          <p class="font-medium">Configuration via Environment Variables</p>
          <p class="text-sm text-brand-blue/80 mt-1">
            Email settings are configured through environment variables in your
            <code class="bg-brand-blue/50 px-1 rounded">.env</code> file or Docker environment.
            Use the "Send Test Email" feature to verify your configuration is working correctly.
          </p>
        </div>
      </div>

      <!-- Success message -->
      <div
        v-if="successMessage"
        class="p-4 bg-status-success/30 text-status-success rounded-xl border border-status-success/50 mb-4 flex items-start"
      >
        <div class="mr-3 mt-0.5 text-status-success flex-shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
          </svg>
        </div>
        <div>{{ successMessage }}</div>
      </div>

      <!-- Error message -->
      <div
        v-if="errorMessage"
        class="p-4 bg-status-error/30 text-status-error rounded-xl border border-status-error/50 mb-4 flex items-start"
      >
        <div class="mr-3 mt-0.5 text-status-error flex-shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
          </svg>
        </div>
        <div>{{ errorMessage }}</div>
      </div>

      <!-- Loading state -->
      <div v-if="isLoading" class="flex justify-center my-8">
        <div class="animate-spin h-8 w-8 text-brand-blue">
          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
        <span class="ml-3 text-primary">Loading email configuration...</span>
      </div>

      <!-- Email configuration display -->
      <div v-else class="flex flex-col gap-4">
        <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">

          <!-- Configuration Header -->
          <div class="p-6">
            <div class="flex items-start gap-4">
              <!-- Email icon -->
              <div class="flex-shrink-0 h-12 w-12 rounded-lg bg-surface-alt flex items-center justify-center text-secondary">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                </svg>
              </div>

              <!-- Configuration status -->
              <div class="flex-1">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-medium text-lg text-primary">SMTP Email Service</span>
                  <span
                    class="px-2 py-0.5 text-xs rounded-full border"
                    :class="emailConfig?.is_configured ? 'bg-status-success/50 text-status-success border-status-success' : 'bg-surface-alt text-tertiary border-default'"
                  >
                    {{ emailConfig?.is_configured ? 'Configured' : 'Not Configured' }}
                  </span>
                  <span
                    v-if="emailConfig?.enabled"
                    class="px-2 py-0.5 text-xs rounded-full border bg-brand-blue/50 text-brand-blue border-brand-blue"
                  >
                    Enabled
                  </span>
                </div>
                <div class="text-sm text-secondary mt-1">
                  Send emails via SMTP for notifications and communications
                </div>

                <!-- Current Configuration -->
                <div v-if="emailConfig?.is_configured" class="mt-4 grid grid-cols-1 md:grid-cols-2 gap-3 text-sm">
                  <div>
                    <span class="text-tertiary">SMTP Server:</span>
                    <span class="text-primary ml-2">{{ emailConfig.smtp_host }}:{{ emailConfig.smtp_port }}</span>
                  </div>
                  <div>
                    <span class="text-tertiary">Username:</span>
                    <span class="text-primary ml-2">{{ emailConfig.smtp_username }}</span>
                  </div>
                  <div>
                    <span class="text-tertiary">From Name:</span>
                    <span class="text-primary ml-2">{{ emailConfig.from_name }}</span>
                  </div>
                  <div>
                    <span class="text-tertiary">From Email:</span>
                    <span class="text-primary ml-2">{{ emailConfig.from_email }}</span>
                  </div>
                  <div>
                    <span class="text-tertiary">Password:</span>
                    <span
                      class="ml-2"
                      :class="emailConfig.smtp_password_configured ? 'text-status-success' : 'text-status-error'"
                    >
                      {{ emailConfig.smtp_password_configured ? 'Configured' : 'Not Configured' }}
                    </span>
                  </div>
                </div>

                <!-- Configuration error -->
                <div v-if="emailConfig?.error" class="mt-4 p-3 bg-status-error/20 border border-status-error/50 rounded-lg text-sm text-status-error">
                  <div class="flex items-center gap-2 mb-1">
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                      <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
                    </svg>
                    Configuration Error
                  </div>
                  {{ emailConfig.error }}
                </div>

                <!-- Required environment variables -->
                <div class="mt-4">
                  <div class="text-xs text-tertiary mb-1">Required environment variables:</div>
                  <div class="flex flex-wrap gap-1">
                    <code
                      v-for="envVar in getRequiredEnvVars()"
                      :key="envVar"
                      class="text-xs bg-surface-alt text-secondary px-1.5 py-0.5 rounded"
                    >
                      {{ envVar }}
                    </code>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Test Email Section -->
          <div v-if="emailConfig?.is_configured" class="border-t border-default p-6 bg-surface-alt">
            <div class="mb-4">
              <h3 class="text-sm font-medium text-primary mb-2">Send Test Email</h3>
              <p class="text-xs text-secondary mb-4">
                Verify your email configuration by sending a test email to any address.
              </p>

              <div class="flex gap-2 flex-col sm:flex-row">
                <input
                  v-model="testEmailAddress"
                  type="email"
                  placeholder="recipient@example.com"
                  class="flex-1 px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-brand-blue focus:border-transparent text-sm"
                  :disabled="sendingTest"
                  @keyup.enter="sendTestEmail"
                />
                <button
                  @click="sendTestEmail"
                  :disabled="sendingTest || !testEmailAddress"
                  class="px-4 py-2 bg-brand-purple/50 text-brand-purple rounded-lg text-sm hover:bg-brand-purple/80 border border-brand-purple font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 whitespace-nowrap"
                >
                  <svg v-if="sendingTest" class="animate-spin h-4 w-4" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
                  </svg>
                  {{ sendingTest ? 'Sending...' : 'Send Test Email' }}
                </button>
              </div>
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
