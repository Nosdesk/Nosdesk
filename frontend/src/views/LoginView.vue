<!-- LoginView.vue -->
<script setup lang="ts">
import { ref, onMounted, nextTick } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { useMicrosoftAuth } from "@/composables/useMicrosoftAuth";
import ForgotPasswordModal from "@/components/auth/ForgotPasswordModal.vue";
import MFARecoveryModal from "@/components/auth/MFARecoveryModal.vue";
import logo from "@/assets/logo.svg";
import axios from "axios";

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const { handleMicrosoftLogin, handleMicrosoftLogout, error: microsoftError } = useMicrosoftAuth();
const email = ref("");
const password = ref("");
const rememberMe = ref(false);
const isLoading = ref(false);
const errorMessage = ref("");
const successMessage = ref("");
const showForgotPasswordModal = ref(false);
const showMFARecoveryModal = ref(false);

// MFA state
const mfaToken = ref("");

// Check for success message and email prefill from URL query params (e.g., from onboarding)
onMounted(() => {
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
  isLoading.value = true;
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
      
      // Store credentials in sessionStorage for MFA setup
      sessionStorage.setItem('mfaSetupCredentials', JSON.stringify({
        email: email.value,
        password: password.value,
        from: 'login',
        timestamp: Date.now()
      }));
      
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
    isLoading.value = false;
  }
};

const handleMfaLogin = async () => {
  if (!mfaToken.value.trim()) {
    errorMessage.value = "Please enter your MFA code";
    return;
  }

  isLoading.value = true;
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
    isLoading.value = false;
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
  
  // Auto-submit when we have a complete code (6 digits or 8-char backup code)
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

const handleMicrosoftLoginClick = async () => {
  isLoading.value = true;
  errorMessage.value = "";
  successMessage.value = "";

  try {
    const redirectPath =
      router.currentRoute.value.query.redirect?.toString() || "/";
    await handleMicrosoftLogin(redirectPath);
  } catch (error: any) {
    console.error("Error initiating Microsoft authentication:", error);
    errorMessage.value = microsoftError.value || "Failed to initiate Microsoft authentication";
    isLoading.value = false;
  }
};

const handleMicrosoftLogoutClick = async () => {
  try {
    errorMessage.value = "";
    successMessage.value = "";
    await handleMicrosoftLogout(window.location.href);
  } catch (error: any) {
    console.error("Error logging out of Microsoft:", error);
    errorMessage.value = microsoftError.value || "Failed to initiate Microsoft logout";
  }
};
</script>

<template>
  <div
    class="min-h-screen w-full flex items-center justify-center bg-slate-900"
  >
    <div class="flex flex-col gap-4 w-full max-w-md p-8">
      <!-- Logo/Brand -->
      <div class="flex flex-col gap-2 items-center">
        <img :src="logo" alt="Nosdesk Logo" class="px-4" />
        <p class="text-slate-400 mt-2">Sign in to your account</p>
      </div>

      <!-- Success Message -->
      <div
        v-if="successMessage"
        class="bg-green-900/50 border border-green-700 text-green-200 px-4 py-3 rounded-lg text-sm"
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
            <div class="inline-flex items-center justify-center w-12 h-12 bg-blue-600/10 rounded-full mb-4">
              <svg
                class="w-6 h-6 text-blue-500"
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
            <h2 class="text-xl font-semibold text-white mb-2">
              Two-Factor Authentication
            </h2>
            <p class="text-slate-400 text-sm">
              Please enter your authentication code
            </p>
          </div>
        </div>

        <!-- Error Message -->
        <div
          v-if="errorMessage"
          class="bg-red-900/20 border border-red-700/50 text-red-200 px-4 py-3 rounded-lg text-sm flex items-center gap-2"
        >
          <svg
            class="w-4 h-4 text-red-400 flex-shrink-0"
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
              class="block text-sm font-medium text-slate-300"
            >
              Authentication Code
            </label>
            <div class="relative">
              <input
                id="mfa-token"
                v-model="mfaToken"
                type="text"
                required
                autocomplete="one-time-code"
                placeholder="000000"
                class="w-full px-4 py-3 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 text-center text-xl tracking-[0.5em] font-mono"
                maxlength="8"
                @input="handleMfaInput"
                @paste="handleMfaPaste"
              />
              <div
                class="absolute inset-y-0 right-0 pr-3 flex items-center pointer-events-none"
              >
                <svg
                  class="w-5 h-5 text-slate-400"
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
            <p class="text-xs text-slate-500 text-center">
              Enter the 6-digit code from your authenticator app or an
              8-character backup code
            </p>
          </div>

          <!-- Action Buttons -->
          <div class="flex gap-3">
            <button
              type="button"
              @click="handleBackToLogin"
              class="flex-1 py-3 px-4 border border-slate-600 rounded-lg text-sm font-medium text-slate-300 bg-slate-800 hover:bg-slate-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-slate-500 focus:ring-offset-slate-900 transition-colors"
            >
              Back
            </button>
            <button
              type="submit"
              :disabled="isLoading || !mfaToken.trim()"
              class="flex-2 py-3 px-6 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500 focus:ring-offset-slate-900 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
            >
              <svg
                v-if="isLoading"
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
              <span v-if="isLoading">Verifying...</span>
              <span v-else>Verify & Sign In</span>
            </button>
          </div>

          <!-- MFA Recovery Link -->
          <div class="text-center">
            <button
              type="button"
              @click="showMFARecoveryModal = true"
              class="text-sm text-blue-500 hover:text-blue-400 transition-colors"
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
          class="bg-red-900/50 border border-red-700 text-red-200 px-4 py-3 rounded-lg text-sm"
        >
          {{ errorMessage }}
        </div>

        <div class="flex flex-col gap-1">
          <label for="email" class="block text-sm font-medium text-slate-300"
            >Email</label
          >
          <input
            id="email"
            v-model="email"
            type="email"
            required
            autocomplete="email"
            class="mt-1 block w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
            placeholder="Enter your email"
          />
        </div>

        <div class="flex flex-col gap-1">
          <label for="password" class="block text-sm font-medium text-slate-300"
            >Password</label
          >
          <input
            id="password"
            v-model="password"
            type="password"
            required
            autocomplete="current-password"
            class="mt-1 block w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
            placeholder="Enter your password"
          />
        </div>

        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1.5">
            <input
              id="remember-me"
              v-model="rememberMe"
              type="checkbox"
              class="h-4 w-4 rounded border-slate-700 bg-slate-800 text-blue-500 focus:ring-blue-500 focus:ring-offset-slate-900"
            />
            <label for="remember-me" class="ml-2 block text-sm text-slate-300"
              >Remember me</label
            >
          </div>

          <button
            type="button"
            @click="showForgotPasswordModal = true"
            class="text-sm text-blue-500 hover:text-blue-400 transition-colors"
          >
            Forgot password?
          </button>
        </div>

        <button
          type="submit"
          :disabled="isLoading"
          class="w-full flex justify-center py-2 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500 focus:ring-offset-slate-900 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <span v-if="isLoading">Signing in...</span>
          <span v-else>Sign in</span>
        </button>

        <div class="relative flex gap-2 items-center justify-center">
          <div class="border-t border-slate-700 flex-grow"></div>
          <span class="mx-4 text-sm text-slate-500">or</span>
          <div class="border-t border-slate-700 flex-grow"></div>
        </div>

        <div class="flex gap-2">
          <button
            type="button"
            @click="handleMicrosoftLoginClick"
            class="flex-1 flex gap-1 justify-center items-center py-2 px-4 border border-slate-600 rounded-lg shadow-sm text-sm font-medium text-slate-200 bg-slate-800 hover:bg-slate-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-slate-500 focus:ring-offset-slate-900"
          >
            <svg
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
            Sign in with Microsoft Entra
          </button>

          <button
            type="button"
            @click="handleMicrosoftLogoutClick"
            title="Sign out of Microsoft account"
            class="p-2 border border-slate-600 rounded-lg text-slate-400 bg-slate-800 hover:bg-slate-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-slate-500 focus:ring-offset-slate-900"
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
