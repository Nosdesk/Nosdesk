import { ref } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import { useMfaSetupStore } from '@/stores/mfaSetup';

export interface AutoLoginOptions {
  /** Source identifier for MFA setup (e.g., 'onboarding', 'invitation') */
  source: string;
  /** Callback when login succeeds */
  onSuccess?: () => void;
  /** Callback when MFA is required */
  onMfaRequired?: () => void;
  /** Callback when MFA setup is required */
  onMfaSetupRequired?: () => void;
  /** Callback when login fails */
  onFallback?: () => void;
  /** Delay before redirect in ms */
  redirectDelay?: number;
}

export function useAutoLogin(options: AutoLoginOptions) {
  const router = useRouter();
  const authStore = useAuthStore();
  const mfaSetupStore = useMfaSetupStore();

  const isLoggingIn = ref(false);
  const isComplete = ref(false);
  const message = ref('');

  const {
    source,
    onSuccess,
    onMfaRequired,
    onMfaSetupRequired,
    onFallback,
    redirectDelay = 1000
  } = options;

  /**
   * Attempt to automatically log in the user after account creation/activation
   */
  const attemptLogin = async (email: string, password: string): Promise<boolean> => {
    if (!email) {
      message.value = 'Please log in with your credentials.';
      onFallback?.();
      return false;
    }

    isLoggingIn.value = true;
    message.value = 'Logging you in...';

    try {
      const loginSuccess = await authStore.login({ email, password });

      if (loginSuccess) {
        isComplete.value = true;
        message.value = 'Successfully logged in! Redirecting...';
        onSuccess?.();

        setTimeout(() => {
          router.push('/');
        }, redirectDelay);
        return true;
      }

      // Handle MFA scenarios
      if (authStore.mfaRequired) {
        message.value = 'MFA verification required.';
        onMfaRequired?.();

        setTimeout(() => {
          router.push({
            path: '/login',
            query: {
              message: 'Please complete MFA verification.',
              email
            }
          });
        }, redirectDelay * 1.5);
        return true;
      }

      if (authStore.mfaSetupRequired) {
        message.value = 'Setting up multi-factor authentication...';
        mfaSetupStore.setCredentials(email, password, source);
        onMfaSetupRequired?.();

        setTimeout(() => {
          router.push({ name: 'mfa-setup' });
        }, redirectDelay * 1.5);
        return true;
      }

      // Fallback - login didn't succeed for unknown reason
      message.value = 'Please log in with your credentials.';
      isLoggingIn.value = false;
      onFallback?.();
      return false;
    } catch (error) {
      console.error('Auto-login error:', error);
      message.value = 'Please log in with your credentials.';
      isLoggingIn.value = false;
      onFallback?.();
      return false;
    }
  };

  return {
    isLoggingIn,
    isComplete,
    message,
    attemptLogin
  };
}
