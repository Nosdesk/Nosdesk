<script setup lang="ts">
import { ref, computed } from 'vue';
import { useAuthStore } from '@/stores/auth';
import authService from '@/services/authService';
import userService from '@/services/userService';
import Spinner from '@/components/common/Spinner.vue';

const props = defineProps<{
  targetUserUuid?: string;
}>();

// Get current user info
const authStore = useAuthStore();

const isManagingOtherUser = computed(() => {
  return !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid;
});

// Form state
const currentPassword = ref('');
const newPassword = ref('');
const confirmPassword = ref('');
const loading = ref(false);

// Admin reset state
const adminNewPassword = ref('');
const adminConfirmPassword = ref('');

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

const adminPasswordsMatch = computed(() => {
  return adminNewPassword.value === adminConfirmPassword.value;
});

const isAdminFormValid = computed(() => {
  return adminNewPassword.value.length >= 8 && adminPasswordsMatch.value;
});

// Password change function (self)
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

// Admin password reset
const adminResetPassword = async () => {
  if (!isAdminFormValid.value || !props.targetUserUuid) return;

  loading.value = true;

  try {
    await userService.adminResetUserPassword(props.targetUserUuid, adminNewPassword.value);

    adminNewPassword.value = '';
    adminConfirmPassword.value = '';

    emit('success', 'Password has been reset for this user');
  } catch (err) {
    const axiosError = err as { response?: { data?: { message?: string } } };
    const errorMessage = axiosError.response?.data?.message || 'Failed to reset password';
    emit('error', errorMessage);
  } finally {
    loading.value = false;
  }
};
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <h2 class="text-lg font-medium text-primary">Password</h2>
      <p class="text-sm text-tertiary mt-1">
        {{ isManagingOtherUser ? "Reset this user's password" : "Update your account password" }}
      </p>
    </div>

    <div class="p-6">
      <!-- Admin: reset password form -->
      <form v-if="isManagingOtherUser" @submit.prevent="adminResetPassword" class="flex flex-col gap-4">
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide">New Password</label>
          <div class="bg-surface-alt rounded-lg border border-subtle">
            <input
              v-model="adminNewPassword"
              type="password"
              autocomplete="new-password"
              class="w-full px-4 py-2 bg-transparent text-primary rounded-lg focus:ring-2 focus:ring-accent focus:outline-none"
              placeholder="Enter new password"
              minlength="8"
              required
            />
          </div>
          <p class="text-xs text-tertiary">Password must be at least 8 characters long</p>
        </div>

        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-tertiary uppercase tracking-wide">Confirm New Password</label>
          <div class="bg-surface-alt rounded-lg border border-subtle">
            <input
              v-model="adminConfirmPassword"
              type="password"
              autocomplete="new-password"
              class="w-full px-4 py-2 bg-transparent text-primary rounded-lg focus:ring-2 focus:ring-accent focus:outline-none"
              placeholder="Confirm new password"
              required
            />
          </div>
          <p v-if="adminConfirmPassword && !adminPasswordsMatch" class="text-xs text-status-error">
            Passwords do not match
          </p>
        </div>

        <div class="pt-2">
          <button
            type="submit"
            :disabled="!isAdminFormValid || loading"
            class="px-6 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed flex items-center"
          >
            <span v-if="loading" class="mr-2 inline-flex">
              <Spinner />
            </span>
            Reset Password
          </button>
        </div>
      </form>

      <!-- Self: password change form -->
      <form v-else @submit.prevent="changePassword" class="flex flex-col gap-4">
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
            <span v-if="loading" class="mr-2 inline-flex">
              <Spinner />
            </span>
            Change Password
          </button>
        </div>
      </form>
    </div>
  </div>
</template>