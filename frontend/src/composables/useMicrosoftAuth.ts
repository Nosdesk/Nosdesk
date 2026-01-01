import { ref } from 'vue';
import apiClient from '@/services/apiConfig';

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
      const axiosError = err as { response?: { data?: { message?: string; error?: string } } };
      error.value = axiosError.response?.data?.message ||
                   axiosError.response?.data?.error ||
                   'Failed to initiate Microsoft authentication';
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
      const axiosError = err as { response?: { data?: { message?: string } } };
      error.value = axiosError.response?.data?.message || 'Failed to initiate Microsoft logout';
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