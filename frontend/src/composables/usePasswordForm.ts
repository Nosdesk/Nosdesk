import { ref, computed, onScopeDispose } from 'vue';

/**
 * Composable for password + confirm password form logic.
 * Handles state, visibility toggles, validation, and sensitive data cleanup.
 */
export function usePasswordForm() {
  const newPassword = ref('');
  const confirmPassword = ref('');
  const showPassword = ref(false);
  const showConfirmPassword = ref(false);

  const passwordValidation = computed(() => ({
    length: newPassword.value.length >= 8,
  }));

  const passwordsMatch = computed(() => {
    return !!confirmPassword.value && newPassword.value === confirmPassword.value;
  });

  const isFormValid = computed(() => {
    return passwordValidation.value.length && passwordsMatch.value;
  });

  const validatePassword = () => {
    // Triggers reactivity; match indicator updates via computed
  };

  const validatePasswordMatch = () => {
    // Triggers reactivity; match indicator updates via computed
  };

  const clearSensitiveData = () => {
    newPassword.value = '';
    confirmPassword.value = '';
  };

  // Auto-cleanup on scope disposal
  onScopeDispose(clearSensitiveData);

  return {
    newPassword,
    confirmPassword,
    showPassword,
    showConfirmPassword,
    passwordValidation,
    passwordsMatch,
    isFormValid,
    validatePassword,
    validatePasswordMatch,
    clearSensitiveData,
  };
}
