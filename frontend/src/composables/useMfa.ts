import { ref, computed } from 'vue';
import authService, {
  MFASetupData,
  MFAStatusResponse,
  MFAVerifyRequest,
  MFAEnableRequest,
  MFALoginSetupRequest,
  MFALoginEnableRequest
} from '@/services/authService';

/**
 * Composable for MFA functionality following Vue 3 best practices
 *
 * Architecture: Component → Composable → Service → API Client
 *
 * This composable provides:
 * - Reactive state management for MFA operations
 * - Methods that wrap authService calls
 * - Proper error handling and loading states
 * - Support for both login setup and authenticated user flows
 */
export function useMfa(options?: { isLoginSetup?: boolean }) {
  // Reactive state
  const loading = ref(false);
  const verifying = ref(false);
  const mfaEnabled = ref(false);
  const mfaStep = ref<'setup' | 'verify' | 'enabled' | 'success'>('setup');
  const qrCodeUrl = ref('');
  const mfaSecret = ref('');
  const backupCodes = ref<string[]>([]);
  const error = ref<string | null>(null);
  const successMessage = ref<string | null>(null);

  // Computed properties
  const isLoginSetup = computed(() => options?.isLoginSetup || false);
  const showSetupSteps = computed(() => !mfaEnabled.value && mfaStep.value === 'setup');
  const showVerificationStep = computed(() => !mfaEnabled.value && mfaStep.value === 'verify');
  const showEnabledState = computed(() => mfaEnabled.value && !isLoginSetup.value);
  const showSuccessState = computed(() => mfaStep.value === 'success');

  /**
   * Check current MFA status for authenticated user
   */
  async function checkMFAStatus(): Promise<void> {
    try {
      loading.value = true;
      error.value = null;

      const status: MFAStatusResponse = await authService.getMFAStatus();
      mfaEnabled.value = status.enabled || false;

      if (mfaEnabled.value) {
        mfaStep.value = 'enabled';
      }
    } catch (err) {
      console.error('Error checking MFA status:', err);
      error.value = 'Failed to check MFA status';
    } finally {
      loading.value = false;
    }
  }

  /**
   * Setup MFA for login (unauthenticated flow)
   */
  async function setupMFAForLogin(email: string, password: string): Promise<MFASetupData | null> {
    try {
      loading.value = true;
      error.value = null;

      const request: MFALoginSetupRequest = { email, password };
      const data = await authService.setupMFAForLogin(request);

      qrCodeUrl.value = data.qr_code;
      mfaSecret.value = data.secret;
      backupCodes.value = data.backup_codes || [];
      mfaStep.value = 'verify';

      successMessage.value = 'MFA setup initiated. Please scan the QR code with your authenticator app.';

      return data;
    } catch (err) {
      console.error('Error setting up MFA for login:', err);
      error.value = 'Failed to setup MFA';
      return null;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Start MFA setup for authenticated user
   */
  async function startMFASetup(): Promise<void> {
    try {
      loading.value = true;
      error.value = null;
      mfaStep.value = 'verify';

      const data = await authService.setupMFA();

      qrCodeUrl.value = data.qr_code;
      mfaSecret.value = data.secret;
      backupCodes.value = [];

      successMessage.value = 'MFA setup initiated. Please scan the QR code with your authenticator app.';
    } catch (err) {
      console.error('Error setting up MFA:', err);
      error.value = 'Failed to setup MFA';
      resetMFASetup();
    } finally {
      loading.value = false;
    }
  }

  /**
   * Verify MFA token during setup
   */
  async function verifyMFAToken(token: string): Promise<boolean> {
    if (token.length !== 6) {
      error.value = 'Please enter a valid 6-digit code';
      return false;
    }

    if (!mfaSecret.value) {
      error.value = 'MFA secret is missing. Please restart the setup process.';
      return false;
    }

    try {
      verifying.value = true;
      error.value = null;
      successMessage.value = null;

      const request: MFAVerifyRequest = {
        token,
        secret: mfaSecret.value
      };

      const result = await authService.verifyMFA(request);
      return result.valid;
    } catch (err) {
      console.error('Error verifying MFA:', err);
      error.value = err instanceof Error ? err.message : 'Invalid verification code. Please try again.';
      return false;
    } finally {
      verifying.value = false;
    }
  }

  /**
   * Enable MFA for authenticated user
   */
  async function enableMFA(token: string, password?: string): Promise<{ success: boolean; backup_codes?: string[] }> {
    try {
      loading.value = true;
      error.value = null;

      const request: MFAEnableRequest = {
        token,
        secret: mfaSecret.value,
        password
      };

      const result = await authService.enableMFA(request);

      if (result.success) {
        backupCodes.value = result.backup_codes || [];
        mfaEnabled.value = true;
        mfaStep.value = 'enabled';
        successMessage.value = 'MFA enabled successfully! Your account is now more secure.';
      }

      return { success: result.success, backup_codes: result.backup_codes };
    } catch (err) {
      console.error('Error enabling MFA:', err);
      error.value = err instanceof Error ? err.message : 'Failed to enable MFA';
      throw err;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Enable MFA during login flow
   */
  async function enableMFAForLogin(
    email: string,
    password: string,
    token: string
  ): Promise<any> {
    try {
      loading.value = true;
      error.value = null;

      const request: MFALoginEnableRequest = {
        email,
        password,
        token,
        secret: mfaSecret.value,
        backup_codes: backupCodes.value
      };

      const result = await authService.enableMFAForLogin(request);

      if (result.success) {
        mfaEnabled.value = true;
        mfaStep.value = 'success';
        successMessage.value = 'MFA enabled successfully!';
      }

      return result;
    } catch (err) {
      console.error('Error enabling MFA for login:', err);
      error.value = err instanceof Error ? err.message : 'Failed to enable MFA';
      throw err;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Disable MFA
   */
  async function disableMFA(password: string): Promise<boolean> {
    try {
      loading.value = true;
      error.value = null;

      const result = await authService.disableMFA(password);

      if (result.success) {
        mfaEnabled.value = false;
        mfaStep.value = 'setup';
        resetMFASetup();
        successMessage.value = 'MFA has been disabled';
      }

      return result.success;
    } catch (err) {
      console.error('Error disabling MFA:', err);
      error.value = 'Failed to disable MFA';
      return false;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Regenerate backup codes
   */
  async function regenerateBackupCodes(password: string): Promise<string[] | null> {
    try {
      loading.value = true;
      error.value = null;

      const result = await authService.regenerateBackupCodes(password);

      if (result.backup_codes) {
        backupCodes.value = result.backup_codes;
        successMessage.value = 'Backup codes regenerated successfully';
        return result.backup_codes;
      }

      return null;
    } catch (err) {
      console.error('Error regenerating backup codes:', err);
      error.value = 'Failed to regenerate backup codes';
      return null;
    } finally {
      loading.value = false;
    }
  }

  /**
   * Reset MFA setup state
   */
  function resetMFASetup(): void {
    mfaStep.value = 'setup';
    qrCodeUrl.value = '';
    mfaSecret.value = '';
    backupCodes.value = [];
    error.value = null;
    successMessage.value = null;
  }

  /**
   * Clear messages
   */
  function clearMessages(): void {
    error.value = null;
    successMessage.value = null;
  }

  return {
    // State
    loading,
    verifying,
    mfaEnabled,
    mfaStep,
    qrCodeUrl,
    mfaSecret,
    backupCodes,
    error,
    successMessage,

    // Computed
    isLoginSetup,
    showSetupSteps,
    showVerificationStep,
    showEnabledState,
    showSuccessState,

    // Methods
    checkMFAStatus,
    setupMFAForLogin,
    startMFASetup,
    verifyMFAToken,
    enableMFA,
    enableMFAForLogin,
    disableMFA,
    regenerateBackupCodes,
    resetMFASetup,
    clearMessages
  };
}
