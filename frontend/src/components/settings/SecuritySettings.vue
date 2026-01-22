<script setup lang="ts">
import { ref, computed } from 'vue';
import { useAuthStore } from '@/stores/auth';
import authService from '@/services/authService';

// Get current user info
const authStore = useAuthStore();

// Form state
const currentPassword = ref('');
const newPassword = ref('');
const confirmPassword = ref('');
const loading = ref(false);

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Validation
const passwordsMatch = computed(() => {
  return newPassword.value === confirmPassword.value;
});

const isFormValid = computed(() => {
  return currentPassword.value.length > 0 &&
         newPassword.value.length >= 8 &&
         passwordsMatch.value;
});

// Password change function
const changePassword = async () => {
  if (!isFormValid.value) {
    emit('error', 'Please fill in all fields correctly');
    return;
  }

  loading.value = true;

  try {
    await authService.changePassword(currentPassword.value, newPassword.value);

    // Reset form on success
    currentPassword.value = '';
    newPassword.value = '';
    confirmPassword.value = '';

    emit('success', 'Password changed successfully');
  } catch (err) {
    const axiosError = err as { response?: { data?: { message?: string } } };
    const errorMessage = axiosError.response?.data?.message || 'Failed to change password. Please check your current password.';
    emit('error', errorMessage);
    console.error('Error changing password:', err);
  } finally {
    loading.value = false;
  }
};
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <h2 class="text-lg font-medium text-primary">Password</h2>
      <p class="text-sm text-tertiary mt-1">Update your account password</p>
    </div>
    
    <div class="p-6">
      <form @submit.prevent="changePassword" class="flex flex-col gap-4">
        <!-- Hidden username field for accessibility and password managers -->
        <input
          type="email"
          :value="authStore.user?.email || ''"
          autocomplete="username"
          class="sr-only"
          tabindex="-1"
          readonly
        />
        
        <!-- Current Password -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide">Current Password</label>
          <div class="bg-surface-alt rounded-lg border border-subtle">
            <input
              v-model="currentPassword"
              type="password"
              autocomplete="current-password"
              class="w-full px-4 py-2 bg-transparent text-primary rounded-lg focus:ring-2 focus:ring-accent focus:outline-none"
              placeholder="Enter your current password"
              required
            />
          </div>
        </div>

        <!-- New Password -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide">New Password</label>
          <div class="bg-surface-alt rounded-lg border border-subtle">
            <input
              v-model="newPassword"
              type="password"
              autocomplete="new-password"
              class="w-full px-4 py-2 bg-transparent text-primary rounded-lg focus:ring-2 focus:ring-accent focus:outline-none"
              placeholder="Enter your new password"
              minlength="8"
              required
            />
          </div>
          <p class="text-xs text-tertiary">Password must be at least 8 characters long</p>
        </div>

        <!-- Confirm New Password -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide">Confirm New Password</label>
          <div class="bg-surface-alt rounded-lg border border-subtle">
            <input
              v-model="confirmPassword"
              type="password"
              autocomplete="new-password"
              class="w-full px-4 py-2 bg-transparent text-primary rounded-lg focus:ring-2 focus:ring-accent focus:outline-none"
              placeholder="Confirm your new password"
              required
            />
          </div>
          <p v-if="confirmPassword && !passwordsMatch" class="text-xs text-status-error">
            Passwords do not match
          </p>
        </div>

        <!-- Submit Button -->
        <div class="pt-4">
          <button
            type="submit"
            :disabled="!isFormValid || loading"
            class="px-6 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed flex items-center"
          >
            <span v-if="loading" class="animate-spin h-4 w-4 mr-2">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 004 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
            </span>
            Change Password
          </button>
        </div>
      </form>
    </div>
  </div>
</template> 