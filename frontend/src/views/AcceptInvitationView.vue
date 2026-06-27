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
        <Spinner size="lg" class="text-accent" />
        <p class="text-sm text-secondary">{{ $t('accept-invitation-checking') }}</p>
      </div>

      <!-- Error (invalid / expired link) -->
      <div
        v-else-if="errorMessage && !acceptSuccess"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-status-error-muted flex items-center justify-center">
          <Icon name="warning" size="lg" class="text-status-error" />
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ invalidTitle }}</h2>
          <p class="text-sm text-secondary">{{ errorMessage }}</p>
        </div>
        <button
          @click="goToLogin"
          class="px-4 py-2 bg-accent hover:opacity-90 text-on-accent rounded-lg text-sm font-medium transition-colors"
        >
          {{ $t('accept-invitation-go-to-signin') }}
        </button>
      </div>

      <!-- Logging in (spinner) -->
      <div
        v-else-if="acceptSuccess && loggingIn && !loginComplete"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-accent-muted flex items-center justify-center">
          <Spinner size="lg" class="text-accent" />
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ successTitleActivating }}</h2>
          <p class="text-sm text-secondary">{{ loginMessage || $t('accept-invitation-signing-in') }}</p>
        </div>
      </div>

      <!-- Logged in -->
      <div
        v-else-if="acceptSuccess && loginComplete"
        class="bg-surface rounded-xl border border-default shadow-sm p-6 sm:p-8 flex flex-col items-center gap-4 text-center"
      >
        <div class="w-12 h-12 rounded-full bg-status-success-muted flex items-center justify-center">
          <Icon name="checkCircle" size="lg" class="text-status-success" />
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
          <Icon name="checkCircle" size="lg" class="text-status-success" />
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ successTitleComplete }}</h2>
          <p class="text-sm text-secondary">{{ loginMessage || $t('accept-invitation-manual-login') }}</p>
        </div>
        <button
          @click="goToLogin"
          class="px-4 py-2 bg-accent hover:opacity-90 text-on-accent rounded-lg text-sm font-medium transition-colors"
        >
          {{ $t('accept-invitation-go-to-signin') }}
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
            <label for="new-password" class="text-sm font-medium text-primary">{{ $t('accept-invitation-password-label') }}</label>
            <div class="relative">
              <input
                id="new-password"
                v-model="newPassword"
                :type="showPassword ? 'text' : 'password'"
                required
                autocomplete="new-password"
                :placeholder="$t('accept-invitation-password-placeholder')"
                class="w-full px-3 py-2.5 pr-11 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent transition-colors"
                :disabled="loading"
                @input="validatePassword"
              />
              <button
                type="button"
                @click="showPassword = !showPassword"
                class="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 text-tertiary hover:text-primary transition-colors"
                :aria-label="showPassword ? $t('accept-invitation-hide-password') : $t('accept-invitation-show-password')"
                tabindex="-1"
              >
                <Icon v-if="showPassword" name="eye" size="md" />
                <Icon v-else name="eyeOff" size="md" />
              </button>
            </div>
            <p
              class="flex items-center gap-1.5 text-xs transition-colors"
              :class="passwordValidation.length ? 'text-status-success' : 'text-tertiary'"
            >
              <Icon :name="passwordValidation.length ? 'check' : 'warning'" />
              {{ $t('accept-invitation-req-length') }}
            </p>
          </div>

          <!-- Confirm password -->
          <div class="flex flex-col gap-2">
            <label for="confirm-password" class="text-sm font-medium text-primary">
              {{ $t('accept-invitation-confirm-label') }}
            </label>
            <div class="relative">
              <input
                id="confirm-password"
                v-model="confirmPassword"
                :type="showConfirmPassword ? 'text' : 'password'"
                required
                autocomplete="new-password"
                :placeholder="$t('accept-invitation-confirm-placeholder')"
                class="w-full px-3 py-2.5 pr-11 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent transition-colors"
                :disabled="loading"
                @input="validatePasswordMatch"
              />
              <button
                type="button"
                @click="showConfirmPassword = !showConfirmPassword"
                class="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 text-tertiary hover:text-primary transition-colors"
                :aria-label="showConfirmPassword ? $t('accept-invitation-hide-password') : $t('accept-invitation-show-password')"
                tabindex="-1"
              >
                <Icon v-if="showConfirmPassword" name="eye" size="md" />
                <Icon v-else name="eyeOff" size="md" />
              </button>
            </div>
            <p
              v-if="confirmPassword"
              class="flex items-center gap-1.5 text-xs transition-colors"
              :class="passwordsMatch ? 'text-status-success' : 'text-status-error'"
            >
              <Icon :name="passwordsMatch ? 'check' : 'close'" />
              {{ passwordsMatch ? $t('accept-invitation-match-yes') : $t('accept-invitation-match-no') }}
            </p>
          </div>

          <div
            v-if="submitError"
            role="alert"
            class="bg-status-error-muted border border-status-error/40 text-status-error rounded-lg px-3 py-2.5 text-sm flex items-start gap-2"
          >
            <Icon name="warning" class="shrink-0 mt-0.5" />
            <span>{{ submitError }}</span>
          </div>

          <button
            type="submit"
            class="inline-flex items-center justify-center gap-2 px-4 py-2.5 bg-accent hover:opacity-90 text-on-accent rounded-lg text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            :disabled="loading || !isFormValid"
          >
            <Spinner v-if="loading" />
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
        <Icon name="chevronLeft" />
        {{ $t('accept-invitation-back-to-signin') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useRouter, useRoute, RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import authService from '@nosdesk/core/services/authService';
import { useAutoLogin } from '@/composables/useAutoLogin';
import { usePasswordForm } from '@/composables/usePasswordForm';
import { useBrandingStore } from '@/stores/branding';
import { useThemeStore } from '@/stores/theme';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import { extractErrorMessage } from '@/utils/errors';

const router = useRouter();
const route = useRoute();
const brandingStore = useBrandingStore();
const themeStore = useThemeStore();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args);

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
    ? t('accept-invitation-heading-validating')
    : isGuestTicket.value
    ? t('accept-invitation-heading-guest')
    : t('accept-invitation-heading-welcome', { app: appName.value })
);

const subheading = computed(() => {
  if (validating.value) return t('accept-invitation-subheading-validating');
  if (isGuestTicket.value) return t('accept-invitation-subheading-guest');
  return t('accept-invitation-subheading-invitation');
});

const invalidTitle = computed(() =>
  isGuestTicket.value
    ? t('accept-invitation-invalid-title-guest')
    : t('accept-invitation-invalid-title-invitation')
);

const successTitleActivating = computed(() =>
  isGuestTicket.value
    ? t('accept-invitation-activating-title-guest')
    : t('accept-invitation-activating-title-invitation')
);
const successTitleComplete = computed(() =>
  isGuestTicket.value
    ? t('accept-invitation-success-title-guest')
    : t('accept-invitation-success-title-invitation', { app: appName.value })
);

const submitLabel = computed(() =>
  isGuestTicket.value
    ? t('accept-invitation-submit-guest')
    : t('accept-invitation-submit-invitation')
);
const submitLoadingLabel = computed(() =>
  isGuestTicket.value
    ? t('accept-invitation-submit-loading-guest')
    : t('accept-invitation-submit-loading-invitation')
);

onMounted(async () => {
  if (!brandingStore.isLoaded) {
    brandingStore.loadBranding();
  }

  token.value = (route.query.token as string) || '';

  if (!token.value) {
    errorMessage.value = t('accept-invitation-error-missing-token');
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
        response.message || t('accept-invitation-error-default');
    }
  } catch (error) {
    console.error('Invitation validation error:', error);
    errorMessage.value = extractErrorMessage(error, t('accept-invitation-error-validation-failed'));
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
    submitError.value = extractErrorMessage(error, t('accept-invitation-error-submit'));
  } finally {
    loading.value = false;
  }
};

const goToLogin = () => {
  clearSensitiveData();
  router.push('/login');
};
</script>
