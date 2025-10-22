<template>
  <teleport to="body">
    <transition name="modal">
      <div
        v-if="isOpen"
        class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm"
        @click.self="close"
      >
        <div
          class="bg-slate-800 rounded-xl border border-slate-700 shadow-2xl max-w-md w-full overflow-hidden"
          @click.stop
        >
          <!-- Header -->
          <div class="px-6 py-4 bg-slate-700/30 border-b border-slate-700/50 flex items-center justify-between">
            <h2 class="text-lg font-semibold text-white">Reset Your Password</h2>
            <button
              @click="close"
              class="text-slate-400 hover:text-white transition-colors p-1 rounded-lg hover:bg-slate-700/50"
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
              <div class="bg-green-600/20 rounded-full p-3">
                <svg class="w-8 h-8 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"></path>
                </svg>
              </div>
              <div>
                <h3 class="text-lg font-medium text-white mb-2">Check Your Email</h3>
                <p class="text-sm text-slate-400">
                  If an account with that email exists, we've sent a password reset link to
                  <span class="text-white font-medium">{{ email }}</span>
                </p>
              </div>
              <div class="bg-blue-600/10 border border-blue-600/20 rounded-lg p-4 text-sm text-slate-300">
                <p class="mb-2"><strong class="text-blue-400">Important:</strong></p>
                <ul class="space-y-1 text-xs">
                  <li>• The link will expire in <strong>1 hour</strong></li>
                  <li>• Check your spam folder if you don't see it</li>
                  <li>• You can close this window now</li>
                </ul>
              </div>
              <button
                @click="close"
                class="w-full px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors font-medium"
              >
                Done
              </button>
            </div>

            <!-- Form State -->
            <form v-else @submit.prevent="handleSubmit" class="flex flex-col gap-4">
              <p class="text-sm text-slate-400">
                Enter your email address and we'll send you a link to reset your password.
              </p>

              <!-- Error Message -->
              <div
                v-if="errorMessage"
                class="bg-red-900/50 border border-red-700 text-red-200 px-4 py-3 rounded-lg text-sm"
              >
                {{ errorMessage }}
              </div>

              <!-- Email Input -->
              <div>
                <label for="reset-email" class="block text-sm font-medium text-slate-300 mb-2">
                  Email Address
                </label>
                <input
                  id="reset-email"
                  v-model="email"
                  type="email"
                  required
                  autocomplete="email"
                  placeholder="you@example.com"
                  class="w-full px-4 py-3 bg-slate-700/50 border border-slate-600 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition-colors"
                  :disabled="loading"
                />
              </div>

              <!-- Action Buttons -->
              <div class="flex gap-3 pt-2">
                <button
                  type="button"
                  @click="close"
                  class="flex-1 px-4 py-2 bg-slate-700 hover:bg-slate-600 text-white rounded-lg transition-colors font-medium"
                  :disabled="loading"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  class="flex-1 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                  :disabled="loading || !email"
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
                  <span>{{ loading ? 'Sending...' : 'Send Reset Link' }}</span>
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

const props = defineProps<{
  isOpen: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const email = ref('');
const loading = ref(false);
const emailSent = ref(false);
const errorMessage = ref('');

// Reset state when modal opens
watch(() => props.isOpen, (newValue) => {
  if (newValue) {
    email.value = '';
    emailSent.value = false;
    errorMessage.value = '';
    loading.value = false;
  }
});

const handleSubmit = async () => {
  errorMessage.value = '';
  loading.value = true;

  try {
    const response = await fetch('/api/auth/password-reset/request', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ email: email.value }),
    });

    const data = await response.json();

    if (response.ok) {
      emailSent.value = true;
    } else {
      // Show generic error to prevent account enumeration
      errorMessage.value = data.message || 'Failed to send reset email. Please try again.';
    }
  } catch (error) {
    console.error('Password reset request error:', error);
    errorMessage.value = 'Network error. Please check your connection and try again.';
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
