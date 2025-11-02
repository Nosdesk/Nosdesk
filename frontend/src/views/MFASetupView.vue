<template>
  <div class="min-h-screen w-full flex items-center justify-center bg-app">
    <div class="flex flex-col gap-6 w-full max-w-4xl p-8 py-12">
      <!-- Header -->
      <div class="flex flex-col gap-2 items-center">
        <img :src="logo" alt="Nosdesk Logo" class="px-4 max-w-md" />
        <h1 class="text-2xl font-bold text-primary mt-4">Complete Your Account Setup</h1>
        <p class="text-secondary text-center">
          Your account type requires multi-factor authentication for security
        </p>
      </div>

      <!-- Error Message -->
      <div v-if="errorMessage" class="bg-red-900/50 border border-red-700 text-red-200 px-4 py-3 rounded-lg text-sm">
        {{ errorMessage }}
      </div>

      <!-- Success Message -->
      <div v-if="successMessage" class="bg-green-900/50 border border-green-700 text-green-200 px-4 py-3 rounded-lg text-sm">
        {{ successMessage }}
      </div>



      <!-- MFA Settings Component - Loaded Immediately -->
      <div class="bg-surface rounded-xl border border-subtle">
        <MFASettings
          ref="mfaSettingsRef"
          :is-login-setup="true"
          @success="handleMfaSetupSuccess"
          @error="handleMfaSetupError"
        />
      </div>

      <!-- Navigation -->
      <div class="flex justify-between items-center">
        <button
          @click="goBackToLogin"
          class="flex items-center gap-2 px-4 py-2 text-sm text-tertiary hover:text-primary transition-colors"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
          </svg>
          Back to Login
        </button>

        <div class="text-xs text-tertiary">
          Step 1 of 1: Setup Multi-Factor Authentication
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import MFASettings from '@/components/settings/MFASettings.vue';
import logo from '@/assets/logo.svg';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();

const errorMessage = ref('');
const successMessage = ref('');
const isContextReady = ref(false);

// Template ref for MFASettings component
const mfaSettingsRef = ref();

// Get credentials from navigation state (more secure than URL params)
const credentials = ref<{ email: string; password: string } | null>(null);

// Security check and credential setup for async component
onMounted(async () => {
  console.log('🔍 MFA Setup - Checking for credentials:', {
    hasLoginSetupContext: !!sessionStorage.getItem('mfaLoginSetupContext'),
    hasSetupCredentials: !!sessionStorage.getItem('mfaSetupCredentials'),
    route: route.fullPath
  });
  
  // First check if we already have a valid setup context (from a refresh or direct navigation)
  const existingContext = sessionStorage.getItem('mfaLoginSetupContext');
  if (existingContext) {
    try {
      const context = JSON.parse(existingContext);
      // Check if the context is recent (within 10 minutes)
      if (context.timestamp && (Date.now() - context.timestamp) < 10 * 60 * 1000) {
        console.log('✅ Found valid existing MFA setup context');
        credentials.value = {
          email: context.email,
          password: context.password
        };
        isContextReady.value = true;
        return;
      } else {
        console.log('⏰ Existing context expired, cleaning up');
        sessionStorage.removeItem('mfaLoginSetupContext');
      }
    } catch (error) {
      console.log('❌ Invalid existing context, cleaning up');
      sessionStorage.removeItem('mfaLoginSetupContext');
    }
  }
  
  // Check if we have fresh credentials from login/onboarding
  const setupCredentials = sessionStorage.getItem('mfaSetupCredentials');
  if (setupCredentials) {
    try {
      const creds = JSON.parse(setupCredentials);
      // Check if the credentials are recent (within 5 minutes)
      if (creds.timestamp && (Date.now() - creds.timestamp) < 5 * 60 * 1000) {
        console.log('✅ Found valid setup credentials from:', creds.from);
        credentials.value = {
          email: creds.email,
          password: creds.password
        };
        
        // Clean up the setup credentials since we'll create the full context
        sessionStorage.removeItem('mfaSetupCredentials');
        
        // Store credentials for the MFASettings component to handle setup
        sessionStorage.setItem('mfaLoginSetupContext', JSON.stringify({
          email: credentials.value.email,
          password: credentials.value.password,
          timestamp: Date.now()
        }));
        
        console.log('✅ MFA setup context created successfully');
        return;
      } else {
        console.log('⏰ Setup credentials expired, cleaning up');
        sessionStorage.removeItem('mfaSetupCredentials');
      }
    } catch (error) {
      console.log('❌ Invalid setup credentials, cleaning up');
      sessionStorage.removeItem('mfaSetupCredentials');
    }
  }
  
  // Check if we're coming from a valid MFA setup requirement state
  if (authStore.mfaSetupRequired && authStore.mfaUserUuid) {
    console.log('🔄 No credentials found, but auth store indicates MFA setup required');
    // This happens if user refreshed the page or navigated directly
    // Redirect back to login to restart the process
    errorMessage.value = 'Session expired. Please log in again to set up MFA.';
    setTimeout(() => {
      authStore.clearMfaState();
      router.push('/login');
    }, 3000);
  } else {
    // No proper credentials - redirect back to login
    console.error('❌ No valid credentials found');
    errorMessage.value = 'Invalid access. Redirecting to login...';
    setTimeout(() => {
      router.push('/login');
    }, 2000);
  }
});

// Handle successful MFA setup from the component
const handleMfaSetupSuccess = async (message: string) => {
  // Only redirect if the setup is actually complete
  if (message === 'setup-complete') {
    // Clean up the temporary session data
    sessionStorage.removeItem('mfaLoginSetupContext');
    sessionStorage.removeItem('mfaSetupCredentials');
    
    // Redirect to dashboard immediately
    router.push('/');
  }
  // For other success messages (like "MFA setup initiated"), don't redirect
  // Let the user complete the process
};

// Handle MFA setup errors from the component
const handleMfaSetupError = (error: string) => {
  errorMessage.value = error;
};

// Navigation back to login
const goBackToLogin = () => {
  // Clean up any temporary data
  sessionStorage.removeItem('mfaLoginSetupContext');
  sessionStorage.removeItem('mfaSetupCredentials');
  authStore.clearMfaState();
  router.push('/login');
};
</script>

