import { ref, computed } from 'vue';
import authService from '@nosdesk/core/services/authService';
import { logger } from '@nosdesk/core/utils/logger';

export type StepUpMethod = 'password' | 'mfa' | 'none';

/**
 * Which credential a step-up re-auth asks the signed-in user for, matching
 * the backend's `verify_step_up`: the local password first, then a TOTP or
 * backup code, and 'none' when the account has neither (SSO-only, no MFA).
 */
export function useStepUpMethod() {
  const hasLocalPassword = ref(false);
  const mfaEnabled = ref(false);
  /** False until `load` finishes, so a caller doesn't offer 'none' early. */
  const loaded = ref(false);

  const method = computed<StepUpMethod>(() => {
    if (hasLocalPassword.value) return 'password';
    if (mfaEnabled.value) return 'mfa';
    return 'none';
  });

  /** Best-effort: a failed lookup leaves that method off. */
  async function load() {
    try {
      const identities = await authService.getUserAuthIdentities();
      hasLocalPassword.value = identities.some((i) => i.provider_type === 'local');
    } catch (err) {
      logger.debug('Could not load auth identities for step-up', { error: err });
    }
    try {
      const status = await authService.getMFAStatus();
      mfaEnabled.value = status.enabled;
    } catch (err) {
      logger.debug('Could not load MFA status for step-up', { error: err });
    }
    loaded.value = true;
  }

  /** The request body for the entered value. */
  function credential(value: string): { password?: string; mfa_code?: string } {
    if (method.value === 'password') return { password: value };
    if (method.value === 'mfa') return { mfa_code: value.trim() };
    return {};
  }

  return { method, loaded, load, credential };
}
