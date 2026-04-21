<template>
  <div class="min-h-screen w-full flex flex-col items-center bg-app py-8 px-4 sm:px-6 gap-6">
    <!-- Brand -->
    <RouterLink
      to="/"
      class="flex items-center justify-center"
      :aria-label="`${appName} home`"
    >
      <img
        v-if="customLogoUrl"
        :src="customLogoUrl"
        :alt="appName"
        class="h-10 max-w-[240px] object-contain"
      />
      <LogoIcon v-else class="h-10 text-accent" :aria-label="`${appName} Logo`" />
    </RouterLink>

    <div class="w-full max-w-md mx-auto flex flex-col gap-4">
      <!-- Heading (reactive to context) -->
      <div class="flex flex-col gap-1 text-center">
        <h1 class="text-2xl font-bold text-primary">{{ heading }}</h1>
        <p class="text-sm text-secondary">{{ subheading }}</p>
      </div>

      <!-- Loading -->
      <div
        v-if="validating"
        class="bg-surface rounded-xl border border-default shadow-sm p-8 flex flex-col items-center gap-3"
      >
        <svg class="w-6 h-6 animate-spin text-accent" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
          <path class="opacity-75" fill="currentColor"
            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
        </svg>
        <p class="text-sm text-secondary">Checking your link…</p>
      </div>

      <!-- Error (invalid / expired link) -->
      <div
        v-else-if="errorMessage && !acceptSuccess"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-status-error-muted flex items-center justify-center">
          <svg class="w-6 h-6 text-status-error" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
              d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ invalidTitle }}</h2>
          <p class="text-sm text-secondary">{{ errorMessage }}</p>
        </div>
        <button
          @click="goToLogin"
          class="px-4 py-2 bg-accent hover:opacity-90 text-white rounded-lg text-sm font-medium transition-colors"
        >
          Go to sign in
        </button>
      </div>

      <!-- Logging in (spinner) -->
      <div
        v-else-if="acceptSuccess && loggingIn && !loginComplete"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-accent-muted flex items-center justify-center">
          <svg class="w-6 h-6 text-accent animate-spin" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor"
              d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ successTitleActivating }}</h2>
          <p class="text-sm text-secondary">{{ loginMessage || 'Signing you in…' }}</p>
        </div>
      </div>

      <!-- Logged in -->
      <div
        v-else-if="acceptSuccess && loginComplete"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-status-success-muted flex items-center justify-center">
          <svg class="w-6 h-6 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ successTitleComplete }}</h2>
          <p class="text-sm text-secondary">{{ loginMessage }}</p>
        </div>
      </div>

      <!-- Manual-login fallback -->
      <div
        v-else-if="acceptSuccess && !loggingIn"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-status-success-muted flex items-center justify-center">
          <svg class="w-6 h-6 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ successTitleComplete }}</h2>
          <p class="text-sm text-secondary">{{ loginMessage || 'Please sign in with the password you just set.' }}</p>
        </div>
        <button
          @click="goToLogin"
          class="px-4 py-2 bg-accent hover:opacity-90 text-white rounded-lg text-sm font-medium transition-colors"
        >
          Go to sign in
        </button>
      </div>

      <!-- Password form -->
      <div
        v-else
        class="bg-surface rounded-xl border border-default shadow-sm overflow-hidden"
      >
        <!-- Identity summary -->
        <div
          v-if="userEmail"
          class="px-5 sm:px-6 py-4 bg-surface-alt border-b border-default flex items-center gap-3"
        >
          <div class="shrink-0 w-9 h-9 rounded-full bg-accent-muted flex items-center justify-center text-accent font-semibold text-sm">
            {{ initials }}
          </div>
          <div class="flex-1 min-w-0 flex flex-col gap-0.5">
            <div v-if="userName" class="text-sm font-medium text-primary truncate">{{ userName }}</div>
            <div class="text-xs text-tertiary truncate">{{ userEmail }}</div>
          </div>
        </div>

        <form @submit.prevent="handleSubmit" class="p-5 sm:p-6 flex flex-col gap-4">
          <!-- New password -->
          <div class="flex flex-col gap-2">
            <label for="new-password" class="text-sm font-medium text-primary">Password</label>
            <div class="relative">
              <input
                id="new-password"
                v-model="newPassword"
                :type="showPassword ? 'text' : 'password'"
                required
                autocomplete="new-password"
                placeholder="At least 8 characters"
                class="w-full px-3 py-2.5 pr-11 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent transition-colors"
                :disabled="loading"
                @input="validatePassword"
              />
              <button
                type="button"
                @click="showPassword = !showPassword"
                class="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 text-tertiary hover:text-primary transition-colors"
                :aria-label="showPassword ? 'Hide password' : 'Show password'"
                tabindex="-1"
              >
                <svg v-if="showPassword" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                </svg>
                <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21" />
                </svg>
              </button>
            </div>
            <p
              class="flex items-center gap-1.5 text-xs transition-colors"
              :class="passwordValidation.length ? 'text-status-success' : 'text-tertiary'"
            >
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  :d="passwordValidation.length ? 'M5 13l4 4L19 7' : 'M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z'"
                />
              </svg>
              At least 8 characters
            </p>
          </div>

          <!-- Confirm password -->
          <div class="flex flex-col gap-2">
            <label for="confirm-password" class="text-sm font-medium text-primary">
              Confirm password
            </label>
            <div class="relative">
              <input
                id="confirm-password"
                v-model="confirmPassword"
                :type="showConfirmPassword ? 'text' : 'password'"
                required
                autocomplete="new-password"
                placeholder="Enter it again"
                class="w-full px-3 py-2.5 pr-11 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent transition-colors"
                :disabled="loading"
                @input="validatePasswordMatch"
              />
              <button
                type="button"
                @click="showConfirmPassword = !showConfirmPassword"
                class="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 text-tertiary hover:text-primary transition-colors"
                :aria-label="showConfirmPassword ? 'Hide password' : 'Show password'"
                tabindex="-1"
              >
                <svg v-if="showConfirmPassword" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                </svg>
                <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21" />
                </svg>
              </button>
            </div>
            <p
              v-if="confirmPassword"
              class="flex items-center gap-1.5 text-xs transition-colors"
              :class="passwordsMatch ? 'text-status-success' : 'text-status-error'"
            >
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  :d="passwordsMatch ? 'M5 13l4 4L19 7' : 'M6 18L18 6M6 6l12 12'"
                />
              </svg>
              {{ passwordsMatch ? 'Passwords match' : 'Passwords do not match' }}
            </p>
          </div>

          <div
            v-if="submitError"
            role="alert"
            class="bg-status-error-muted border border-status-error/40 text-status-error rounded-lg px-3 py-2.5 text-sm flex items-start gap-2"
          >
            <svg class="w-4 h-4 shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span>{{ submitError }}</span>
          </div>

          <button
            type="submit"
            class="inline-flex items-center justify-center gap-2 px-4 py-2.5 bg-accent hover:opacity-90 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            :disabled="loading || !isFormValid"
          >
            <svg
              v-if="loading"
              class="w-4 h-4 animate-spin"
              fill="none"
              viewBox="0 0 24 24"
            >
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            {{ loading ? submitLoadingLabel : submitLabel }}
          </button>
        </form>
      </div>

      <!-- Back link -->
      <button
        v-if="!acceptSuccess && !validating && !loggingIn"
        @click="goToLogin"
        class="self-center inline-flex items-center gap-1.5 text-xs text-tertiary hover:text-primary transition-colors"
      >
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
        </svg>
        Back to sign in
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useRouter, useRoute, RouterLink } from 'vue-router';
import authService from '@/services/authService';
import { useAutoLogin } from '@/composables/useAutoLogin';
import { usePasswordForm } from '@/composables/usePasswordForm';
import { useBrandingStore } from '@/stores/branding';
import { useThemeStore } from '@/stores/theme';
import LogoIcon from '@/components/icons/LogoIcon.vue';

const router = useRouter();
const route = useRoute();
const brandingStore = useBrandingStore();
const themeStore = useThemeStore();

const {
  isLoggingIn: loggingIn,
  isComplete: loginComplete,
  message: loginMessage,
  attemptLogin
} = useAutoLogin({ source: 'invitation' });

const {
  newPassword,
  confirmPassword,
  showPassword,
  showConfirmPassword,
  passwordValidation,
  passwordsMatch,
  isFormValid,
  validatePassword,
  validatePasswordMatch,
  clearSensitiveData
} = usePasswordForm();

const loading = ref(false);
const validating = ref(true);
const acceptSuccess = ref(false);
const errorMessage = ref('');
const submitError = ref('');
const userEmail = ref('');
const userName = ref('');
const context = ref<'guest_ticket' | 'invitation' | string>('invitation');
const token = ref('');

const appName = computed(() => brandingStore.appName);
const customLogoUrl = computed(() =>
  brandingStore.getLogoUrl(themeStore.isDarkMode)
);
const isGuestTicket = computed(() => context.value === 'guest_ticket');

const initials = computed(() => {
  const source = userName.value?.trim() || userEmail.value?.trim() || '';
  if (!source) return '?';
  const parts = source.split(/\s+/).filter(Boolean);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return source.slice(0, 2).toUpperCase();
});

// Copy swaps for the two contexts. Keeping these as computeds — the
// template reads them and re-renders when the token validation resolves.
const heading = computed(() =>
  validating.value
    ? 'Just a moment…'
    : isGuestTicket.value
    ? 'Confirm your ticket submission'
    : `Welcome to ${appName.value}`
);

const subheading = computed(() => {
  if (validating.value) return 'Verifying your link.';
  // The identity summary shows the email and the password label carries the
  // instruction — the subheading stays short and contextual.
  if (isGuestTicket.value) return 'Set a password to release your ticket.';
  return 'Finish setting up your account.';
});

const invalidTitle = computed(() =>
  isGuestTicket.value ? 'This confirmation link is no longer valid' : 'Invitation invalid'
);

const successTitleActivating = computed(() =>
  isGuestTicket.value ? 'Releasing your ticket…' : 'Activating your account…'
);
const successTitleComplete = computed(() =>
  isGuestTicket.value ? "You're all set" : `Welcome to ${appName.value}`
);

const submitLabel = computed(() =>
  isGuestTicket.value ? 'Confirm & release ticket' : 'Activate account'
);
const submitLoadingLabel = computed(() =>
  isGuestTicket.value ? 'Confirming…' : 'Activating…'
);

onMounted(async () => {
  if (!brandingStore.isLoaded) {
    brandingStore.loadBranding();
  }

  token.value = (route.query.token as string) || '';

  if (!token.value) {
    errorMessage.value = 'Invalid or missing confirmation link.';
    validating.value = false;
    return;
  }

  try {
    const response = await authService.validateInvitation(token.value);
    if (response.valid) {
      userEmail.value = response.user_email || '';
      userName.value = response.user_name || '';
      context.value = response.context ?? 'invitation';
    } else {
      errorMessage.value =
        response.message || 'This link is invalid or has expired.';
    }
  } catch (error) {
    console.error('Invitation validation error:', error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value =
      axiosError.response?.data?.message ||
      'Failed to validate link. Please try again later.';
  } finally {
    validating.value = false;
  }
});

const handleSubmit = async () => {
  if (!isFormValid.value || !token.value) return;
  submitError.value = '';
  loading.value = true;
  try {
    await authService.acceptInvitation(token.value, newPassword.value);
    acceptSuccess.value = true;

    const success = await attemptLogin(userEmail.value, newPassword.value);
    if (success) clearSensitiveData();
  } catch (error) {
    console.error('Accept invitation error:', error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    submitError.value =
      axiosError.response?.data?.message ||
      'Failed to complete confirmation. The link may have expired.';
  } finally {
    loading.value = false;
  }
};

const goToLogin = () => {
  clearSensitiveData();
  router.push('/login');
};
</script>
