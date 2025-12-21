<template>
  <div class="min-h-screen flex items-center justify-center bg-app p-4">
    <div class="max-w-md w-full">
      <!-- Loading State -->
      <div v-if="loading" class="bg-surface rounded-xl border border-default shadow-2xl p-8">
        <div class="flex flex-col items-center gap-4">
          <svg
            class="w-12 h-12 animate-spin text-accent"
            fill="none"
            viewBox="0 0 24 24"
          >
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <p class="text-secondary">Validating recovery link...</p>
        </div>
      </div>

      <!-- Success State - MFA Management -->
      <div v-else-if="recoveryToken" class="bg-surface rounded-xl border border-default shadow-2xl overflow-hidden">
        <!-- Header -->
        <div class="px-6 py-4 bg-surface-alt border-b border-subtle">
          <h1 class="text-xl font-semibold text-primary">MFA Account Recovery</h1>
          <p class="text-sm text-secondary mt-1">Manage your multi-factor authentication settings</p>
        </div>

        <!-- Content -->
        <div class="p-6 space-y-6">
          <!-- Session Timer Warning -->
          <div class="bg-status-warning/10 border border-status-warning/20 rounded-lg p-4">
            <div class="flex items-start gap-3">
              <svg class="w-5 h-5 text-status-warning flex-shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"></path>
              </svg>
              <div class="text-sm text-secondary">
                <p class="font-medium text-status-warning mb-1">Limited Recovery Session</p>
                <p>This session will expire in {{ formatTime(timeRemaining) }}. You can only manage your MFA settings during this session.</p>
              </div>
            </div>
          </div>

          <!-- User Info -->
          <div v-if="user" class="bg-surface-alt rounded-lg p-4">
            <div class="flex items-center gap-3">
              <div class="w-12 h-12 rounded-full bg-accent flex items-center justify-center text-white font-medium text-lg">
                {{ user.name.charAt(0).toUpperCase() }}
              </div>
              <div>
                <p class="text-primary font-medium">{{ user.name }}</p>
                <p class="text-sm text-secondary">{{ user.email }}</p>
              </div>
            </div>
          </div>

          <!-- MFA Actions -->
          <div class="space-y-3">
            <h2 class="text-sm font-medium text-secondary">Available Actions</h2>

            <!-- Disable MFA -->
            <button
              @click="showDisableConfirm = true"
              :disabled="disabling"
              class="w-full px-4 py-3 bg-status-error/10 hover:bg-status-error/20 border border-status-error/30 text-status-error rounded-lg transition-colors font-medium text-left flex items-center justify-between group disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <div class="flex items-center gap-3">
                <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"></path>
                </svg>
                <div>
                  <p class="font-medium">Disable MFA</p>
                  <p class="text-xs text-tertiary">Remove two-factor authentication from your account</p>
                </div>
              </div>
              <svg class="w-5 h-5 group-hover:translate-x-1 transition-transform" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"></path>
              </svg>
            </button>

            <!-- Setup New MFA (Future Enhancement) -->
            <div class="w-full px-4 py-3 bg-surface-alt border border-default text-tertiary rounded-lg text-left flex items-center justify-between opacity-60 cursor-not-allowed">
              <div class="flex items-center gap-3">
                <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"></path>
                </svg>
                <div>
                  <p class="font-medium">Setup New Authenticator</p>
                  <p class="text-xs text-tertiary">Coming soon - disable MFA first, then set up a new one after logging in</p>
                </div>
              </div>
            </div>
          </div>

          <!-- Back to Login -->
          <div class="pt-4 border-t border-default">
            <button
              @click="backToLogin"
              class="w-full px-4 py-2 bg-surface-alt hover:bg-surface-hover text-primary rounded-lg transition-colors font-medium"
            >
              Back to Login
            </button>
          </div>
        </div>
      </div>

      <!-- Error State -->
      <div v-else-if="error" class="bg-surface rounded-xl border border-status-error/50 shadow-2xl p-8">
        <div class="flex flex-col items-center gap-4 text-center">
          <div class="bg-status-error/10 rounded-full p-3">
            <svg class="w-8 h-8 text-status-error" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
            </svg>
          </div>
          <div>
            <h2 class="text-lg font-medium text-primary mb-2">Recovery Link Invalid</h2>
            <p class="text-sm text-secondary">{{ error }}</p>
          </div>
          <button
            @click="backToLogin"
            class="px-6 py-2 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium"
          >
            Back to Login
          </button>
        </div>
      </div>
    </div>

    <!-- Disable MFA Confirmation Modal -->
    <teleport to="body">
      <transition name="modal">
        <div
          v-if="showDisableConfirm"
          class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm"
          @click.self="showDisableConfirm = false"
        >
          <div
            class="relative bg-surface rounded-xl border border-default shadow-2xl max-w-md w-full overflow-hidden z-10"
            @click.stop
          >
            <!-- Header -->
            <div class="px-6 py-4 bg-status-error/10 border-b border-status-error/50 flex items-center gap-3">
              <svg class="w-6 h-6 text-status-error" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path>
              </svg>
              <h2 class="text-lg font-semibold text-primary">Disable Multi-Factor Authentication</h2>
            </div>

            <!-- Content -->
            <div class="p-6 space-y-4">
              <p class="text-secondary">
                Are you sure you want to disable MFA? This will make your account less secure.
              </p>

              <div class="bg-status-warning/10 border border-status-warning/20 rounded-lg p-4">
                <p class="text-sm text-secondary">
                  <strong class="text-status-warning">Important:</strong> After disabling MFA, you can set up a new authenticator by logging in and going to your security settings.
                </p>
              </div>

              <!-- Error Message -->
              <div
                v-if="disableError"
                class="bg-status-error/20 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm"
              >
                {{ disableError }}
              </div>

              <!-- Action Buttons -->
              <div class="flex gap-3 pt-2">
                <button
                  type="button"
                  @click="showDisableConfirm = false"
                  class="flex-1 px-4 py-2 bg-surface-alt hover:bg-surface-hover text-primary rounded-lg transition-colors font-medium"
                  :disabled="disabling"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  @click="disableMFA"
                  class="flex-1 px-4 py-2 bg-status-error hover:opacity-90 text-white rounded-lg transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                  :disabled="disabling"
                >
                  <svg
                    v-if="disabling"
                    class="w-4 h-4 animate-spin"
                    fill="none"
                    viewBox="0 0 24 24"
                  >
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  <span>{{ disabling ? 'Disabling...' : 'Yes, Disable MFA' }}</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </transition>
    </teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import authService from '@/services/authService';

const route = useRoute();
const router = useRouter();

const loading = ref(true);
const error = ref('');
const recoveryToken = ref('');
const user = ref<any>(null);
const timeRemaining = ref(900); // 15 minutes in seconds
const showDisableConfirm = ref(false);
const disabling = ref(false);
const disableError = ref('');

let timerInterval: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  const token = route.query.token as string;

  if (!token) {
    error.value = 'No recovery token provided. Please request a new recovery link.';
    loading.value = false;
    return;
  }

  try {
    // Validate the token and get the limited-scope JWT
    const data = await authService.completeMFAReset(token);

    recoveryToken.value = data.token;
    user.value = data.user_uuid;

    // Start countdown timer
    startTimer();
  } catch (err: any) {
    console.error('MFA recovery error:', err);
    error.value = err.response?.data?.message || 'Invalid or expired recovery link. Please request a new one.';
  } finally {
    loading.value = false;
  }
});

onUnmounted(() => {
  if (timerInterval) {
    clearInterval(timerInterval);
  }
});

const startTimer = () => {
  timerInterval = setInterval(() => {
    timeRemaining.value--;

    if (timeRemaining.value <= 0) {
      if (timerInterval) {
        clearInterval(timerInterval);
      }
      error.value = 'Recovery session expired. Please request a new recovery link.';
      recoveryToken.value = '';
    }
  }, 1000);
};

const formatTime = (seconds: number): string => {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
};

const disableMFA = async () => {
  disableError.value = '';
  disabling.value = true;

  try {
    await authService.disableMFAWithToken(recoveryToken.value);

    // MFA disabled successfully
    showDisableConfirm.value = false;

    // Show success message and redirect to login
    alert('MFA has been disabled successfully. You can now log in with just your password.');
    router.push('/login');
  } catch (err: any) {
    console.error('MFA disable error:', err);
    disableError.value = err.response?.data?.message || 'Failed to disable MFA. Please try again.';
  } finally {
    disabling.value = false;
  }
};

const backToLogin = () => {
  router.push('/login');
};
</script>

<style scoped>
/* Modal transition animations */
.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.2s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-active > div,
.modal-leave-active > div {
  transition: transform 0.2s ease, opacity 0.2s ease;
}

.modal-enter-from > div,
.modal-leave-to > div {
  transform: scale(0.95);
  opacity: 0;
}
</style>
