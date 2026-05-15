<template>
  <div class="fixed inset-0 bg-app overflow-y-auto">
    <div class="min-h-full flex flex-col items-center justify-center py-8 sm:py-12 px-4 sm:px-8">
      <div class="flex flex-col gap-6 w-full max-w-4xl">
      <!-- Header -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-12 px-4 text-accent" :aria-label="$t('nav-logo-alt')" />
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
            <h3 class="text-primary font-medium text-base sm:text-lg">{{ $t('mfa-setup-totp-name') }}</h3>
            <p class="text-sm text-secondary">{{ $t('mfa-setup-totp-description') }}</p>
          </div>
          <Icon name="chevronRight" size="md" class="flex-shrink-0 text-tertiary" />
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
            <h3 class="text-primary font-medium text-base sm:text-lg">{{ $t('mfa-setup-passkey-name') }}</h3>
            <p class="text-sm text-secondary">{{ $t('mfa-setup-passkey-description') }}</p>
          </div>
          <Icon name="chevronRight" size="md" class="flex-shrink-0 text-tertiary" />
        </button>

        <!-- Info Notice -->
        <div class="bg-surface border border-default rounded-lg p-3 sm:p-4 text-sm text-secondary">
          <div class="flex flex-row items-start gap-3">
            <Icon name="info" size="md" class="text-accent mt-0.5 flex-shrink-0" />
            <div class="flex-1 min-w-0">
              <h4 class="font-medium text-primary mb-1 text-sm">{{ $t('mfa-setup-which-title') }}</h4>
              <p class="text-xs text-tertiary">
                <strong>{{ $t('mfa-setup-which-passkey-label') }}</strong> {{ $t('mfa-setup-which-passkey-body') }}
                <strong>{{ $t('mfa-setup-which-totp-label') }}</strong> {{ $t('mfa-setup-which-totp-body') }}
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
              <Icon name="check" size="lg" class="text-status-success" />
            </div>
            <div>
              <h3 class="text-lg font-semibold text-primary mb-2">{{ $t('mfa-setup-totp-success-title') }}</h3>
              <p class="text-secondary text-sm">
                {{ $t('mfa-setup-totp-success-body') }}
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
            <h3 class="text-primary font-medium">{{ $t('mfa-setup-add-passkey-title') }}</h3>
            <p class="text-sm text-secondary">{{ $t('mfa-setup-add-passkey-description') }}</p>
          </div>
          <Icon name="chevronRight" size="md" class="flex-shrink-0 text-tertiary" />
        </button>

        <button
          @click="finishSetup"
          class="w-full py-3 px-4 border border-default text-secondary rounded-lg hover:bg-surface-hover transition-colors text-sm"
        >
          {{ $t('mfa-setup-skip-now') }}
        </button>
      </div>

      <!-- Offer TOTP (after Passkey setup) -->
      <div v-else-if="mfaMethod === 'offer-totp'" class="flex flex-col gap-4">
        <div class="bg-surface rounded-xl border border-default p-6">
          <div class="flex flex-col items-center text-center gap-4">
            <div class="w-16 h-16 rounded-full bg-status-success/10 flex items-center justify-center">
              <Icon name="check" size="lg" class="text-status-success" />
            </div>
            <div>
              <h3 class="text-lg font-semibold text-primary mb-2">{{ $t('mfa-setup-passkey-success-title') }}</h3>
              <p class="text-secondary text-sm">
                {{ $t('mfa-setup-passkey-success-body') }}
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
            <h3 class="text-primary font-medium">{{ $t('mfa-setup-add-totp-title') }}</h3>
            <p class="text-sm text-secondary">{{ $t('mfa-setup-add-totp-description') }}</p>
          </div>
          <Icon name="chevronRight" size="md" class="flex-shrink-0 text-tertiary" />
        </button>

        <button
          @click="finishSetup"
          class="w-full py-3 px-4 border border-default text-secondary rounded-lg hover:bg-surface-hover transition-colors text-sm"
        >
          {{ $t('mfa-setup-skip-now') }}
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
          <Icon name="chevronLeft" />
          {{ $t('mfa-setup-back-to-login') }}
        </button>
        <button
          v-else-if="showBackButton"
          @click="handleBack"
          class="flex items-center gap-2 px-4 py-2 text-sm text-tertiary hover:text-primary transition-colors"
        >
          <Icon name="chevronLeft" />
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
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import { useMfaSetupStore } from '@/stores/mfaSetup';
import MFASettings from '@/components/settings/MFASettings.vue';
import PasskeySetup from '@/components/auth/PasskeySetup.vue';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import Icon from '@/components/common/Icon.vue';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const mfaSetupStore = useMfaSetupStore();
const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

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
      return t('mfa-setup-header-offer');
    case 'passkey-additional':
    case 'totp-additional':
      return t('mfa-setup-header-additional');
    default:
      return t('mfa-setup-header-default');
  }
});

const headerSubtitle = computed(() => {
  switch (mfaMethod.value) {
    case 'choose':
      return t('mfa-setup-subtitle-choose');
    case 'offer-passkey':
      return t('mfa-setup-subtitle-offer-passkey');
    case 'offer-totp':
      return t('mfa-setup-subtitle-offer-totp');
    case 'passkey-additional':
      return t('mfa-setup-subtitle-passkey-additional');
    case 'totp-additional':
      return t('mfa-setup-subtitle-totp-additional');
    default:
      return t('mfa-setup-subtitle-default');
  }
});

const showBackButton = computed(() => {
  return ['totp', 'passkey', 'passkey-additional', 'totp-additional'].includes(mfaMethod.value);
});

const backButtonText = computed(() => {
  if (mfaMethod.value === 'passkey-additional' || mfaMethod.value === 'totp-additional') {
    return t('mfa-setup-back-skip');
  }
  return t('mfa-setup-back-different');
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
    errorMessage.value = t('mfa-setup-error-session-expired');
    setTimeout(() => {
      mfaSetupStore.clearCredentials();
      authStore.clearMfaState();
      router.push('/login');
    }, 3000);
  } else {
    console.error('❌ No valid credentials found');
    errorMessage.value = t('mfa-setup-error-invalid-access');
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
