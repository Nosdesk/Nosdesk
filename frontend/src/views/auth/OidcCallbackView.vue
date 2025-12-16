<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import apiClient from '@/services/apiConfig';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();

const error = ref<string | null>(null);
const detailedError = ref<string | null>(null);
const loading = ref(true);
const message = ref('Completing sign-in...');
const showTechnicalDetails = ref(false);

// Computed property to determine error type and appropriate messaging
const errorInfo = computed(() => {
  if (!error.value) return null;

  const errorMsg = error.value.toLowerCase();

  if (errorMsg.includes('already connected') || errorMsg.includes('already linked')) {
    return {
      type: 'already_connected',
      title: 'Account Already Connected',
      message: 'This account is already linked to another user in the system.',
      suggestion: 'Please try signing in with a different account, or contact your administrator if you believe this is an error.',
      icon: 'link',
      actions: [
        { label: 'Return to Login', action: 'login' }
      ]
    };
  }

  if (errorMsg.includes('not found') || errorMsg.includes('invalid')) {
    return {
      type: 'invalid_request',
      title: 'Authentication Failed',
      message: 'The authentication request was invalid or has expired.',
      suggestion: 'Please try signing in again.',
      icon: 'warning',
      actions: [
        { label: 'Try Again', action: 'login' }
      ]
    };
  }

  // Generic error
  return {
    type: 'generic',
    title: 'Authentication Failed',
    message: error.value,
    suggestion: 'Please try again or contact support if the problem persists.',
    icon: 'error',
    actions: [
      { label: 'Return to Login', action: 'login' }
    ]
  };
});

const handleAction = (action: string) => {
  switch (action) {
    case 'login':
      router.push('/login');
      break;
  }
};

onMounted(async () => {
  // Extract the code and state parameters from the URL
  const code = route.query.code as string | undefined;
  const state = route.query.state as string | undefined;
  const errorParam = route.query.error as string | undefined;
  const errorDescription = route.query.error_description as string | undefined;

  // Log the received parameters for debugging
  console.log('OIDC Callback URL params:', {
    code: code ? `${code.substring(0, 20)}...` : 'missing',
    state: state ? `${state.substring(0, 20)}...` : 'missing',
    error: errorParam,
    errorDescription
  });

  // Handle errors returned by the OIDC provider
  if (errorParam) {
    error.value = errorDescription || errorParam;
    loading.value = false;
    return;
  }

  // Validate that we have code and state
  if (!code || !state) {
    error.value = 'Missing required authentication parameters';
    detailedError.value = `Missing: ${!code ? 'code' : ''} ${!state ? 'state' : ''}`;
    loading.value = false;
    return;
  }

  try {
    // Call backend to handle the OAuth code exchange with the code and state parameters
    message.value = 'Processing authentication response...';
    console.log('Sending OIDC callback request to backend...');

    const response = await apiClient.get(`/auth/oauth/callback`, {
      params: {
        code,
        state
      }
    });

    console.log('Backend response received:', response.status);

    // Handle cookie-based authentication response
    const data = response.data;

    if (data && data.success && data.csrf_token) {
      message.value = 'Authentication successful, redirecting...';
      console.log('Authentication successful, cookies set, user data:', data.user);

      // Set auth provider to oidc
      authStore.setAuthProvider('oidc');

      // Use the user data from the response (already authenticated via cookies)
      if (data.user) {
        authStore.user = data.user;
      }

      // Redirect to dashboard or original destination
      let redirectPath = sessionStorage.getItem('authRedirect') || '/';
      // If the redirect is the callback URL, go to home instead
      if (redirectPath.includes('/auth/oidc/callback')) {
        redirectPath = '/';
      }
      sessionStorage.removeItem('authRedirect'); // Clear stored redirect
      console.log('Redirecting to:', redirectPath);
      router.push(redirectPath);
    } else {
      console.error('Invalid response format:', data);
      error.value = 'Invalid response from server';
      detailedError.value = 'Server response did not contain expected authentication data. Response format: ' +
                          JSON.stringify(data, null, 2).substring(0, 200);
      loading.value = false;
    }
  } catch (err: any) {
    console.error('Error during OIDC authentication:', err);

    // Try to extract error message from response if available
    const errorMsg = err.response?.data?.message ||
                    err.response?.data?.error ||
                    'An unexpected error occurred during authentication';

    // Get detailed error information
    let details = '';
    if (err.response) {
      details = `Status: ${err.response.status}\nData: ${JSON.stringify(err.response.data, null, 2)}`;
    } else if (err.request) {
      details = 'No response received from server';
    } else {
      details = err.message || 'Unknown error';
    }

    error.value = errorMsg;
    detailedError.value = details;
    loading.value = false;
  }
});
</script>

<template>
  <div class="h-screen flex items-center justify-center bg-slate-900 p-4">
    <div class="bg-slate-800 p-8 rounded-xl shadow-lg max-w-md w-full border border-slate-700/50">
      <div v-if="loading" class="flex flex-col items-center justify-center gap-4">
        <div class="animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-blue-500"></div>
        <h2 class="text-xl font-medium text-white">{{ message }}</h2>
        <p class="text-slate-400 text-center">Please wait while we complete your authentication</p>
      </div>

      <div v-else-if="error && errorInfo" class="flex flex-col items-center justify-center gap-6">
        <!-- Error Icon -->
        <div class="rounded-full p-4" :class="{
          'bg-red-500': errorInfo.icon === 'error',
          'bg-orange-500': errorInfo.icon === 'warning',
          'bg-blue-500': errorInfo.icon === 'link'
        }">
          <!-- Link Icon for already connected -->
          <svg v-if="errorInfo.icon === 'link'" xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
          </svg>
          <!-- Warning Icon -->
          <svg v-else-if="errorInfo.icon === 'warning'" xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16.5c-.77.833.192 2.5 1.732 2.5z" />
          </svg>
          <!-- X Icon for generic errors -->
          <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </div>

        <!-- Error Title and Message -->
        <div class="text-center">
          <h2 class="text-xl font-medium text-white mb-2">{{ errorInfo.title }}</h2>
          <p class="text-slate-300 mb-3">{{ errorInfo.message }}</p>
          <p class="text-sm text-slate-400">{{ errorInfo.suggestion }}</p>
        </div>

        <!-- Action Buttons -->
        <div class="flex flex-col gap-2 w-full">
          <button
            v-for="action in errorInfo.actions"
            :key="action.action"
            @click="handleAction(action.action)"
            class="px-4 py-2 rounded-lg transition-colors text-sm font-medium bg-brand-blue hover:opacity-90 text-white"
          >
            {{ action.label }}
          </button>
        </div>

        <!-- Technical Details (Collapsible) -->
        <div v-if="detailedError" class="w-full">
          <button
            @click="showTechnicalDetails = !showTechnicalDetails"
            class="flex items-center gap-2 text-sm text-slate-400 hover:text-slate-300 transition-colors"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="h-4 w-4 transition-transform"
              :class="{ 'rotate-90': showTechnicalDetails }"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
            >
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
            </svg>
            Technical Details
          </button>

          <div v-if="showTechnicalDetails" class="mt-2 overflow-auto max-h-40 bg-slate-900 p-3 rounded-lg border border-slate-700">
            <pre class="text-xs text-slate-300 font-mono whitespace-pre-wrap">{{ detailedError }}</pre>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
