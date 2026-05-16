import { ref, computed, onMounted } from 'vue';
import passkeyService, { type PasskeyInfo, type PasskeyLoginResult } from '@/services/passkeyService';
import { translate } from '@/i18n';
import { logger } from '@/utils/logger';

/**
 * Composable for passkey functionality following Vue 3 best practices
 *
 * Architecture: Component → Composable → Service → API Client
 *
 * This composable provides:
 * - Reactive state management for passkey operations
 * - Methods that wrap passkeyService calls
 * - Proper error handling and loading states
 * - Browser support detection
 */
export function usePasskeys() {
  // Reactive state
  const loading = ref(false);
  const registering = ref(false);
  const authenticating = ref(false);
  const passkeys = ref<PasskeyInfo[]>([]);
  const error = ref<string | null>(null);
  const successMessage = ref<string | null>(null);

  // Browser support - check synchronously on creation to avoid flash of "not supported" message
  const isSupported = ref(passkeyService.isSupported());
  const isConditionalUISupported = ref(false);

  // Computed properties
  const hasPasskeys = computed(() => passkeys.value.length > 0);
  const passkeyCount = computed(() => passkeys.value.length);
  const canAddPasskey = computed(() => passkeys.value.length < 10); // Max 10 passkeys

  /**
   * Check browser support on initialization
   */
  async function checkSupport(): Promise<void> {
    isSupported.value = passkeyService.isSupported();
    if (isSupported.value) {
      isConditionalUISupported.value = await passkeyService.isConditionalUISupported();
    }
    logger.debug('Passkey support check', {
      supported: isSupported.value,
      conditionalUI: isConditionalUISupported.value,
    });
  }

  /**
   * Load user's passkeys
   */
  async function loadPasskeys(): Promise<void> {
    if (!isSupported.value) return;

    try {
      loading.value = true;
      error.value = null;
      passkeys.value = await passkeyService.listPasskeys();
    } catch (err) {
      logger.error('Failed to load passkeys', { error: err });
      error.value = translate('auth-passkey-load-failed', undefined, 'Failed to load passkeys');
    } finally {
      loading.value = false;
    }
  }

  /**
   * Register a new passkey
   */
  async function registerPasskey(name?: string): Promise<boolean> {
    if (!isSupported.value) {
      error.value = translate('auth-passkey-not-supported-browser', undefined, 'Passkeys are not supported in this browser');
      return false;
    }

    if (!canAddPasskey.value) {
      error.value = translate('auth-passkey-max-reached', undefined, 'Maximum number of passkeys reached (10)');
      return false;
    }

    try {
      registering.value = true;
      error.value = null;
      successMessage.value = null;

      const result = await passkeyService.registerPasskey(name);

      if (result.success) {
        successMessage.value = translate(
          'auth-passkey-registered',
          { name: result.passkey.name },
          `Passkey "${result.passkey.name}" registered successfully`,
        );
        // Reload passkeys to get updated list
        await loadPasskeys();
        return true;
      }

      return false;
    } catch (err: unknown) {
      logger.error('Failed to register passkey', { error: err });

      // Handle specific WebAuthn errors
      if (err instanceof Error) {
        if (err.name === 'NotAllowedError') {
          error.value = translate('auth-passkey-registration-not-allowed', undefined, 'Registration was cancelled or not allowed');
        } else if (err.name === 'NotSupportedError') {
          error.value = translate('auth-passkey-not-supported-device', undefined, 'Passkeys are not supported on this device');
        } else if (err.name === 'InvalidStateError') {
          error.value = translate('auth-passkey-already-registered', undefined, 'This passkey is already registered');
        } else if (err.message.includes('cancelled')) {
          error.value = translate('auth-passkey-registration-cancelled', undefined, 'Registration was cancelled');
        } else if (err.message.includes('Not implemented')) {
          error.value = translate('auth-passkey-not-supported-device', undefined, 'Passkeys are not supported on this device');
        } else {
          error.value = err.message || translate('auth-passkey-register-failed', undefined, 'Failed to register passkey');
        }
      } else {
        error.value = translate('auth-passkey-register-failed', undefined, 'Failed to register passkey');
      }

      return false;
    } finally {
      registering.value = false;
    }
  }

  /**
   * Login with passkey
   * - If email is provided: Uses email-based lookup (works with all passkeys)
   * - If no email: Uses discoverable auth (requires resident key passkeys)
   */
  async function loginWithPasskey(email?: string): Promise<PasskeyLoginResult | null> {
    if (!isSupported.value) {
      error.value = translate('auth-passkey-not-supported-browser', undefined, 'Passkeys are not supported in this browser');
      return null;
    }

    try {
      authenticating.value = true;
      error.value = null;
      successMessage.value = null;

      const result = await passkeyService.loginWithPasskey(email);

      if (result.success) {
        successMessage.value = translate('auth-passkey-login-success', undefined, 'Logged in successfully with passkey');
        return result;
      }

      return null;
    } catch (err: unknown) {
      logger.error('Failed to login with passkey', { error: err });

      // Handle specific WebAuthn errors
      if (err instanceof Error) {
        if (err.name === 'NotAllowedError') {
          error.value = translate('auth-passkey-auth-not-allowed', undefined, 'Authentication was cancelled or not allowed');
        } else if (err.name === 'NotSupportedError') {
          error.value = translate('auth-passkey-not-supported-device', undefined, 'Passkeys are not supported on this device');
        } else if (err.message.includes('No passkeys registered')) {
          error.value = translate('auth-passkey-none-registered', undefined, 'No passkeys registered for this account');
        } else if (err.message.includes('cancelled')) {
          error.value = translate('auth-passkey-auth-cancelled', undefined, 'Authentication was cancelled');
        } else if (err.message.includes('Not implemented')) {
          error.value = translate('auth-passkey-not-supported-device', undefined, 'Passkeys are not supported on this device');
        } else {
          error.value = err.message || translate('auth-passkey-login-failed', undefined, 'Failed to login with passkey');
        }
      } else {
        error.value = translate('auth-passkey-login-failed', undefined, 'Failed to login with passkey');
      }

      return null;
    } finally {
      authenticating.value = false;
    }
  }

  /**
   * Rename a passkey
   */
  async function renamePasskey(credentialId: string, name: string): Promise<boolean> {
    if (!name || !name.trim()) {
      error.value = translate('auth-passkey-name-required', undefined, 'Passkey name is required');
      return false;
    }

    if (name.trim().length > 100) {
      error.value = translate('auth-passkey-name-too-long', undefined, 'Passkey name must be 100 characters or less');
      return false;
    }

    try {
      loading.value = true;
      error.value = null;
      successMessage.value = null;

      const success = await passkeyService.renamePasskey(credentialId, name.trim());

      if (success) {
        successMessage.value = translate('auth-passkey-renamed', undefined, 'Passkey renamed successfully');
        // Update local state
        const passkey = passkeys.value.find(p => p.id === credentialId);
        if (passkey) {
          passkey.name = name.trim();
        }
        return true;
      }

      return false;
    } catch (err) {
      logger.error('Failed to rename passkey', { error: err });
      error.value = translate('auth-passkey-rename-failed', undefined, 'Failed to rename passkey');
      return false;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Delete a passkey
   */
  async function deletePasskey(credentialId: string, password: string): Promise<boolean> {
    if (!password) {
      error.value = translate('auth-passkey-delete-password-required', undefined, 'Password is required to delete a passkey');
      return false;
    }

    try {
      loading.value = true;
      error.value = null;
      successMessage.value = null;

      const success = await passkeyService.deletePasskey(credentialId, password);

      if (success) {
        successMessage.value = translate('auth-passkey-deleted', undefined, 'Passkey deleted successfully');
        // Remove from local state
        passkeys.value = passkeys.value.filter(p => p.id !== credentialId);
        return true;
      }

      return false;
    } catch (err: unknown) {
      logger.error('Failed to delete passkey', { error: err });

      if (err instanceof Error && err.message.includes('Incorrect password')) {
        error.value = translate('auth-passkey-incorrect-password', undefined, 'Incorrect password');
      } else {
        error.value = translate('auth-passkey-delete-failed', undefined, 'Failed to delete passkey');
      }

      return false;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Clear messages
   */
  function clearMessages(): void {
    error.value = null;
    successMessage.value = null;
  }

  /**
   * Format date for display
   */
  function formatDate(dateString: string | null): string {
    if (!dateString) return translate('passkey-last-used-never', undefined, 'Never');
    try {
      return new Date(dateString).toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      });
    } catch {
      return dateString;
    }
  }

  // Initialize support check
  onMounted(() => {
    checkSupport();
  });

  return {
    // State
    loading,
    registering,
    authenticating,
    passkeys,
    error,
    successMessage,
    isSupported,
    isConditionalUISupported,

    // Computed
    hasPasskeys,
    passkeyCount,
    canAddPasskey,

    // Methods
    checkSupport,
    loadPasskeys,
    registerPasskey,
    loginWithPasskey,
    renamePasskey,
    deletePasskey,
    clearMessages,
    formatDate,
  };
}
