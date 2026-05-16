<template>
  <div class="min-h-screen w-full flex items-center justify-center bg-app p-4">
    <div class="flex flex-col gap-6 w-full max-w-md">
      <!-- Header -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-12 px-4 text-accent" :aria-label="$t('nav-logo-alt')" />
        <h1 class="text-2xl font-bold text-primary mt-4">{{ $t('password-reset-page-title') }}</h1>
        <p class="text-secondary text-center text-sm">
          {{ $t('password-reset-subtitle') }}
        </p>
      </div>

      <!-- Error Message -->
      <div
        v-if="errorMessage"
        class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm"
      >
        {{ errorMessage }}
      </div>

      <!-- Success State -->
      <div
        v-if="resetSuccess"
        class="bg-surface rounded-xl border border-default shadow-xl overflow-hidden"
      >
        <div class="p-8">
          <div class="flex flex-col items-center gap-4 text-center">
            <div class="bg-status-success/20 rounded-full p-4">
              <svg class="w-12 h-12 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"></path>
              </svg>
            </div>
            <div>
              <h2 class="text-xl font-semibold text-primary mb-2">{{ $t('password-reset-success-title') }}</h2>
              <p class="text-sm text-secondary">
                {{ $t('password-reset-success-body') }}
              </p>
            </div>
            <button
              @click="goToLogin"
              class="w-full px-6 py-3 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium mt-2"
            >
              {{ $t('password-reset-success-cta') }}
            </button>
          </div>
        </div>
      </div>

      <!-- Form State -->
      <div
        v-else
        class="bg-surface rounded-xl border border-default shadow-xl overflow-hidden"
      >
        <div class="p-8">
          <form @submit.prevent="handleSubmit" class="flex flex-col gap-4">
            <!-- New Password -->
            <div>
              <label for="new-password" class="block text-sm font-medium text-secondary mb-2">
                {{ $t('password-reset-field-new') }}
              </label>
              <div class="relative">
                <input
                  id="new-password"
                  v-model="newPassword"
                  :type="showPassword ? 'text' : 'password'"
                  required
                  autocomplete="new-password"
                  :placeholder="$t('password-reset-field-new-placeholder')"
                  class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors pr-12"
                  :disabled="loading"
                  @input="validatePassword"
                />
                <button
                  type="button"
                  @click="showPassword = !showPassword"
                  class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary hover:text-primary transition-colors p-1"
                  tabindex="-1"
                >
                  <Icon v-if="showPassword" name="eye" size="md" />
                  <Icon v-else name="eyeOff" size="md" />
                </button>
              </div>

              <!-- Password Requirements -->
              <div class="flex flex-col gap-1 mt-2 text-xs">
                <p
                  class="flex items-center gap-2 transition-colors"
                  :class="passwordValidation.length ? 'text-status-success' : 'text-tertiary'"
                >
                  <Icon :name="passwordValidation.length ? 'check' : 'close'" />
                  {{ $t('password-reset-req-length') }}
                </p>
              </div>
            </div>

            <!-- Confirm Password -->
            <div>
              <label for="confirm-password" class="block text-sm font-medium text-secondary mb-2">
                {{ $t('password-reset-field-confirm') }}
              </label>
              <div class="relative">
                <input
                  id="confirm-password"
                  v-model="confirmPassword"
                  :type="showConfirmPassword ? 'text' : 'password'"
                  required
                  autocomplete="new-password"
                  :placeholder="$t('password-reset-field-confirm-placeholder')"
                  class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors pr-12"
                  :disabled="loading"
                  @input="validatePasswordMatch"
                />
                <button
                  type="button"
                  @click="showConfirmPassword = !showConfirmPassword"
                  class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary hover:text-primary transition-colors p-1"
                  tabindex="-1"
                >
                  <Icon v-if="showConfirmPassword" name="eye" size="md" />
                  <Icon v-else name="eyeOff" size="md" />
                </button>
              </div>

              <!-- Password Match Indicator -->
              <p
                v-if="confirmPassword"
                class="mt-2 text-xs flex items-center gap-2 transition-colors"
                :class="passwordsMatch ? 'text-status-success' : 'text-status-error'"
              >
                <Icon :name="passwordsMatch ? 'check' : 'close'" />
                {{ passwordsMatch ? $t('password-reset-match-yes') : $t('password-reset-match-no') }}
              </p>
            </div>

            <!-- Submit Button -->
            <button
              type="submit"
              class="w-full px-6 py-3 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 mt-2"
              :disabled="loading || !isFormValid"
            >
              <Spinner v-if="loading" size="md" />
              <span>{{ loading ? $t('password-reset-submit-loading') : $t('password-reset-submit') }}</span>
            </button>
          </form>
        </div>
      </div>

      <!-- Back to Login -->
      <button
        v-if="!resetSuccess"
        @click="goToLogin"
        class="flex items-center justify-center gap-2 text-sm text-tertiary hover:text-primary transition-colors py-2"
      >
        <Icon name="chevronLeft" />
        {{ $t('password-reset-back-to-login') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import authService from '@/services/authService';
import { usePasswordForm } from '@/composables/usePasswordForm';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';

const router = useRouter();
const route = useRoute();
const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

// Password form composable (handles state, validation, cleanup)
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
} = usePasswordForm();

const loading = ref(false);
const resetSuccess = ref(false);
const errorMessage = ref('');

const token = ref('');

// Get token from URL query params
onMounted(() => {
  token.value = (route.query.token as string) || '';

  if (!token.value) {
    errorMessage.value = t('password-reset-error-no-token');
  }
});

const handleSubmit = async () => {
  if (!isFormValid.value || !token.value) {
    return;
  }

  errorMessage.value = '';
  loading.value = true;

  try {
    await authService.completePasswordReset(token.value, newPassword.value);
    resetSuccess.value = true;
  } catch (error) {
    console.error('Password reset error:', error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || t('password-reset-error-failed');
  } finally {
    loading.value = false;
  }
};

const goToLogin = () => {
  router.push('/login');
};
</script>
