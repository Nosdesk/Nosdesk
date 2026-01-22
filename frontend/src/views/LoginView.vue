<!-- LoginView.vue -->
<script setup lang="ts">
import { ref, onMounted, nextTick, computed } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { useMfaSetupStore } from "@/stores/mfaSetup";
import { useBrandingStore } from "@/stores/branding";
import { useThemeStore } from "@/stores/theme";
import { useMicrosoftAuth } from "@/composables/useMicrosoftAuth";
import { usePasskeys } from "@/composables/usePasskeys";
import ForgotPasswordModal from "@/components/auth/ForgotPasswordModal.vue";
import MFARecoveryModal from "@/components/auth/MFARecoveryModal.vue";
import Checkbox from "@/components/common/Checkbox.vue";
import authService from "@/services/authService";
import LogoIcon from "@/components/icons/LogoIcon.vue";

// Get branding and theme stores
const brandingStore = useBrandingStore();
const themeStore = useThemeStore();

// Computed logo URL - use custom logo if available
const customLogoUrl = computed(() => {
  return brandingStore.getLogoUrl(themeStore.isDarkMode);
});

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const mfaSetupStore = useMfaSetupStore();
const { handleMicrosoftLogin, handleMicrosoftLogout, error: microsoftError } = useMicrosoftAuth();
const {
  isSupported: passkeySupported,
  authenticating: passkeyAuthenticating,
  loginWithPasskey,
  error: passkeyError,
  checkSupport: checkPasskeySupport,
} = usePasskeys();
const email = ref("");
const password = ref("");
const showPassword = ref(false);
const rememberMe = ref(false);
// Track which specific action is loading (null = nothing loading)
const loadingAction = ref<'login' | 'mfa' | 'microsoft' | 'oidc' | 'passkey' | null>(null);
// Computed for convenience - true if any action is loading
const isLoading = computed(() => loadingAction.value !== null);
const errorMessage = ref("");
const successMessage = ref("");
const showForgotPasswordModal = ref(false);
const showMFARecoveryModal = ref(false);
const microsoftAuthEnabled = ref(false);
const oidcEnabled = ref(false);
const oidcDisplayName = ref("SSO");

// MFA state
const mfaToken = ref("");

// Check for success message and email prefill from URL query params (e.g., from onboarding)
onMounted(async () => {
  // Load branding if not already loaded (important for blank layout pages like login)
  if (!brandingStore.isLoaded) {
    brandingStore.loadBranding();
  }

  // Check passkey support
  await checkPasskeySupport();

  // Check if onboarding is required before showing login
  try {
    const setupStatus = await authService.checkSetupStatus();
    if (setupStatus.requires_setup) {
      router.replace({ name: 'onboarding' });
      return;
    }
    microsoftAuthEnabled.value = setupStatus.microsoft_auth_enabled || false;
    oidcEnabled.value = setupStatus.oidc_enabled || false;
    oidcDisplayName.value = setupStatus.oidc_display_name || "SSO";
  } catch (error) {
    // Continue to show login page if check fails
  }

  if (route.query.message) {
    successMessage.value = route.query.message as string;
  }

  // Prefill email if provided (e.g., from onboarding flow)
  if (route.query.email && typeof route.query.email === 'string') {
    email.value = route.query.email;
  }

  // Clean up the URL by removing the query parameters
  if (route.query.message || route.query.email) {
    router.replace({ name: "login" });
  }
});

const handleLogin = async () => {
  loadingAction.value = 'login';
  errorMessage.value = "";
  successMessage.value = "";

  try {
    const success = await authStore.login({
      email: email.value,
      password: password.value,
    });

    // Only show error if login failed and it's not due to MFA requirements
    if (!success && authStore.error && !authStore.mfaSetupRequired && !authStore.mfaRequired) {
      errorMessage.value = authStore.error;
    }

    // Check if MFA setup is required and redirect to MFA setup view
    if (authStore.mfaSetupRequired) {
      console.log('🔄 MFA setup required, redirecting to MFA setup view');

      // Store credentials securely in memory-only Pinia store
      mfaSetupStore.setCredentials(email.value, password.value, 'login');

      // Redirect to MFA setup view
      router.push({ name: "mfa-setup" });
      return;
    }

    // If MFA is required, authStore.mfaRequired will be true
    // Clear any error messages since this is expected flow
    if (authStore.mfaRequired) {
      errorMessage.value = "";
    }
  } catch (error) {
    console.error("Login error:", error);
    errorMessage.value = "An unexpected error occurred. Please try again.";
  } finally {
    loadingAction.value = null;
  }
};

const handleMfaLogin = async () => {
  if (!mfaToken.value.trim()) {
    errorMessage.value = "Please enter your MFA code";
    return;
  }

  loadingAction.value = 'mfa';
  errorMessage.value = "";

  try {
    const success = await authStore.verifyMfaAndLogin(
      email.value,
      password.value,
      mfaToken.value.trim()
    );

    if (!success && authStore.error) {
      errorMessage.value = authStore.error;
    }
    // If successful, authStore will handle redirect
    // and clear MFA state automatically
  } catch (error) {
    console.error("MFA login error:", error);
    errorMessage.value = "An unexpected error occurred. Please try again.";
  } finally {
    loadingAction.value = null;
  }
};

const handleBackToLogin = () => {
  authStore.clearMfaState();
  mfaToken.value = "";
  errorMessage.value = "";
  successMessage.value = "";
};

// Handle MFA input with validation and auto-submit
const handleMfaInput = (event: Event) => {
  const target = event.target as HTMLInputElement;
  const cleanValue = target.value.replace(/[^0-9A-Z]/g, '').toUpperCase();
  
  // Update the model value
  mfaToken.value = cleanValue;
  
  // Auto-submit when a complete code is entered (6 digits or 8-char backup code)
  if (cleanValue.length === 6 || cleanValue.length === 8) {
    nextTick(() => {
      if (!isLoading.value) {
        handleMfaLogin();
      }
    });
  }
};

// Handle paste events for MFA codes
const handleMfaPaste = (event: ClipboardEvent) => {
  event.preventDefault();
  const pastedText = event.clipboardData?.getData('text') || '';
  const cleanValue = pastedText.replace(/[^0-9A-Z]/g, '').toUpperCase();
  
  if (cleanValue.length >= 6) {
    // Take first 8 characters (max length for backup codes)
    mfaToken.value = cleanValue.slice(0, 8);
    
    // Auto-submit after paste
    nextTick(() => {
      if (!isLoading.value) {
        handleMfaLogin();
      }
    });
  } else {
    // If less than 6 chars, just set the value without submitting
    mfaToken.value = cleanValue;
  }
};

const handlePasskeyLogin = async () => {
  loadingAction.value = 'passkey';
  errorMessage.value = "";
  successMessage.value = "";

  try {
    // If email is entered, use email-based lookup (works with all passkeys)
    // If no email, use discoverable auth (requires resident key passkeys)
    const emailToUse = email.value.trim() || undefined;
    const result = await loginWithPasskey(emailToUse);

    if (result) {
      // Update auth store with the logged in user
      authStore.user = result.user;
      authStore.csrfToken = result.csrf_token;
      authStore.isAuthenticated = true;

      // Redirect to dashboard or intended page
      const redirectPath = router.currentRoute.value.query.redirect?.toString() || "/";
      router.push(redirectPath);
    } else if (passkeyError.value) {
      errorMessage.value = passkeyError.value;
    }
  } catch (error) {
    console.error("Passkey login error:", error);
    errorMessage.value = passkeyError.value || "Failed to sign in with passkey";
  } finally {
    loadingAction.value = null;
  }
};

const handleMicrosoftLoginClick = async () => {
  loadingAction.value = 'microsoft';
  errorMessage.value = "";
  successMessage.value = "";

  try {
    const redirectPath =
      router.currentRoute.value.query.redirect?.toString() || "/";
    await handleMicrosoftLogin(redirectPath);
  } catch (error) {
    console.error("Error initiating Microsoft authentication:", error);
    errorMessage.value = microsoftError.value || "Failed to initiate Microsoft authentication";
    loadingAction.value = null;
  }
};

const handleMicrosoftLogoutClick = async () => {
  try {
    errorMessage.value = "";
    successMessage.value = "";
    await handleMicrosoftLogout(window.location.href);
  } catch (error) {
    console.error("Error logging out of Microsoft:", error);
    errorMessage.value = microsoftError.value || "Failed to initiate Microsoft logout";
  }
};

const handleOidcLoginClick = async () => {
  loadingAction.value = 'oidc';
  errorMessage.value = "";
  successMessage.value = "";

  try {
    const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || '';
    const response = await fetch(`${API_BASE_URL}/api/auth/oauth/authorize`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      credentials: 'include',
      body: JSON.stringify({
        provider_type: 'oidc',
      }),
    });

    if (!response.ok) {
      const errorData = await response.json().catch(() => ({}));
      throw new Error(errorData.message || 'Failed to initiate OIDC authentication');
    }

    const data = await response.json();
    if (data.auth_url) {
      window.location.href = data.auth_url;
    } else {
      throw new Error('No authorization URL received');
    }
  } catch (error) {
    console.error("Error initiating OIDC authentication:", error);
    const err = error as Error;
    errorMessage.value = err.message || "Failed to initiate SSO authentication";
    loadingAction.value = null;
  }
};

const handleOidcLogoutClick = async () => {
  try {
    errorMessage.value = "";
    successMessage.value = "";

    const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || '';
    const response = await fetch(`${API_BASE_URL}/api/auth/oauth/logout`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      credentials: 'include',
      body: JSON.stringify({
        provider_type: 'oidc',
        redirect_uri: window.location.href,
      }),
    });

    if (!response.ok) {
      throw new Error('Failed to get logout URL');
    }

    const data = await response.json();
    if (data.logout_url) {
      window.location.href = data.logout_url;
    } else {
      // Provider doesn't support logout, show message
      successMessage.value = data.message || 'Logged out of application. You may still be signed in to your identity provider.';
    }
  } catch (error) {
    console.error("Error logging out of OIDC provider:", error);
    const err = error as Error;
    errorMessage.value = err.message || "Failed to initiate SSO logout";
  }
};
</script>

<template>
  <div class="min-h-screen w-full flex flex-col items-center justify-center bg-app py-8">
    <div class="flex flex-col gap-4 w-full max-w-md px-8">
      <!-- Logo/Brand -->
      <div class="flex flex-col gap-2 items-center">
        <img
          v-if="customLogoUrl"
          :src="customLogoUrl"
          :alt="brandingStore.appName + ' Logo'"
          class="h-12 max-w-full px-4 object-contain"
        />
        <LogoIcon v-else class="h-12 px-4 text-accent" aria-label="Nosdesk Logo" />
        <p class="text-secondary mt-2">Sign in to your account</p>
      </div>

      <!-- Success Message -->
      <div
        v-if="successMessage"
        class="bg-status-success/10 border border-status-success/50 text-status-success px-4 py-3 rounded-lg text-sm"
      >
        <div class="flex items-center gap-2">
          <svg
            class="w-5 h-5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M5 13l4 4L19 7"
            ></path>
          </svg>
          {{ successMessage }}
        </div>
      </div>

      <!-- MFA Verification Form -->
      <div v-if="authStore.mfaRequired" class="flex flex-col gap-6">
        <!-- Header Section -->
        <div class="text-center">
          <div class="mb-4">
            <div class="inline-flex items-center justify-center w-12 h-12 bg-accent/10 rounded-full mb-4">
              <svg
                class="w-6 h-6 text-accent"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"
                ></path>
              </svg>
            </div>
            <h2 class="text-xl font-semibold text-primary mb-2">
              Two-Factor Authentication
            </h2>
            <p class="text-secondary text-sm">
              Please enter your authentication code
            </p>
          </div>
        </div>

        <!-- Error Message -->
        <div
          v-if="errorMessage"
          class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm flex items-center gap-2"
        >
          <svg
            class="w-4 h-4 text-status-error flex-shrink-0"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            ></path>
          </svg>
          {{ errorMessage }}
        </div>

        <form @submit.prevent="handleMfaLogin" class="flex flex-col gap-6">
          <!-- MFA Code Input -->
          <div class="flex flex-col gap-2">
            <label
              for="mfa-token"
              class="block text-sm font-medium text-secondary"
            >
              Authentication Code
            </label>
            <div class="relative">
              <input
                id="mfa-token"
                v-model="mfaToken"
                type="text"
                inputmode="numeric"
                required
                autocomplete="one-time-code"
                placeholder="000000"
                class="w-full px-4 py-3 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent text-center text-xl tracking-[0.5em] font-mono"
                maxlength="8"
                @input="handleMfaInput"
                @paste="handleMfaPaste"
              />
              <div
                class="absolute inset-y-0 right-0 pr-3 flex items-center pointer-events-none"
              >
                <svg
                  class="w-5 h-5 text-tertiary"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M12 18h.01M8 21h8a2 2 0 002-2V5a2 2 0 00-2-2H8a2 2 0 00-2 2v14a2 2 0 002 2z"
                  ></path>
                </svg>
              </div>
            </div>
            <p class="text-xs text-tertiary text-center">
              Enter the 6-digit code from your authenticator app or an
              8-character backup code
            </p>
          </div>

          <!-- Action Buttons -->
          <div class="flex gap-3">
            <button
              type="button"
              @click="handleBackToLogin"
              class="flex-1 py-3 px-4 border border-default rounded-lg text-sm font-medium text-secondary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent transition-colors"
            >
              Back
            </button>
            <button
              type="submit"
              :disabled="isLoading || !mfaToken.trim()"
              class="flex-2 py-3 px-6 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
            >
              <svg
                v-if="loadingAction === 'mfa'"
                class="animate-spin -ml-1 mr-2 h-4 w-4 text-white"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle
                  class="opacity-25"
                  cx="12"
                  cy="12"
                  r="10"
                  stroke="currentColor"
                  stroke-width="4"
                ></circle>
                <path
                  class="opacity-75"
                  fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                ></path>
              </svg>
              <span v-if="loadingAction === 'mfa'">Verifying...</span>
              <span v-else>Verify & Sign In</span>
            </button>
          </div>

          <!-- MFA Recovery Link -->
          <div class="text-center">
            <button
              type="button"
              @click="showMFARecoveryModal = true"
              class="text-sm text-accent hover:text-accent transition-colors"
            >
              Lost access to your authenticator?
            </button>
          </div>
        </form>
      </div>

      <!-- Login Form -->
      <form v-else @submit.prevent="handleLogin" class="flex flex-col gap-6">
        <!-- Error Message within login form -->
        <div
          v-if="errorMessage && !authStore.mfaSetupRequired && !authStore.mfaRequired"
          class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm"
        >
          {{ errorMessage }}
        </div>

        <div class="flex flex-col gap-1">
          <label for="email" class="block text-sm font-medium text-secondary"
            >Email</label
          >
          <input
            id="email"
            v-model="email"
            type="email"
            required
            autocomplete="email"
            class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            placeholder="Enter your email"
          />
        </div>

        <div class="flex flex-col gap-1">
          <label for="password" class="block text-sm font-medium text-secondary"
            >Password</label
          >
          <div class="relative">
            <input
              id="password"
              v-model="password"
              :type="showPassword ? 'text' : 'password'"
              required
              autocomplete="current-password"
              class="mt-1 block w-full px-3 py-2 pr-10 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
              placeholder="Enter your password"
            />
            <button
              type="button"
              @click="showPassword = !showPassword"
              class="absolute inset-y-0 right-0 mt-1 px-3 flex items-center text-tertiary hover:text-secondary transition-colors"
              :aria-label="showPassword ? 'Hide password' : 'Show password'"
            >
              <!-- Eye icon (show) -->
              <svg
                v-if="!showPassword"
                class="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
                />
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"
                />
              </svg>
              <!-- Eye-off icon (hide) -->
              <svg
                v-else
                class="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21"
                />
              </svg>
            </button>
          </div>
        </div>

        <div class="flex items-center justify-between">
          <Checkbox
            id="remember-me"
            v-model="rememberMe"
            label="Remember me"
          />

          <button
            type="button"
            @click="showForgotPasswordModal = true"
            class="text-sm text-accent hover:text-accent transition-colors"
          >
            Forgot password?
          </button>
        </div>

        <button
          type="submit"
          :disabled="isLoading"
          class="w-full flex justify-center py-2 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <span v-if="loadingAction === 'login'">Signing in...</span>
          <span v-else>Sign in</span>
        </button>

        <!-- Passkey Login Button -->
        <button
          v-if="passkeySupported"
          type="button"
          @click="handlePasskeyLogin"
          :disabled="isLoading"
          class="w-full flex justify-center items-center gap-2 py-2 px-4 border border-default rounded-lg shadow-sm text-sm font-medium text-secondary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <svg
            v-if="loadingAction === 'passkey'"
            class="animate-spin h-4 w-4"
            fill="none"
            viewBox="0 0 24 24"
          >
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <svg
            v-else
            xmlns="http://www.w3.org/2000/svg"
            class="h-5 w-5"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
          </svg>
          <span v-if="loadingAction === 'passkey'">Authenticating...</span>
          <span v-else>Sign in with passkey</span>
        </button>

        <div v-if="microsoftAuthEnabled || oidcEnabled" class="relative flex gap-2 items-center justify-center">
          <div class="border-t border-default flex-grow"></div>
          <span class="mx-4 text-sm text-tertiary">or</span>
          <div class="border-t border-default flex-grow"></div>
        </div>

        <!-- SSO Provider Buttons -->
        <div v-if="microsoftAuthEnabled || oidcEnabled" class="flex flex-col gap-2">
          <!-- Microsoft Entra Button -->
          <div v-if="microsoftAuthEnabled" class="flex gap-2">
            <button
              type="button"
              @click="handleMicrosoftLoginClick"
              :disabled="isLoading"
              class="flex-1 flex gap-1 justify-center items-center py-2 px-4 border border-default rounded-lg shadow-sm text-sm font-medium text-secondary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <svg
                v-if="loadingAction === 'microsoft'"
                class="animate-spin h-4 w-4 mr-2"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg
                v-else
                xmlns="http://www.w3.org/2000/svg"
                width="16"
                height="16"
                viewBox="0 0 21 21"
                class="mr-2"
              >
                <rect x="1" y="1" width="9" height="9" fill="#f25022" />
                <rect x="1" y="11" width="9" height="9" fill="#00a4ef" />
                <rect x="11" y="1" width="9" height="9" fill="#7fba00" />
                <rect x="11" y="11" width="9" height="9" fill="#ffb900" />
              </svg>
              <span v-if="loadingAction === 'microsoft'">Connecting...</span>
              <span v-else>Sign in with Microsoft Entra</span>
            </button>

            <button
              type="button"
              @click="handleMicrosoftLogoutClick"
              title="Sign out of Microsoft account"
              class="p-2 border border-default rounded-lg text-tertiary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                class="h-5 w-5"
                viewBox="0 0 20 20"
                fill="currentColor"
              >
                <path
                  fill-rule="evenodd"
                  d="M3 3a1 1 0 00-1 1v12a1 1 0 102 0V4a1 1 0 00-1-1zm10.293 9.293a1 1 0 001.414 1.414l3-3a1 1 0 000-1.414l-3-3a1 1 0 10-1.414 1.414L14.586 9H7a1 1 0 100 2h7.586l-1.293 1.293z"
                  clip-rule="evenodd"
                />
              </svg>
            </button>
          </div>

          <!-- OIDC/SSO Button -->
          <div v-if="oidcEnabled" class="flex gap-2">
            <button
              type="button"
              @click="handleOidcLoginClick"
              :disabled="isLoading"
              class="flex-1 flex gap-1 justify-center items-center py-2 px-4 border border-default rounded-lg shadow-sm text-sm font-medium text-secondary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <svg
                v-if="loadingAction === 'oidc'"
                class="animate-spin h-5 w-5 mr-2"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg
                v-else
                xmlns="http://www.w3.org/2000/svg"
                class="h-5 w-5 mr-2"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              >
                <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" />
                <polyline points="10 17 15 12 10 7" />
                <line x1="15" y1="12" x2="3" y2="12" />
              </svg>
              <span v-if="loadingAction === 'oidc'">Connecting...</span>
              <span v-else>Sign in with {{ oidcDisplayName }}</span>
            </button>

            <button
              type="button"
              @click="handleOidcLogoutClick"
              :title="`Sign out of ${oidcDisplayName} account`"
              class="p-2 border border-default rounded-lg text-tertiary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                class="h-5 w-5"
                viewBox="0 0 20 20"
                fill="currentColor"
              >
                <path
                  fill-rule="evenodd"
                  d="M3 3a1 1 0 00-1 1v12a1 1 0 102 0V4a1 1 0 00-1-1zm10.293 9.293a1 1 0 001.414 1.414l3-3a1 1 0 000-1.414l-3-3a1 1 0 10-1.414 1.414L14.586 9H7a1 1 0 100 2h7.586l-1.293 1.293z"
                  clip-rule="evenodd"
                />
              </svg>
            </button>
          </div>
        </div>
      </form>

      <!-- Forgot Password Modal -->
      <ForgotPasswordModal
        :is-open="showForgotPasswordModal"
        @close="showForgotPasswordModal = false"
      />

      <!-- MFA Recovery Modal -->
      <MFARecoveryModal
        :is-open="showMFARecoveryModal"
        @close="showMFARecoveryModal = false"
      />
    </div>
  </div>
</template>
