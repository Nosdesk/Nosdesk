<script setup lang="ts">
import { ref, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import authService from '@nosdesk/core/services/authService';
import userService from '@/services/userService';
import { extractErrorMessage } from '@/utils/errors';
import SectionCard from '@/components/common/SectionCard.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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
    emit('error', t('settings-security-error-form-invalid'));
    return;
  }

  loading.value = true;

  try {
    await authService.changePassword(currentPassword.value, newPassword.value);

    // Reset form on success
    currentPassword.value = '';
    newPassword.value = '';
    confirmPassword.value = '';

    emit('success', t('settings-security-success-changed'));
  } catch (err) {
    const errorMessage = extractErrorMessage(err, t('settings-security-error-change-failed'));
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

    emit('success', t('settings-security-success-reset'));
  } catch (err) {
    const errorMessage = extractErrorMessage(err, t('settings-security-error-reset-failed'));
    emit('error', errorMessage);
  } finally {
    loading.value = false;
  }
};
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ t('settings-security-title') }}</template>

    <div>
      <!-- Admin: reset password form -->
      <form v-if="isManagingOtherUser" @submit.prevent="adminResetPassword" class="flex flex-col gap-4">
        <FormInput
          v-model="adminNewPassword"
          type="password"
          autocomplete="new-password"
          :label="t('settings-security-label-new')"
          :placeholder="t('settings-security-placeholder-admin-new')"
          :description="t('settings-security-hint-length')"
          required
        />

        <FormInput
          v-model="adminConfirmPassword"
          type="password"
          autocomplete="new-password"
          :label="t('settings-security-label-confirm')"
          :placeholder="t('settings-security-placeholder-admin-confirm')"
          :error="adminConfirmPassword && !adminPasswordsMatch ? t('settings-security-error-mismatch') : undefined"
          required
        />

        <div class="pt-2">
          <Button type="submit" :disabled="!isAdminFormValid" :loading="loading">
            {{ t('settings-security-submit-reset') }}
          </Button>
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

        <FormInput
          v-model="currentPassword"
          type="password"
          autocomplete="current-password"
          :label="t('settings-security-label-current')"
          :placeholder="t('settings-security-placeholder-current')"
          required
        />

        <FormInput
          v-model="newPassword"
          type="password"
          autocomplete="new-password"
          :label="t('settings-security-label-new')"
          :placeholder="t('settings-security-placeholder-new')"
          :description="t('settings-security-hint-length')"
          required
        />

        <FormInput
          v-model="confirmPassword"
          type="password"
          autocomplete="new-password"
          :label="t('settings-security-label-confirm')"
          :placeholder="t('settings-security-placeholder-confirm')"
          :error="confirmPassword && !passwordsMatch ? t('settings-security-error-mismatch') : undefined"
          required
        />

        <!-- Submit Button -->
        <div class="pt-4">
          <Button type="submit" :disabled="!isFormValid" :loading="loading">
            {{ t('settings-security-submit-change') }}
          </Button>
        </div>
      </form>
    </div>
  </SectionCard>
</template>