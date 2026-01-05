<template>
  <div class="fixed inset-0 bg-app overflow-y-auto">
    <div class="min-h-full flex flex-col items-center justify-center py-8 sm:py-12 px-4 sm:px-8">
      <div class="flex flex-col gap-6 w-full max-w-4xl">
      <!-- Header -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-8 px-4 text-accent" aria-label="Nosdesk Logo" />
        <h1 class="text-2xl font-bold text-primary mt-4 text-center">Complete Your Account Setup</h1>
        <p class="text-secondary text-center">
          Your account type requires multi-factor authentication for security
        </p>
      </div>

      <!-- Error Message -->
      <div v-if="errorMessage" class="bg-status-error/50 border border-status-error/70 text-status-error px-4 py-3 rounded-lg text-sm">
        {{ errorMessage }}
      </div>

      <!-- Success Message -->
      <div v-if="successMessage" class="bg-status-success/50 border border-status-success/70 text-status-success px-4 py-3 rounded-lg text-sm">
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

      </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import { useMfaSetupStore } from '@/stores/mfaSetup';
import MFASettings from '@/components/settings/MFASettings.vue';
import LogoIcon from '@/components/icons/LogoIcon.vue';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const mfaSetupStore = useMfaSetupStore();

const errorMessage = ref('');
const successMessage = ref('');

// Template ref for MFASettings component
const mfaSettingsRef = ref();

// Security check and credential setup
onMounted(async () => {
  console.log('🔍 MFA Setup - Checking for credentials:', {
    hasValidCredentials: mfaSetupStore.hasValidCredentials,
    isAuthenticated: !!authStore.user,
    route: route.fullPath
  });

  // If user is already fully authenticated, redirect to dashboard
  if (authStore.user && !authStore.mfaSetupRequired) {
    console.log('✅ User already authenticated, redirecting to dashboard');
    mfaSetupStore.clearCredentials();
    router.push('/');
    return;
  }

  // Check for valid credentials in the secure store
  if (mfaSetupStore.hasValidCredentials) {
    const creds = mfaSetupStore.getCredentials;
    if (creds) {
      console.log('✅ Found valid setup credentials from:', creds.source);
      // Credentials are available in the store for MFASettings component
      return;
    }
  }

  // No valid credentials - redirect back to login
  if (authStore.mfaSetupRequired && authStore.mfaUserUuid) {
    console.log('🔄 No credentials found, but auth store indicates MFA setup required');
    errorMessage.value = 'Session expired. Please log in again to set up MFA.';
    setTimeout(() => {
      mfaSetupStore.clearCredentials();
      authStore.clearMfaState();
      router.push('/login');
    }, 3000);
  } else {
    console.error('❌ No valid credentials found');
    errorMessage.value = 'Invalid access. Redirecting to login...';
    setTimeout(() => {
      mfaSetupStore.clearCredentials();
      router.push('/login');
    }, 2000);
  }
});

// Clean up credentials when leaving the page
onUnmounted(() => {
  // Only clear if navigating away without completing setup
  if (!authStore.user) {
    mfaSetupStore.clearCredentials();
  }
});

// Handle successful MFA setup from the component
const handleMfaSetupSuccess = async (message: string) => {
  // Only redirect if the setup is actually complete
  if (message === 'setup-complete') {
    // Clean up credentials from the secure store
    mfaSetupStore.clearCredentials();

    // Redirect to dashboard immediately
    router.push('/');
  }
  // For other success messages (like "MFA setup initiated"), don't redirect
};

// Handle MFA setup errors from the component
const handleMfaSetupError = (error: string) => {
  errorMessage.value = error;
};

// Navigation back to login
const goBackToLogin = () => {
  mfaSetupStore.clearCredentials();
  authStore.clearMfaState();
  router.push('/login');
};
</script>

