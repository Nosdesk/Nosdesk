<!-- LoginView.vue -->
<script setup lang="ts">
import { ref, onMounted, nextTick, computed } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { useMfaSetupStore } from "@nosdesk/core/stores/mfaSetup";
import { useBrandingStore } from "@/stores/branding";
import { useThemeStore } from "@/stores/theme";
import { useMicrosoftAuth } from "@/composables/useMicrosoftAuth";
import { usePasskeys } from "@/composables/usePasskeys";
import type { PasskeyLoginResult } from "@/services/passkeyService";
import ForgotPasswordModal from "@/components/auth/ForgotPasswordModal.vue";
import AuthLayout from "@/components/auth/AuthLayout.vue";
import authService from "@nosdesk/core/services/authService";
import apiClient from "@nosdesk/core/apiClient";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import Icon from "@/components/common/Icon.vue";
import Spinner from "@/components/common/Spinner.vue";
import Button from "@/components/common/Button.vue";
import FormInput from "@/components/common/FormInput.vue";
import PasswordInput from "@/components/common/PasswordInput.vue";
import { extractErrorMessage } from "@/utils/errors";
import { isTauriRuntime } from "@/platform";

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
  loginWithPasskey,
  loginWithPasskeyConditional,
  error: passkeyError,
  checkSupport: checkPasskeySupport,
} = usePasskeys();
const email = ref("");
const password = ref("");
// Track which specific action is loading (null = nothing loading)
const loadingAction = ref<'login' | 'mfa' | 'microsoft' | 'oidc' | 'passkey' | null>(null);
// Computed for convenience - true if any action is loading
const isLoading = computed(() => loadingAction.value !== null);
const errorMessage = ref("");
const successMessage = ref("");
const showForgotPasswordModal = ref(false);
const microsoftAuthEnabled = ref(false);
const oidcEnabled = ref(false);
const oidcDisplayName = ref("SSO");
// Hosted mode: local password + passkey are disabled and the platform
// OIDC is the only sign-in path. The local forms are hidden and SSO is
// auto-initiated on mount.
const ssoOnly = ref(false);

// MFA state
const mfaToken = ref("");
const recoveryMode = ref(false);
const recoveryCode = ref("");

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
    ssoOnly.value = setupStatus.local_auth_disabled || false;

    // Hosted mode with OIDC configured: the platform OIDC is the only way
    // in, so go straight there instead of rendering a sign-in form the
    // user can't use. The local forms stay hidden (ssoOnly) as a fallback
    // if the redirect is blocked.
    if (ssoOnly.value && oidcEnabled.value) {
      void handleOidcLoginClick();
      return;
    }
  } catch {
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

  // Arm passkey autofill on the login form (no-op if unsupported).
  void startConditionalPasskeyLogin();
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
    if (!success && authStore.error && !authStore.mfaSetupRequired && !authStore.mfaRequired && !authStore.passkeyMfaRequired) {
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

    // If TOTP MFA is required, authStore.mfaRequired will be true
    // Clear any error messages since this is expected flow
    if (authStore.mfaRequired) {
      errorMessage.value = "";
    }

    // If passkey MFA is required, show the passkey verification screen.
    // The user must click the "Verify" button to trigger the WebAuthn ceremony
    // because navigator.credentials.get() requires a user gesture (transient activation).
    if (authStore.passkeyMfaRequired) {
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
  recoveryMode.value = false;
  recoveryCode.value = "";
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

// Apply a successful passkey login and redirect. The passkey response
// carries a slimmer user shape than the canonical User type; the auth
// store hydrates the rest from /me on its first authenticated fetch.
const completePasskeyLogin = (result: PasskeyLoginResult) => {
  authStore.user = result.user as unknown as typeof authStore.user;
  authStore.setAuthProvider('local');
  const redirectPath = router.currentRoute.value.query.redirect?.toString() || "/";
  router.push(redirectPath);
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
      completePasskeyLogin(result);
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

// Arm passkey autofill (conditional UI) in the background. If the user
// picks a passkey from the email field's autofill, log them straight in;
// otherwise it stays silent and the password / manual-passkey paths are
// unaffected. Started once the login form is confirmed to be showing.
const startConditionalPasskeyLogin = async () => {
  const result = await loginWithPasskeyConditional();
  if (result) completePasskeyLogin(result);
};

// Handle passkey MFA verification (after password login, passkey is the second factor)
const handlePasskeyMfaVerify = async () => {
  loadingAction.value = 'passkey';
  errorMessage.value = "";

  try {
    // Use the email from the login form for targeted passkey lookup
    const result = await loginWithPasskey(email.value.trim());

    if (result) {
      // Passkey verified - update auth store and redirect.
      // Slimmer user shape from passkey response; the auth store
      // hydrates the rest from /me.
      authStore.user = result.user as unknown as typeof authStore.user;
      authStore.setAuthProvider('local');
      authStore.passkeyMfaRequired = false;
      authStore.mfaUserUuid = '';

      const redirectPath = router.currentRoute.value.query.redirect?.toString() || "/";
      router.push(redirectPath);
    } else if (passkeyError.value) {
      errorMessage.value = passkeyError.value;
    }
  } catch (error) {
    console.error("Passkey MFA verification error:", error);
    errorMessage.value = passkeyError.value || "Failed to verify passkey. Please try again.";
  } finally {
    loadingAction.value = null;
  }
};

// Handle recovery code login (for passkey MFA users who can't use their passkey)
const handleRecoveryLogin = async () => {
  if (!recoveryCode.value.trim()) {
    errorMessage.value = "Please enter a recovery code";
    return;
  }

  loadingAction.value = 'mfa';
  errorMessage.value = "";

  try {
    const response = await apiClient.post('/auth/recovery-login', {
      email: email.value.trim(),
      password: password.value,
      recovery_code: recoveryCode.value.trim(),
    });

    if (response.data.success && response.data.csrf_token) {
      authStore.user = response.data.user;
      authStore.setAuthProvider('local');
      authStore.passkeyMfaRequired = false;
      authStore.mfaUserUuid = '';
      recoveryMode.value = false;
      recoveryCode.value = '';

      const redirectPath = router.currentRoute.value.query.redirect?.toString() || "/";
      router.push(redirectPath);
    } else {
      errorMessage.value = response.data.message || 'Recovery code verification failed';
    }
  } catch (error) {
    console.error("Recovery login error:", error);
    errorMessage.value = extractErrorMessage(error, "Invalid recovery code. Please try again.");
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

  // Native app: the web redirect flow can't return to the app, so run the
  // RFC 8252 native OIDC flow (system browser + PKCE) to get a bearer session,
  // then hydrate the auth store from /me and route in. (No-op import on web.)
  if (isTauriRuntime()) {
    try {
      const { loginWithOidc } = await import('@nosdesk/mobile');
      await loginWithOidc();
      await authStore.fetchUserData();
      authStore.setAuthProvider('oidc');
      const redirectPath = router.currentRoute.value.query.redirect?.toString() || "/";
      router.push(redirectPath);
    } catch (error) {
      const err = error as Error;
      errorMessage.value = err.message || "Sign-in failed";
      loadingAction.value = null;
    }
    return;
  }

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
  <AuthLayout>
    <template #logo>
      <img
        v-if="customLogoUrl"
        :src="customLogoUrl"
        :alt="brandingStore.appName + ' Logo'"
        class="h-9 w-auto max-w-[180px] object-contain"
      />
      <LogoIcon v-else class="h-9 w-auto text-accent" :aria-label="$t('nav-logo-alt')" />
    </template>

    <div class="flex flex-col gap-8">
      <!-- Success Message -->
      <div
        v-if="successMessage"
        class="bg-status-success/10 border border-status-success/50 text-status-success px-4 py-3 rounded-lg text-sm"
      >
        <div class="flex items-center gap-2">
          <Icon name="check" size="md" />
          {{ successMessage }}
        </div>
      </div>

      <!-- MFA Verification Form -->
      <div v-if="authStore.mfaRequired" class="flex flex-col gap-6">
        <!-- Header Section -->
        <div class="text-center">
          <div class="mb-4">
            <div class="inline-flex items-center justify-center w-12 h-12 bg-accent/10 rounded-full mb-4">
              <Icon name="lock" size="lg" class="text-accent" />
            </div>
            <h2 class="text-xl font-semibold text-primary mb-2">
              {{ $t("login-mfa-title") }}
            </h2>
            <p class="text-secondary text-sm">
              {{ $t("login-mfa-subtitle") }}
            </p>
          </div>
        </div>

        <!-- Error Message -->
        <div
          v-if="errorMessage"
          class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm flex items-center gap-2"
        >
          <Icon name="warning" class="text-status-error flex-shrink-0" />
          {{ errorMessage }}
        </div>

        <form @submit.prevent="handleMfaLogin" class="flex flex-col gap-6">
          <!-- MFA Code Input -->
          <div class="flex flex-col gap-2">
            <label
              for="mfa-token"
              class="block text-sm font-medium text-secondary"
            >
              {{ $t("login-mfa-code-label") }}
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
              {{ $t("login-mfa-code-help") }}
            </p>
          </div>

          <!-- Action Buttons -->
          <div class="flex gap-3">
            <button
              type="button"
              @click="handleBackToLogin"
              class="flex-1 py-3 px-4 border border-default rounded-lg text-sm font-medium text-secondary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent transition-colors"
            >
              {{ $t("login-mfa-back") }}
            </button>
            <button
              type="submit"
              :disabled="isLoading || !mfaToken.trim()"
              class="flex-2 py-3 px-6 border border-transparent rounded-lg shadow-sm text-sm font-medium text-on-accent bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
            >
              <Spinner v-if="loadingAction === 'mfa'" class="-ml-1 mr-2 text-white" />
              <span v-if="loadingAction === 'mfa'">{{ $t("login-mfa-verifying") }}</span>
              <span v-else>{{ $t("login-mfa-verify") }}</span>
            </button>
          </div>
        </form>
      </div>

      <!-- Passkey MFA Verification -->
      <!-- Password verified, just need passkey tap. navigator.credentials.get() -->
      <!-- requires a user gesture so we show the verify button inline. -->
      <div v-else-if="authStore.passkeyMfaRequired" class="flex flex-col gap-6">
        <div
          v-if="errorMessage"
          class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm"
        >
          {{ errorMessage }}
        </div>

        <div class="flex flex-col gap-1 text-center">
          <p class="text-sm text-secondary">
            {{ $t("login-passkey-mfa-verified", { email }) }}
          </p>
        </div>

        <!-- Recovery code mode -->
        <template v-if="recoveryMode">
          <form @submit.prevent="handleRecoveryLogin" class="flex flex-col gap-4">
            <div class="flex flex-col gap-2">
              <label for="recovery-code" class="block text-sm font-medium text-secondary">
                {{ $t("login-recovery-code-label") }}
              </label>
              <input
                id="recovery-code"
                v-model="recoveryCode"
                type="text"
                required
                autocomplete="off"
                :placeholder="$t('login-recovery-code-placeholder')"
                class="w-full px-4 py-3 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent text-center text-lg tracking-widest font-mono uppercase"
                maxlength="8"
              />
              <p class="text-xs text-tertiary text-center">
                {{ $t("login-recovery-code-help") }}
              </p>
            </div>

            <div class="flex gap-3">
              <button
                type="button"
                @click="recoveryMode = false; errorMessage = ''"
                class="flex-1 py-3 px-4 border border-default rounded-lg text-sm font-medium text-secondary bg-surface hover:bg-surface-hover transition-colors"
              >
                {{ $t("login-mfa-back") }}
              </button>
              <button
                type="submit"
                :disabled="isLoading || !recoveryCode.trim()"
                class="flex-2 py-3 px-6 border border-transparent rounded-lg shadow-sm text-sm font-medium text-on-accent bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
              >
                <Spinner v-if="loadingAction === 'mfa'" class="text-white" />
                <span v-if="loadingAction === 'mfa'">{{ $t("login-mfa-verifying") }}</span>
                <span v-else>{{ $t("login-mfa-verify") }}</span>
              </button>
            </div>
          </form>
        </template>

        <!-- Passkey verification mode (default) -->
        <template v-else>
          <button
            type="button"
            @click="handlePasskeyMfaVerify"
            :disabled="isLoading"
            class="w-full flex justify-center items-center gap-2 py-2 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-on-accent bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Spinner v-if="loadingAction === 'passkey'" />
            <Icon v-else name="key" size="md" />
            <span v-if="loadingAction === 'passkey'">{{ $t("login-mfa-verifying") }}</span>
            <span v-else>{{ $t("login-passkey-mfa-verify-cta") }}</span>
          </button>

          <button
            type="button"
            @click="recoveryMode = true; errorMessage = ''"
            class="text-sm text-tertiary hover:text-primary transition-colors"
          >
            {{ $t("login-passkey-mfa-use-recovery") }}
          </button>
        </template>

        <button
          type="button"
          @click="handleBackToLogin"
          class="text-sm text-tertiary hover:text-primary transition-colors"
        >
          {{ $t("login-passkey-mfa-back-to-login") }}
        </button>
      </div>

      <!-- Login Form -->
      <form v-else-if="!ssoOnly" @submit.prevent="handleLogin" class="flex flex-col gap-5">
        <header class="flex flex-col gap-1.5">
          <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
            {{ $t('login-title') }}
          </h1>
          <p class="text-base text-secondary">{{ $t('login-subtitle') }}</p>
        </header>

        <!-- Error Message within login form -->
        <div
          v-if="errorMessage && !authStore.mfaSetupRequired && !authStore.mfaRequired && !authStore.passkeyMfaRequired"
          class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm"
        >
          {{ errorMessage }}
        </div>

        <FormInput
          v-model="email"
          :label="$t('login-email-label')"
          type="email"
          required
          autocomplete="username webauthn"
          :placeholder="$t('login-email-placeholder')"
        />

        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between">
            <label for="password" class="text-xs font-medium text-tertiary uppercase tracking-wide">
              {{ $t('login-password-label') }}
            </label>
            <button
              type="button"
              @click="showForgotPasswordModal = true"
              class="text-xs font-medium text-accent hover:opacity-80 transition-opacity"
            >
              {{ $t('login-forgot-password') }}
            </button>
          </div>
          <PasswordInput
            id="password"
            v-model="password"
            required
            autocomplete="current-password"
            :placeholder="$t('login-password-placeholder')"
          />
        </div>

        <Button
          type="submit"
          variant="primary"
          block
          :disabled="isLoading"
          :loading="loadingAction === 'login'"
        >
          {{ loadingAction === 'login' ? $t('login-submitting') : $t('login-submit') }}
        </Button>

        <!-- Passkey Login Button -->
        <Button
          v-if="passkeySupported"
          type="button"
          variant="secondary"
          block
          icon="key"
          :disabled="isLoading"
          :loading="loadingAction === 'passkey'"
          @click="handlePasskeyLogin"
        >
          {{ loadingAction === 'passkey' ? $t('login-passkey-authenticating') : $t('login-passkey-cta') }}
        </Button>

        <div v-if="microsoftAuthEnabled || oidcEnabled" class="relative flex gap-2 items-center justify-center">
          <div class="border-t border-default flex-grow"></div>
          <span class="mx-4 text-sm text-tertiary">{{ $t("login-divider-or") }}</span>
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
              <Spinner v-if="loadingAction === 'microsoft'" class="mr-2" />
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
              <span v-if="loadingAction === 'microsoft'">{{ $t("login-microsoft-connecting") }}</span>
              <span v-else>{{ $t("login-microsoft-cta") }}</span>
            </button>

            <button
              type="button"
              @click="handleMicrosoftLogoutClick"
              :title="$t('login-microsoft-logout-title')"
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
              <Spinner v-if="loadingAction === 'oidc'" size="md" class="mr-2" />
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
              <span v-if="loadingAction === 'oidc'">{{ $t("login-oidc-connecting") }}</span>
              <span v-else>{{ $t("login-oidc-cta", { provider: oidcDisplayName }) }}</span>
            </button>

            <button
              type="button"
              @click="handleOidcLogoutClick"
              :title="$t('login-oidc-logout-title', { provider: oidcDisplayName })"
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

      <!-- SSO-only (hosted mode): the platform OIDC is the only sign-in
           path. onMounted auto-initiates the redirect; this is the visible
           state during it, with a manual fallback if it was blocked. -->
      <div v-else class="flex flex-col gap-5">
        <header class="flex flex-col gap-1.5">
          <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
            {{ $t('login-title') }}
          </h1>
          <p class="text-base text-secondary">{{ $t('login-subtitle') }}</p>
        </header>

        <div
          v-if="errorMessage"
          class="bg-status-error/10 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm"
        >
          {{ errorMessage }}
        </div>

        <button
          type="button"
          @click="handleOidcLoginClick"
          :disabled="loadingAction === 'oidc'"
          class="flex-1 flex gap-1 justify-center items-center py-2 px-4 border border-default rounded-lg shadow-sm text-sm font-medium text-secondary bg-surface hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Spinner v-if="loadingAction === 'oidc'" size="md" class="mr-2" />
          <span v-if="loadingAction === 'oidc'">{{ $t("login-oidc-connecting") }}</span>
          <span v-else>{{ $t("login-oidc-cta", { provider: oidcDisplayName }) }}</span>
        </button>
      </div>

      <!-- Forgot Password Modal -->
      <ForgotPasswordModal
        :is-open="showForgotPasswordModal"
        @close="showForgotPasswordModal = false"
      />
    </div>
  </AuthLayout>
</template>
