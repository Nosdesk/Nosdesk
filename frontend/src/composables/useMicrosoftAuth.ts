import { ref } from 'vue';
import apiClient from '@/services/apiConfig';
import { translate } from '@/i18n';
import { extractErrorMessage } from '@/utils/errors';

export function useMicrosoftAuth() {
  const isLoading = ref(false);
  const error = ref<string | null>(null);

  const handleMicrosoftLogin = async (redirectUri?: string) => {
    isLoading.value = true;
    error.value = null;

    try {
      // Store the current URL to redirect back after authentication
      const redirectPath = redirectUri || window.location.pathname;
      sessionStorage.setItem('authRedirect', redirectPath);

      // Get authorization URL from backend
      const response = await apiClient.post('/auth/oauth/authorize', {
        provider_type: 'microsoft',
        redirect_uri: `${window.location.origin}/auth/microsoft/callback`,
      });

      // Make sure we got a valid auth URL
      if (response.data && response.data.auth_url) {
        // Redirect to Microsoft login
        window.location.href = response.data.auth_url;
      } else {
        throw new Error('Invalid authorization URL received');
      }
    } catch (err) {
      console.error('Error initiating Microsoft authentication:', err);
      error.value = extractErrorMessage(err, 'Failed to initiate Microsoft authentication');
      isLoading.value = false;
    }
  };

  const handleMicrosoftLogout = async (redirectUri?: string) => {
    isLoading.value = true;
    error.value = null;

    try {
      // Get the sign-out URL from backend
      const response = await apiClient.post('/auth/oauth/logout', {
        provider_type: 'microsoft',
        redirect_uri: redirectUri || window.location.href,
      });

      // Redirect to Microsoft logout page
      if (response.data && response.data.logout_url) {
        window.location.href = response.data.logout_url;
      } else {
        throw new Error('Invalid logout URL received');
      }
    } catch (err) {
      console.error('Error logging out of Microsoft:', err);
      error.value = extractErrorMessage(err, translate('auth-microsoft-logout-failed', undefined, 'Failed to initiate Microsoft logout'));
      isLoading.value = false;
    }
  };

  const handleMicrosoftLogoutAndRetry = async () => {
    // Logout of current Microsoft session and redirect to login to try again
    await handleMicrosoftLogout(`${window.location.origin}/login`);
  };

  return {
    isLoading,
    error,
    handleMicrosoftLogin,
    handleMicrosoftLogout,
    handleMicrosoftLogoutAndRetry,
  };
} 