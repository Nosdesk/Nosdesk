<template>
  <div class="fixed inset-0 bg-app overflow-y-auto">
    <div class="min-h-full flex flex-col items-center justify-center py-8 sm:py-12 px-4 sm:px-8">
      <div class="flex flex-col gap-6 w-full max-w-4xl">
      <!-- Header -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-12 px-4 text-accent" aria-label="Nosdesk Logo" />
        <h1 class="text-2xl font-bold text-primary mt-4 text-center">
          {{ headerTitle }}
        </h1>
        <p class="text-secondary text-center">
          {{ headerSubtitle }}
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

      <!-- Method Choice -->
      <div v-if="mfaMethod === 'choose'" class="flex flex-col gap-4">
        <button
          @click="mfaMethod = 'totp'"
          class="flex flex-row items-center gap-3 sm:gap-4 p-4 sm:p-5 bg-surface border border-default rounded-lg hover:bg-surface-hover hover:border-accent transition-colors text-left w-full"
        >
          <div class="flex-shrink-0 h-12 w-12 sm:h-14 sm:w-14 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg class="w-6 h-6 sm:w-7 sm:h-7 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M10.5 1.5H8.25A2.25 2.25 0 006 3.75v16.5a2.25 2.25 0 002.25 2.25h7.5A2.25 2.25 0 0018 20.25V3.75a2.25 2.25 0 00-2.25-2.25H13.5m-3 0V3h3V1.5m-3 0h3m-3 18.75h3" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="text-primary font-medium text-base sm:text-lg">Authenticator App</h3>
            <p class="text-sm text-secondary">Use an app like Google Authenticator, Authy, or 1Password to generate time-based codes</p>
          </div>
          <svg class="flex-shrink-0 w-5 h-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>

        <button
          @click="mfaMethod = 'passkey'"
          class="flex flex-row items-center gap-3 sm:gap-4 p-4 sm:p-5 bg-surface border border-default rounded-lg hover:bg-surface-hover hover:border-accent transition-colors text-left w-full"
        >
          <div class="flex-shrink-0 h-12 w-12 sm:h-14 sm:w-14 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg class="w-6 h-6 sm:w-7 sm:h-7 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M7.864 4.243A7.5 7.5 0 0119.5 10.5c0 2.92-.556 5.709-1.568 8.268M5.742 6.364A7.465 7.465 0 004.5 10.5a7.464 7.464 0 01-1.15 3.993m1.989 3.559A11.209 11.209 0 008.25 10.5a3.75 3.75 0 117.5 0c0 .527-.021 1.049-.064 1.565M12 10.5a14.94 14.94 0 01-3.6 9.75m6.633-4.596a18.666 18.666 0 01-2.485 5.33" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="text-primary font-medium text-base sm:text-lg">Passkey</h3>
            <p class="text-sm text-secondary">Use biometrics like Face ID, Touch ID, or a hardware security key for passwordless login</p>
          </div>
          <svg class="flex-shrink-0 w-5 h-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>

        <!-- Info Notice -->
        <div class="bg-surface border border-default rounded-lg p-3 sm:p-4 text-sm text-secondary">
          <div class="flex flex-row items-start gap-3">
            <svg class="w-5 h-5 text-accent mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
            </svg>
            <div class="flex-1 min-w-0">
              <h4 class="font-medium text-primary mb-1 text-sm">Which should I choose?</h4>
              <p class="text-xs text-tertiary">
                <strong>Passkeys</strong> are more secure and convenient - just use your fingerprint or face.
                <strong>Authenticator apps</strong> work on any device and don't require biometrics.
              </p>
            </div>
          </div>
        </div>
      </div>

      <!-- MFA Settings Component (TOTP) -->
      <div v-else-if="mfaMethod === 'totp'" class="bg-surface rounded-xl border border-subtle">
        <MFASettings
          ref="mfaSettingsRef"
          :is-login-setup="true"
          @success="handleMfaSetupSuccess"
          @error="handleMfaSetupError"
        />
      </div>

      <!-- Passkey Setup -->
      <div v-else-if="mfaMethod === 'passkey'" class="bg-surface rounded-xl border border-subtle">
        <PasskeySetup
          :is-login-setup="true"
          @success="handlePasskeySetupSuccess"
          @error="handlePasskeySetupError"
        />
      </div>

      <!-- Offer Passkey (after TOTP setup) -->
      <div v-else-if="mfaMethod === 'offer-passkey'" class="flex flex-col gap-4">
        <div class="bg-surface rounded-xl border border-default p-6">
          <div class="flex flex-col items-center text-center gap-4">
            <div class="w-16 h-16 rounded-full bg-status-success/10 flex items-center justify-center">
              <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
              </svg>
            </div>
            <div>
              <h3 class="text-lg font-semibold text-primary mb-2">Authenticator App Set Up!</h3>
              <p class="text-secondary text-sm">
                Would you also like to add a passkey for faster, passwordless sign-in?
              </p>
            </div>
          </div>
        </div>

        <button
          @click="mfaMethod = 'passkey-additional'"
          class="flex flex-row items-center gap-3 sm:gap-4 p-4 bg-surface border border-default rounded-lg hover:bg-surface-hover hover:border-accent transition-colors text-left w-full"
        >
          <div class="flex-shrink-0 h-12 w-12 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg class="w-6 h-6 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M7.864 4.243A7.5 7.5 0 0119.5 10.5c0 2.92-.556 5.709-1.568 8.268M5.742 6.364A7.465 7.465 0 004.5 10.5a7.464 7.464 0 01-1.15 3.993m1.989 3.559A11.209 11.209 0 008.25 10.5a3.75 3.75 0 117.5 0c0 .527-.021 1.049-.064 1.565M12 10.5a14.94 14.94 0 01-3.6 9.75m6.633-4.596a18.666 18.666 0 01-2.485 5.33" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="text-primary font-medium">Add a Passkey</h3>
            <p class="text-sm text-secondary">Use Face ID, Touch ID, or a security key</p>
          </div>
          <svg class="flex-shrink-0 w-5 h-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>

        <button
          @click="finishSetup"
          class="w-full py-3 px-4 border border-default text-secondary rounded-lg hover:bg-surface-hover transition-colors text-sm"
        >
          Skip for now
        </button>
      </div>

      <!-- Offer TOTP (after Passkey setup) -->
      <div v-else-if="mfaMethod === 'offer-totp'" class="flex flex-col gap-4">
        <div class="bg-surface rounded-xl border border-default p-6">
          <div class="flex flex-col items-center text-center gap-4">
            <div class="w-16 h-16 rounded-full bg-status-success/10 flex items-center justify-center">
              <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
              </svg>
            </div>
            <div>
              <h3 class="text-lg font-semibold text-primary mb-2">Passkey Created!</h3>
              <p class="text-secondary text-sm">
                Would you also like to set up an authenticator app as a backup method?
              </p>
            </div>
          </div>
        </div>

        <button
          @click="mfaMethod = 'totp-additional'"
          class="flex flex-row items-center gap-3 sm:gap-4 p-4 bg-surface border border-default rounded-lg hover:bg-surface-hover hover:border-accent transition-colors text-left w-full"
        >
          <div class="flex-shrink-0 h-12 w-12 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg class="w-6 h-6 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M10.5 1.5H8.25A2.25 2.25 0 006 3.75v16.5a2.25 2.25 0 002.25 2.25h7.5A2.25 2.25 0 0018 20.25V3.75a2.25 2.25 0 00-2.25-2.25H13.5m-3 0V3h3V1.5m-3 0h3m-3 18.75h3" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="text-primary font-medium">Set Up Authenticator App</h3>
            <p class="text-sm text-secondary">Use as a backup if you lose access to your passkey</p>
          </div>
          <svg class="flex-shrink-0 w-5 h-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>

        <button
          @click="finishSetup"
          class="w-full py-3 px-4 border border-default text-secondary rounded-lg hover:bg-surface-hover transition-colors text-sm"
        >
          Skip for now
        </button>
      </div>

      <!-- Additional Passkey Setup (after TOTP) -->
      <div v-else-if="mfaMethod === 'passkey-additional'" class="bg-surface rounded-xl border border-subtle">
        <PasskeySetup
          :is-login-setup="false"
          @success="handleAdditionalSetupSuccess"
          @error="handlePasskeySetupError"
        />
      </div>

      <!-- Additional TOTP Setup (after Passkey) -->
      <div v-else-if="mfaMethod === 'totp-additional'" class="bg-surface rounded-xl border border-subtle">
        <MFASettings
          ref="mfaSettingsRef"
          :is-login-setup="false"
          @success="handleAdditionalSetupSuccess"
          @error="handleMfaSetupError"
        />
      </div>

      <!-- Navigation -->
      <div class="flex justify-between items-center">
        <button
          v-if="mfaMethod === 'choose'"
          @click="goBackToLogin"
          class="flex items-center gap-2 px-4 py-2 text-sm text-tertiary hover:text-primary transition-colors"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
          </svg>
          Back to Login
        </button>
        <button
          v-else-if="showBackButton"
          @click="handleBack"
          class="flex items-center gap-2 px-4 py-2 text-sm text-tertiary hover:text-primary transition-colors"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
          </svg>
          {{ backButtonText }}
        </button>
        <div v-else></div>
      </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import { useMfaSetupStore } from '@/stores/mfaSetup';
import MFASettings from '@/components/settings/MFASettings.vue';
import PasskeySetup from '@/components/auth/PasskeySetup.vue';
import LogoIcon from '@/components/icons/LogoIcon.vue';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const mfaSetupStore = useMfaSetupStore();

const errorMessage = ref('');
const successMessage = ref('');

// MFA method states
type MfaMethodState = 'choose' | 'totp' | 'passkey' | 'offer-passkey' | 'offer-totp' | 'passkey-additional' | 'totp-additional';
const mfaMethod = ref<MfaMethodState>('choose');

// Template ref for MFASettings component
const mfaSettingsRef = ref();

// Computed header content
const headerTitle = computed(() => {
  switch (mfaMethod.value) {
    case 'offer-passkey':
    case 'offer-totp':
      return 'Add Another Method?';
    case 'passkey-additional':
    case 'totp-additional':
      return 'Add Backup Method';
    default:
      return 'Complete Your Account Setup';
  }
});

const headerSubtitle = computed(() => {
  switch (mfaMethod.value) {
    case 'choose':
      return 'Choose your preferred authentication method';
    case 'offer-passkey':
      return 'Passkeys provide a faster, passwordless sign-in experience';
    case 'offer-totp':
      return 'An authenticator app provides a backup if you lose your passkey';
    case 'passkey-additional':
      return 'Set up a passkey for faster sign-in';
    case 'totp-additional':
      return 'Set up an authenticator app as a backup';
    default:
      return 'Your account type requires multi-factor authentication for security';
  }
});

const showBackButton = computed(() => {
  return ['totp', 'passkey', 'passkey-additional', 'totp-additional'].includes(mfaMethod.value);
});

const backButtonText = computed(() => {
  if (mfaMethod.value === 'passkey-additional' || mfaMethod.value === 'totp-additional') {
    return 'Skip';
  }
  return 'Choose Different Method';
});

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
  if (!authStore.user) {
    mfaSetupStore.clearCredentials();
  }
});

// Handle successful MFA setup
const handleMfaSetupSuccess = async (message: string) => {
  if (message === 'setup-complete') {
    // Show offer to add passkey
    mfaMethod.value = 'offer-passkey';
    successMessage.value = '';
    errorMessage.value = '';
  }
};

// Handle MFA setup errors
const handleMfaSetupError = (error: string) => {
  errorMessage.value = error;
};

// Handle successful passkey setup
const handlePasskeySetupSuccess = async (message: string) => {
  if (message === 'setup-complete') {
    // Show offer to add TOTP
    mfaMethod.value = 'offer-totp';
    successMessage.value = '';
    errorMessage.value = '';
  }
};

// Handle passkey setup errors
const handlePasskeySetupError = (error: string) => {
  errorMessage.value = error;
};

// Handle additional method setup success
const handleAdditionalSetupSuccess = async (message: string) => {
  if (message === 'setup-complete' || message === 'Passkey created successfully') {
    finishSetup();
  }
};

// Handle back button
const handleBack = () => {
  if (mfaMethod.value === 'passkey-additional' || mfaMethod.value === 'totp-additional') {
    finishSetup();
  } else {
    mfaMethod.value = 'choose';
  }
};

// Finish setup and redirect
const finishSetup = () => {
  mfaSetupStore.clearCredentials();
  router.push('/');
};

// Navigation back to login
const goBackToLogin = () => {
  mfaSetupStore.clearCredentials();
  authStore.clearMfaState();
  router.push('/login');
};
</script>
