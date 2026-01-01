<template>
  <div class="min-h-screen w-full flex items-center justify-center bg-app p-4">
    <div class="flex flex-col gap-6 w-full max-w-md">
      <!-- Header -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-8 px-4 text-accent" aria-label="Nosdesk Logo" />
        <h1 class="text-2xl font-bold text-primary mt-4">Reset Your Password</h1>
        <p class="text-secondary text-center text-sm">
          Enter your new password below
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
              <h2 class="text-xl font-semibold text-primary mb-2">Password Reset Complete!</h2>
              <p class="text-sm text-secondary">
                Your password has been successfully updated. You can now log in with your new password.
              </p>
            </div>
            <button
              @click="goToLogin"
              class="w-full px-6 py-3 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium mt-2"
            >
              Go to Login
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
                New Password
              </label>
              <div class="relative">
                <input
                  id="new-password"
                  v-model="newPassword"
                  :type="showPassword ? 'text' : 'password'"
                  required
                  autocomplete="new-password"
                  placeholder="Enter new password"
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
                  <svg v-if="showPassword" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"></path>
                  </svg>
                  <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21"></path>
                  </svg>
                </button>
              </div>

              <!-- Password Requirements -->
              <div class="mt-2 text-xs space-y-1">
                <p
                  class="flex items-center gap-2 transition-colors"
                  :class="passwordValidation.length ? 'text-status-success' : 'text-tertiary'"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      :d="passwordValidation.length ? 'M5 13l4 4L19 7' : 'M6 18L18 6M6 6l12 12'"
                    ></path>
                  </svg>
                  At least 8 characters
                </p>
              </div>
            </div>

            <!-- Confirm Password -->
            <div>
              <label for="confirm-password" class="block text-sm font-medium text-secondary mb-2">
                Confirm New Password
              </label>
              <div class="relative">
                <input
                  id="confirm-password"
                  v-model="confirmPassword"
                  :type="showConfirmPassword ? 'text' : 'password'"
                  required
                  autocomplete="new-password"
                  placeholder="Confirm new password"
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
                  <svg v-if="showConfirmPassword" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"></path>
                  </svg>
                  <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21"></path>
                  </svg>
                </button>
              </div>

              <!-- Password Match Indicator -->
              <p
                v-if="confirmPassword"
                class="mt-2 text-xs flex items-center gap-2 transition-colors"
                :class="passwordsMatch ? 'text-status-success' : 'text-status-error'"
              >
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    :d="passwordsMatch ? 'M5 13l4 4L19 7' : 'M6 18L18 6M6 6l12 12'"
                  ></path>
                </svg>
                {{ passwordsMatch ? 'Passwords match' : 'Passwords do not match' }}
              </p>
            </div>

            <!-- Submit Button -->
            <button
              type="submit"
              class="w-full px-6 py-3 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 mt-2"
              :disabled="loading || !isFormValid"
            >
              <svg
                v-if="loading"
                class="w-5 h-5 animate-spin"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <span>{{ loading ? 'Resetting Password...' : 'Reset Password' }}</span>
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
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
        </svg>
        Back to Login
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import authService from '@/services/authService';
import LogoIcon from '@/components/icons/LogoIcon.vue';

const router = useRouter();
const route = useRoute();

const newPassword = ref('');
const confirmPassword = ref('');
const showPassword = ref(false);
const showConfirmPassword = ref(false);
const loading = ref(false);
const resetSuccess = ref(false);
const errorMessage = ref('');

const token = ref('');

// Password validation
const passwordValidation = computed(() => ({
  length: newPassword.value.length >= 8,
}));

const passwordsMatch = computed(() => {
  return confirmPassword.value && newPassword.value === confirmPassword.value;
});

const isFormValid = computed(() => {
  return passwordValidation.value.length && passwordsMatch.value;
});

const validatePassword = () => {
  if (confirmPassword.value) {
    validatePasswordMatch();
  }
};

const validatePasswordMatch = () => {
  // Validation is handled by computed property
};

// Get token from URL query params
onMounted(() => {
  token.value = (route.query.token as string) || '';

  if (!token.value) {
    errorMessage.value = 'Invalid or missing reset token. Please request a new password reset.';
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
    errorMessage.value = axiosError.response?.data?.message || 'Failed to reset password. The link may have expired.';
  } finally {
    loading.value = false;
  }
};

const goToLogin = () => {
  router.push('/login');
};
</script>
