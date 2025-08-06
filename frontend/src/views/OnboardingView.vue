<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import logo from '@/assets/logo.svg';
import authService, { type AdminSetupRequest } from '@/services/authService';

const router = useRouter();
const authStore = useAuthStore();
const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

// Enhanced state management for better UX
const currentStep = ref<'setup' | 'logging-in' | 'complete'>('setup');
const autoLoginAttempted = ref(false);

// Form data
const adminData = ref<AdminSetupRequest>({
  name: '',
  email: '',
  password: ''
});

// Password confirmation for better UX
const confirmPassword = ref('');

// Computed properties for better reactivity
const isSetupStep = computed(() => currentStep.value === 'setup');
const isLoggingIn = computed(() => currentStep.value === 'logging-in');
const isComplete = computed(() => currentStep.value === 'complete');
const canSubmit = computed(() => !isLoading.value && validateFormComputed.value);

// Check if setup is actually required on mount
onMounted(async () => {
  try {
    const status = await authService.checkSetupStatus();
    if (!status.requires_setup) {
      // Setup already completed, redirect to login
      router.push('/login');
    }
  } catch (error) {
    console.error('Error checking setup status:', error);
    errorMessage.value = 'Failed to verify setup status. Please try again.';
  }
});

// Computed validation for better reactivity
const validateFormComputed = computed((): boolean => {
  if (!adminData.value.name.trim()) return false;
  if (!adminData.value.email.trim()) return false;
  
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!emailRegex.test(adminData.value.email)) return false;
  
  if (adminData.value.password.length < 8) return false;
  if (adminData.value.password !== confirmPassword.value) return false;
  
  return true;
});

const validateForm = (): boolean => {
  if (!adminData.value.name.trim()) {
    errorMessage.value = 'Administrator name is required';
    return false;
  }

  if (!adminData.value.email.trim()) {
    errorMessage.value = 'Email address is required';
    return false;
  }

  // Basic email validation
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!emailRegex.test(adminData.value.email)) {
    errorMessage.value = 'Please enter a valid email address';
    return false;
  }

  if (adminData.value.password.length < 8) {
    errorMessage.value = 'Password must be at least 8 characters long';
    return false;
  }

  if (adminData.value.password !== confirmPassword.value) {
    errorMessage.value = 'Passwords do not match';
    return false;
  }

  return true;
};

const handleSetup = async () => {
  // Prevent double submission
  if (isLoading.value || autoLoginAttempted.value) return;
  
  errorMessage.value = '';
  successMessage.value = '';

  if (!validateForm()) {
    return;
  }

  isLoading.value = true;
  currentStep.value = 'setup';

  try {
    const response = await authService.setupInitialAdmin(adminData.value);
    
    if (response.success) {
      // Clear the setup status cache since setup is now complete
      authService.clearSetupStatusCache();
      
      // Move to logging in step for better UX
      currentStep.value = 'logging-in';
      successMessage.value = 'Admin account created successfully! Logging you in...';
      autoLoginAttempted.value = true;
      
      // Small delay for better UX (let user see the success message)
      await new Promise(resolve => setTimeout(resolve, 1000));
      
      // Attempt automatic login with credentials
      const loginSuccess = await attemptAutoLogin();
      
      if (loginSuccess) {
        currentStep.value = 'complete';
        // Auth store will handle the redirect automatically
      } else {
        // Fallback to manual login page
        handleLoginFallback();
      }
    } else {
      errorMessage.value = response.message || 'Setup failed. Please try again.';
      currentStep.value = 'setup';
    }
  } catch (error: any) {
    console.error('Setup error:', error);
    currentStep.value = 'setup';
    
    if (error.response?.data?.message) {
      errorMessage.value = error.response.data.message;
    } else if (error.response?.data?.status === 'error') {
      errorMessage.value = error.response.data.message || 'Setup failed. Please try again.';
    } else {
      errorMessage.value = 'An unexpected error occurred. Please try again.';
    }
  } finally {
    isLoading.value = false;
  }
};

const attemptAutoLogin = async (): Promise<boolean> => {
  try {
    // Use the auth store's login method for consistency
    const loginSuccess = await authStore.login({
      email: adminData.value.email,
      password: adminData.value.password
    });
    
    if (loginSuccess) {
      successMessage.value = 'Successfully logged in! Redirecting...';
      return true;
    } else {
      // Check if MFA is required
      if (authStore.mfaRequired) {
        successMessage.value = 'Account created! MFA verification required.';
        // Redirect to login with MFA state
        setTimeout(() => {
          router.push({
            path: '/login',
            query: { 
              message: 'Account created successfully. Please complete MFA verification.',
              email: adminData.value.email 
            }
          });
        }, 2000);
        return true;
      } else if (authStore.mfaSetupRequired) {
        successMessage.value = 'Account created successfully! Setting up multi-factor authentication...';
        // Redirect to MFA setup with proper credentials storage
        setTimeout(() => {
          console.log('🔄 Onboarding: MFA setup required, storing credentials');
          
          // Store credentials in sessionStorage for MFA setup
          sessionStorage.setItem('mfaSetupCredentials', JSON.stringify({
            email: adminData.value.email,
            password: adminData.value.password,
            from: 'onboarding',
            timestamp: Date.now()
          }));
          
          console.log('🔄 Onboarding: Credentials stored for MFA setup');
          
          router.push({ name: 'mfa-setup' });
        }, 1500);
        return true;
      }
    }
    
    return false;
  } catch (error) {
    console.error('Auto-login error:', error);
    return false;
  }
};

const handleLoginFallback = () => {
  errorMessage.value = '';
  successMessage.value = 'Account created successfully! Please log in with your credentials.';
  
  setTimeout(() => {
    router.push({
      path: '/login',
      query: { 
        message: 'Account created successfully. Please log in.',
        email: adminData.value.email 
      }
    });
  }, 2500);
};

// Security: Clear sensitive data when component is unmounted
const clearSensitiveData = () => {
  adminData.value.password = '';
  confirmPassword.value = '';
};

// Vue 3 best practice: Cleanup on unmount
import { onUnmounted } from 'vue';
onUnmounted(() => {
  clearSensitiveData();
});
</script>

<template>
  <div class="min-h-screen w-full flex items-center justify-center bg-slate-900">
    <div class="flex flex-col gap-6 w-full max-w-lg p-8">
      <!-- Logo/Brand -->
      <div class="flex flex-col gap-2 items-center">
        <img :src="logo" alt="Nosdesk Logo" class="px-4" />
        <h1 class="text-2xl font-bold text-white mt-4">Welcome to Nosdesk</h1>
        <p class="text-slate-400 text-center">
          Let's get started by creating your administrator account
        </p>
      </div>

      <!-- Success Message -->
      <div v-if="successMessage" class="bg-green-900/50 border border-green-700 text-green-200 px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-2">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
          </svg>
          {{ successMessage }}
        </div>
      </div>

      <!-- Error Message -->
      <div v-if="errorMessage" class="bg-red-900/50 border border-red-700 text-red-200 px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-2">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.996-.833-2.768 0L3.232 16.5c-.77.833.192 2.5 1.732 2.5z"></path>
          </svg>
          {{ errorMessage }}
        </div>
      </div>

      <!-- Setup Form -->
      <form v-if="isSetupStep" @submit.prevent="handleSetup" class="flex flex-col gap-4">
        <div>
          <label for="admin-name" class="block text-sm font-medium text-slate-300">Administrator Name</label>
          <input
            id="admin-name"
            v-model="adminData.name"
            type="text"
            required
            autocomplete="name"
            :disabled="isLoading"
            class="mt-1 block w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:opacity-50 transition-colors"
            placeholder="Enter your full name"
          />
        </div>

        <div>
          <label for="admin-email" class="block text-sm font-medium text-slate-300">Email Address</label>
          <input
            id="admin-email"
            v-model="adminData.email"
            type="email"
            required
            autocomplete="email"
            :disabled="isLoading"
            class="mt-1 block w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:opacity-50 transition-colors"
            placeholder="Enter your email address"
          />
        </div>

        <div>
          <label for="admin-password" class="block text-sm font-medium text-slate-300">Password</label>
          <input
            id="admin-password"
            v-model="adminData.password"
            type="password"
            required
            autocomplete="new-password"
            :disabled="isLoading"
            class="mt-1 block w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:opacity-50 transition-colors"
            placeholder="Choose a secure password (8+ characters)"
          />
        </div>

        <div>
          <label for="confirm-password" class="block text-sm font-medium text-slate-300">Confirm Password</label>
          <input
            id="confirm-password"
            v-model="confirmPassword"
            type="password"
            required
            autocomplete="new-password"
            :disabled="isLoading"
            class="mt-1 block w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:opacity-50 transition-colors"
            placeholder="Confirm your password"
          />
        </div>

        <div class="pt-2">
          <button
            type="submit"
            :disabled="!canSubmit"
            class="w-full flex justify-center py-3 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500 focus:ring-offset-slate-900 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            <span v-if="isLoading" class="flex items-center gap-2">
              <svg class="animate-spin -ml-1 mr-2 h-4 w-4 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              Creating Administrator...
            </span>
            <span v-else>Create Administrator Account</span>
          </button>
        </div>
      </form>

      <!-- Auto-login Progress -->
      <div v-else-if="isLoggingIn" class="flex flex-col items-center gap-6 text-center">
        <div class="flex items-center justify-center w-16 h-16 bg-blue-600/10 rounded-full">
          <svg class="animate-spin w-8 h-8 text-blue-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
        <div class="flex flex-col gap-2">
          <h3 class="text-lg font-semibold text-white">Setting up your account</h3>
          <p class="text-slate-400">This will only take a moment...</p>
        </div>
      </div>

      <!-- Completion State -->
      <div v-else-if="isComplete" class="flex flex-col items-center gap-6 text-center">
        <div class="flex items-center justify-center w-16 h-16 bg-green-600/10 rounded-full">
          <svg class="w-8 h-8 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
          </svg>
        </div>
        <div class="flex flex-col gap-2">
          <h3 class="text-lg font-semibold text-white">Welcome to Nosdesk!</h3>
          <p class="text-slate-400">Your administrator account has been created successfully.</p>
        </div>
      </div>

      <!-- Security Notice -->
      <div class="bg-slate-800 border border-slate-700 rounded-lg p-4 text-sm text-slate-300">
        <div class="flex items-start gap-3">
          <svg class="w-5 h-5 text-blue-400 mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 0h12a2 2 0 002-2v-9a2 2 0 00-2-2H6a2 2 0 00-2 2v9a2 2 0 002 2zm10-12V6a4 4 0 00-8 0v3h8z"></path>
          </svg>
          <div>
            <h4 class="font-medium text-white mb-1">Security Notice</h4>
            <p class="text-xs text-slate-400">
              This will create the first administrator account for your Nosdesk installation. 
              Choose a strong password as this account will have full system access.
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template> 