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
        <h1 class="text-2xl font-bold text-white">Email Configuration</h1>
        <p class="text-slate-400 mt-2">
          View email configuration status and send test emails. Email settings are configured via environment variables.
        </p>
      </div>

      <!-- Configuration Notice -->
      <div class="p-4 bg-blue-900/30 text-blue-400 rounded-xl border border-blue-700/50 mb-4 flex items-start">
        <div class="mr-3 mt-0.5 text-blue-400 flex-shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z" clip-rule="evenodd" />
          </svg>
        </div>
        <div>
          <p class="font-medium">Configuration via Environment Variables</p>
          <p class="text-sm text-blue-300 mt-1">
            Email settings are configured through environment variables in your
            <code class="bg-blue-800/50 px-1 rounded">.env</code> file or Docker environment.
            Use the "Send Test Email" feature to verify your configuration is working correctly.
          </p>
        </div>
      </div>

      <!-- Success message -->
      <div
        v-if="successMessage"
        class="p-4 bg-green-900/30 text-green-400 rounded-xl border border-green-700/50 mb-4 flex items-start"
      >
        <div class="mr-3 mt-0.5 text-green-400 flex-shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
          </svg>
        </div>
        <div>{{ successMessage }}</div>
      </div>

      <!-- Error message -->
      <div
        v-if="errorMessage"
        class="p-4 bg-red-900/30 text-red-400 rounded-xl border border-red-700/50 mb-4 flex items-start"
      >
        <div class="mr-3 mt-0.5 text-red-400 flex-shrink-0">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
          </svg>
        </div>
        <div>{{ errorMessage }}</div>
      </div>

      <!-- Loading state -->
      <div v-if="isLoading" class="flex justify-center my-8">
        <div class="animate-spin h-8 w-8 text-blue-500">
          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
        <span class="ml-3 text-white">Loading email configuration...</span>
      </div>

      <!-- Email configuration display -->
      <div v-else class="flex flex-col gap-4">
        <div class="bg-slate-800 border border-slate-700/50 rounded-xl hover:border-slate-600/50 transition-colors">

          <!-- Configuration Header -->
          <div class="p-6">
            <div class="flex items-start gap-4">
              <!-- Email icon -->
              <div class="flex-shrink-0 h-12 w-12 rounded-lg bg-slate-700/50 flex items-center justify-center text-slate-300">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                </svg>
              </div>

              <!-- Configuration status -->
              <div class="flex-1">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-medium text-lg text-white">SMTP Email Service</span>
                  <span
                    class="px-2 py-0.5 text-xs rounded-full border"
                    :class="emailConfig?.is_configured ? 'bg-green-900/50 text-green-200 border-green-700' : 'bg-slate-700/50 text-slate-400 border-slate-600'"
                  >
                    {{ emailConfig?.is_configured ? 'Configured' : 'Not Configured' }}
                  </span>
                  <span
                    v-if="emailConfig?.enabled"
                    class="px-2 py-0.5 text-xs rounded-full border bg-blue-900/50 text-blue-200 border-blue-700"
                  >
                    Enabled
                  </span>
                </div>
                <div class="text-sm text-slate-400 mt-1">
                  Send emails via SMTP for notifications and communications
                </div>

                <!-- Current Configuration -->
                <div v-if="emailConfig?.is_configured" class="mt-4 grid grid-cols-1 md:grid-cols-2 gap-3 text-sm">
                  <div>
                    <span class="text-slate-400">SMTP Server:</span>
                    <span class="text-white ml-2">{{ emailConfig.smtp_host }}:{{ emailConfig.smtp_port }}</span>
                  </div>
                  <div>
                    <span class="text-slate-400">Username:</span>
                    <span class="text-white ml-2">{{ emailConfig.smtp_username }}</span>
                  </div>
                  <div>
                    <span class="text-slate-400">From Name:</span>
                    <span class="text-white ml-2">{{ emailConfig.from_name }}</span>
                  </div>
                  <div>
                    <span class="text-slate-400">From Email:</span>
                    <span class="text-white ml-2">{{ emailConfig.from_email }}</span>
                  </div>
                  <div>
                    <span class="text-slate-400">Password:</span>
                    <span
                      class="ml-2"
                      :class="emailConfig.smtp_password_configured ? 'text-green-400' : 'text-red-400'"
                    >
                      {{ emailConfig.smtp_password_configured ? 'Configured' : 'Not Configured' }}
                    </span>
                  </div>
                </div>

                <!-- Configuration error -->
                <div v-if="emailConfig?.error" class="mt-4 p-3 bg-red-900/20 border border-red-700/50 rounded-lg text-sm text-red-300">
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
                  <div class="text-xs text-slate-500 mb-1">Required environment variables:</div>
                  <div class="flex flex-wrap gap-1">
                    <code
                      v-for="envVar in getRequiredEnvVars()"
                      :key="envVar"
                      class="text-xs bg-slate-700/50 text-slate-300 px-1.5 py-0.5 rounded"
                    >
                      {{ envVar }}
                    </code>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Test Email Section -->
          <div v-if="emailConfig?.is_configured" class="border-t border-slate-700/50 p-6 bg-slate-700/20">
            <div class="mb-4">
              <h3 class="text-sm font-medium text-white mb-2">Send Test Email</h3>
              <p class="text-xs text-slate-400 mb-4">
                Verify your email configuration by sending a test email to any address.
              </p>

              <div class="flex gap-2 flex-col sm:flex-row">
                <input
                  v-model="testEmailAddress"
                  type="email"
                  placeholder="recipient@example.com"
                  class="flex-1 px-3 py-2 bg-slate-900 border border-slate-600 rounded-lg text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-sm"
                  :disabled="sendingTest"
                  @keyup.enter="sendTestEmail"
                />
                <button
                  @click="sendTestEmail"
                  :disabled="sendingTest || !testEmailAddress"
                  class="px-4 py-2 bg-purple-900/50 text-purple-200 rounded-lg text-sm hover:bg-purple-800/50 border border-purple-700 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 whitespace-nowrap"
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
        <div v-if="!emailConfig?.is_configured" class="text-center py-12 text-slate-400 bg-slate-800 rounded-xl border border-slate-700/50 p-6">
          <div class="flex justify-center mb-4">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 text-slate-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
            </svg>
          </div>
          <p class="text-lg font-medium">Email is not configured</p>
          <p class="mt-2 text-slate-500">Configure email settings in your environment variables to enable email functionality</p>
        </div>
      </div>
    </div>
  </div>
</template>
