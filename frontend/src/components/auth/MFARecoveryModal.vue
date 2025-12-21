<template>
  <teleport to="body">
    <transition name="modal">
      <div
        v-if="isOpen"
        class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm"
        @click.self="close"
      >
        <div
          class="relative bg-surface rounded-xl border border-default shadow-2xl max-w-md w-full overflow-hidden z-10"
          @click.stop
        >
          <!-- Header -->
          <div class="px-6 py-4 bg-surface-alt border-b border-subtle flex items-center justify-between">
            <h2 class="text-lg font-semibold text-primary">MFA Account Recovery</h2>
            <button
              @click="close"
              class="text-tertiary hover:text-primary transition-colors p-1 rounded-lg hover:bg-surface-hover"
              aria-label="Close modal"
            >
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
              </svg>
            </button>
          </div>

          <!-- Content -->
          <div class="p-6">
            <!-- Success State -->
            <div v-if="emailSent" class="flex flex-col items-center gap-4 text-center">
              <div class="bg-status-success/20 rounded-full p-3">
                <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"></path>
                </svg>
              </div>
              <div>
                <h3 class="text-lg font-medium text-primary mb-2">Check Your Email</h3>
                <p class="text-sm text-secondary">
                  If an account with that email exists and has MFA enabled, we've sent a recovery link to
                  <span class="text-primary font-medium">{{ email }}</span>
                </p>
              </div>
              <div class="bg-status-warning/10 border border-status-warning/20 rounded-lg p-4 text-sm text-secondary">
                <p class="mb-2"><strong class="text-status-warning">Important:</strong></p>
                <ul class="space-y-1 text-xs">
                  <li>• The recovery link will expire in <strong>15 minutes</strong></li>
                  <li>• You'll be able to disable or reconfigure MFA</li>
                  <li>• Check your spam folder if you don't see it</li>
                  <li>• You can close this window now</li>
                </ul>
              </div>
              <button
                @click="close"
                class="w-full px-4 py-2 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium"
              >
                Done
              </button>
            </div>

            <!-- Form State -->
            <form v-else @submit.prevent="handleSubmit" class="flex flex-col gap-4">
              <p class="text-sm text-secondary">
                To verify your identity, please enter your email and password. We'll send you a secure recovery link to manage your MFA settings.
              </p>

              <!-- Error Message -->
              <div
                v-if="errorMessage"
                class="bg-status-error/50 border border-status-error text-status-error px-4 py-3 rounded-lg text-sm"
              >
                {{ errorMessage }}
              </div>

              <!-- Email Input -->
              <div>
                <label for="mfa-recovery-email" class="block text-sm font-medium text-secondary mb-2">
                  Email Address
                </label>
                <input
                  id="mfa-recovery-email"
                  v-model="email"
                  type="email"
                  required
                  autocomplete="email"
                  placeholder="you@example.com"
                  class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                  :disabled="loading"
                />
              </div>

              <!-- Password Input -->
              <div>
                <label for="mfa-recovery-password" class="block text-sm font-medium text-secondary mb-2">
                  Password
                </label>
                <input
                  id="mfa-recovery-password"
                  v-model="password"
                  type="password"
                  required
                  autocomplete="current-password"
                  placeholder="Enter your password"
                  class="w-full px-4 py-3 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent transition-colors"
                  :disabled="loading"
                />
              </div>

              <!-- Security Notice -->
              <div class="bg-accent/10 border border-accent/20 rounded-lg p-3 text-xs text-secondary">
                <p class="flex items-start gap-2">
                  <svg class="w-4 h-4 text-accent flex-shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                  </svg>
                  <span>This will send a secure recovery link to your email. The link can only be used once and expires in 15 minutes.</span>
                </p>
              </div>

              <!-- Action Buttons -->
              <div class="flex gap-3 pt-2">
                <button
                  type="button"
                  @click="close"
                  class="flex-1 px-4 py-2 bg-surface-alt hover:bg-surface-hover text-primary rounded-lg transition-colors font-medium"
                  :disabled="loading"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  class="flex-1 px-4 py-2 bg-accent hover:opacity-90 text-white rounded-lg transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                  :disabled="loading || !email || !password"
                >
                  <svg
                    v-if="loading"
                    class="w-4 h-4 animate-spin"
                    fill="none"
                    viewBox="0 0 24 24"
                  >
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  <span>{{ loading ? 'Sending...' : 'Send Recovery Link' }}</span>
                </button>
              </div>
            </form>
          </div>
        </div>
      </div>
    </transition>
  </teleport>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import authService from '@/services/authService';

const props = defineProps<{
  isOpen: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const email = ref('');
const password = ref('');
const loading = ref(false);
const emailSent = ref(false);
const errorMessage = ref('');

// Reset state when modal opens
watch(() => props.isOpen, (newValue) => {
  if (newValue) {
    email.value = '';
    password.value = '';
    emailSent.value = false;
    errorMessage.value = '';
    loading.value = false;
  }
});

const handleSubmit = async () => {
  errorMessage.value = '';
  loading.value = true;

  try {
    await authService.requestMFAReset(email.value, password.value);
    emailSent.value = true;
  } catch (error: any) {
    console.error('MFA recovery request error:', error);
    // Show generic error to prevent account enumeration
    errorMessage.value = error.response?.data?.message || 'Failed to send recovery email. Please try again.';
  } finally {
    loading.value = false;
  }
};

const close = () => {
  if (!loading.value) {
    emit('close');
  }
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
