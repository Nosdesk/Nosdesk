<template>
  <AuthLayout wide>
    <template #logo>
      <LogoIcon class="h-9 w-auto text-accent" :aria-label="$t('nav-logo-alt')" />
    </template>

    <div class="flex flex-col gap-6">
      <!-- Header -->
      <header class="flex flex-col gap-1.5">
        <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
          {{ headerTitle }}
        </h1>
        <p class="text-base text-secondary">{{ headerSubtitle }}</p>
      </header>

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
      </div>

      <!-- MFA Settings Component (TOTP). The component owns its own card
           chrome (SectionCard), so no extra wrapper here. -->
      <MFASettings
        v-else-if="mfaMethod === 'totp'"
        ref="mfaSettingsRef"
        bare
        :is-login-setup="true"
        @success="handleMfaSetupSuccess"
        @error="handleMfaSetupError"
      />

      <!-- Passkey Setup (owns its own card chrome) -->
      <PasskeySetup
        v-else-if="mfaMethod === 'passkey'"
        :is-login-setup="true"
        @success="handlePasskeySetupSuccess"
        @error="handlePasskeySetupError"
      />

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

        <Button variant="secondary" block @click="finishSetup">
          {{ $t('mfa-setup-skip-now') }}
        </Button>
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

        <Button variant="secondary" block @click="finishSetup">
          {{ $t('mfa-setup-skip-now') }}
        </Button>
      </div>

      <!-- Additional Passkey Setup (after TOTP) -->
      <PasskeySetup
        v-else-if="mfaMethod === 'passkey-additional'"
        :is-login-setup="false"
        @success="handleAdditionalSetupSuccess"
        @error="handlePasskeySetupError"
      />

      <!-- Additional TOTP Setup (after Passkey) -->
      <MFASettings
        v-else-if="mfaMethod === 'totp-additional'"
        ref="mfaSettingsRef"
        bare
        :is-login-setup="false"
        @success="handleAdditionalSetupSuccess"
        @error="handleMfaSetupError"
      />

      <!-- Navigation -->
      <div class="flex justify-start items-center">
        <Button
          v-if="mfaMethod === 'choose'"
          variant="ghost"
          size="sm"
          icon="chevronLeft"
          @click="goBackToLogin"
        >
          {{ $t('mfa-setup-back-to-login') }}
        </Button>
        <Button
          v-else-if="showBackButton"
          variant="ghost"
          size="sm"
          icon="chevronLeft"
          @click="handleBack"
        >
          {{ backButtonText }}
        </Button>
      </div>
    </div>
  </AuthLayout>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { landAfterLogin } from '@/router';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import { useMfaSetupStore } from '@nosdesk/core/stores/mfaSetup';
import MFASettings from '@/components/settings/MFASettings.vue';
import PasskeySetup from '@/components/auth/PasskeySetup.vue';
import AuthLayout from '@/components/auth/AuthLayout.vue';
import Button from '@/components/common/Button.vue';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import Icon from '@/components/common/Icon.vue';

const router = useRouter();
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

  // If user is already fully authenticated, redirect to dashboard
  if (authStore.user && !authStore.mfaSetupRequired) {
    mfaSetupStore.clearCredentials();
    await landAfterLogin();
    return;
  }

  // Check for valid credentials in the secure store
  if (mfaSetupStore.hasValidCredentials) {
    const creds = mfaSetupStore.getCredentials;
    if (creds) {
      return;
    }
  }

  // No valid credentials - redirect back to login
  if (authStore.mfaSetupRequired && authStore.mfaUserUuid) {
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
    void finishSetup();
  }
};

// Handle back button
const handleBack = () => {
  if (mfaMethod.value === 'passkey-additional' || mfaMethod.value === 'totp-additional') {
    void finishSetup();
  } else {
    mfaMethod.value = 'choose';
  }
};

// Finish setup and redirect
const finishSetup = async () => {
  mfaSetupStore.clearCredentials();
  await landAfterLogin();
};

// Navigation back to login
const goBackToLogin = () => {
  mfaSetupStore.clearCredentials();
  authStore.clearMfaState();
  router.push('/login');
};
</script>
