<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useAutoLogin } from '@/composables/useAutoLogin';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import authService, {
  type AdminSetupRequest,
  type OnboardingRestoreUploadResponse,
  type OnboardingRestoreResult
} from '@/services/authService';

const router = useRouter();

// Auto-login composable (OnboardingView manages its own step state, so we only use attemptLogin)
const { attemptLogin } = useAutoLogin({ source: 'onboarding' });

const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

// Main mode: choose between setup and restore
const mode = ref<'choose' | 'setup' | 'restore'>('choose');

// Enhanced state management for better UX
const currentStep = ref<'setup' | 'logging-in' | 'complete'>('setup');
const autoLoginAttempted = ref(false);

// Restore state
const restoreStep = ref<'upload' | 'preview' | 'password' | 'restoring' | 'complete'>('upload');
const restoreFile = ref<File | null>(null);
const restoreUploadResponse = ref<OnboardingRestoreUploadResponse | null>(null);
const restorePassword = ref('');
const restoreResult = ref<OnboardingRestoreResult | null>(null);
const isDragging = ref(false);

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

      // Attempt automatic login using composable
      const loginSuccess = await attemptLogin(adminData.value.email, adminData.value.password);

      if (loginSuccess) {
        currentStep.value = 'complete';
        // Clear sensitive data
        clearSensitiveData();
      } else {
        // Fallback to manual login page
        handleLoginFallback();
      }
    } else {
      errorMessage.value = response.message || 'Setup failed. Please try again.';
      currentStep.value = 'setup';
    }
  } catch (error) {
    console.error('Setup error:', error);
    currentStep.value = 'setup';

    const axiosError = error as { response?: { data?: { message?: string; status?: string } } };
    if (axiosError.response?.data?.message) {
      errorMessage.value = axiosError.response.data.message;
    } else if (axiosError.response?.data?.status === 'error') {
      errorMessage.value = axiosError.response.data.message || 'Setup failed. Please try again.';
    } else {
      errorMessage.value = 'An unexpected error occurred. Please try again.';
    }
  } finally {
    isLoading.value = false;
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

// Restore functionality
const handleFileSelect = (event: Event) => {
  const input = event.target as HTMLInputElement;
  if (input.files && input.files[0]) {
    restoreFile.value = input.files[0];
    uploadBackupFile();
  }
};

const handleDrop = (event: DragEvent) => {
  isDragging.value = false;
  event.preventDefault();

  if (event.dataTransfer?.files && event.dataTransfer.files[0]) {
    restoreFile.value = event.dataTransfer.files[0];
    uploadBackupFile();
  }
};

const handleDragOver = (event: DragEvent) => {
  event.preventDefault();
  isDragging.value = true;
};

const handleDragLeave = () => {
  isDragging.value = false;
};

const uploadBackupFile = async () => {
  if (!restoreFile.value) return;

  isLoading.value = true;
  errorMessage.value = '';

  try {
    restoreUploadResponse.value = await authService.uploadRestoreBackup(restoreFile.value);

    // Check if password is needed
    if (restoreUploadResponse.value.preview.has_encrypted_sensitive) {
      restoreStep.value = 'password';
    } else {
      restoreStep.value = 'preview';
    }
  } catch (error) {
    console.error('Upload error:', error);
    const axiosError = error as { response?: { data?: { error?: string } } };
    errorMessage.value = axiosError.response?.data?.error || 'Failed to upload backup file';
    restoreFile.value = null;
  } finally {
    isLoading.value = false;
  }
};

const executeRestore = async () => {
  if (!restoreUploadResponse.value) return;

  isLoading.value = true;
  errorMessage.value = '';
  restoreStep.value = 'restoring';

  try {
    const password = restoreUploadResponse.value.preview.has_encrypted_sensitive
      ? restorePassword.value
      : undefined;

    restoreResult.value = await authService.executeRestore(
      restoreUploadResponse.value.file_path,
      password
    );

    if (restoreResult.value.success) {
      restoreStep.value = 'complete';
      // Clear setup status cache
      authService.clearSetupStatusCache();
    } else {
      errorMessage.value = restoreResult.value.message || 'Restore failed';
      restoreStep.value = 'preview';
    }
  } catch (error) {
    console.error('Restore error:', error);
    const axiosError = error as { response?: { data?: { error?: string } } };
    errorMessage.value = axiosError.response?.data?.error || 'Failed to restore backup';
    restoreStep.value = 'preview';
  } finally {
    isLoading.value = false;
  }
};

const formatFileSize = (bytes: number): string => {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
};

const goBackToChoice = () => {
  mode.value = 'choose';
  restoreStep.value = 'upload';
  restoreFile.value = null;
  restoreUploadResponse.value = null;
  restorePassword.value = '';
  errorMessage.value = '';
  successMessage.value = '';
};

const goToLogin = () => {
  router.push('/login');
};

// Security: Clear sensitive data when component is unmounted
const clearSensitiveData = () => {
  adminData.value.password = '';
  confirmPassword.value = '';
  restorePassword.value = '';
};

// Vue 3 best practice: Cleanup on unmount
import { onUnmounted } from 'vue';
onUnmounted(() => {
  clearSensitiveData();
});
</script>

<template>
  <div class="min-h-screen w-full flex flex-col items-center justify-center bg-app py-8">
    <div class="flex flex-col gap-6 w-full max-w-lg px-8">
      <!-- Logo/Brand -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-8 px-4 text-accent" aria-label="Nosdesk Logo" />
        <h1 class="text-2xl font-bold text-primary mt-4">Welcome to Nosdesk</h1>
        <p v-if="mode === 'choose'" class="text-secondary text-center">
          Choose how you'd like to get started
        </p>
        <p v-else-if="mode === 'setup'" class="text-secondary text-center">
          Let's get started by creating your administrator account
        </p>
        <p v-else-if="mode === 'restore'" class="text-secondary text-center">
          Restore your system from a previous backup
        </p>
      </div>

      <!-- Success Message -->
      <div v-if="successMessage" class="bg-status-success/20 border border-status-success/50 text-status-success px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-2">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
          </svg>
          {{ successMessage }}
        </div>
      </div>

      <!-- Error Message -->
      <div v-if="errorMessage" class="bg-status-error/20 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-2">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.996-.833-2.768 0L3.232 16.5c-.77.833.192 2.5 1.732 2.5z"></path>
          </svg>
          {{ errorMessage }}
        </div>
      </div>

      <!-- Mode Choice -->
      <div v-if="mode === 'choose'" class="flex flex-col gap-4">
        <button
          @click="mode = 'setup'"
          class="flex flex-row items-center gap-3 sm:gap-4 p-3 sm:p-4 bg-surface border border-default rounded-lg hover:bg-surface-hover hover:border-accent transition-colors text-left w-full"
        >
          <div class="flex-shrink-0 h-10 w-10 sm:h-12 sm:w-12 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg class="w-5 h-5 sm:w-6 sm:h-6 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 20a6 6 0 0112 0v1H3v-1z" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="text-primary font-medium text-sm sm:text-base">Create New Installation</h3>
            <p class="text-xs sm:text-sm text-secondary truncate sm:whitespace-normal">Set up a fresh Nosdesk instance with a new administrator account</p>
          </div>
          <svg class="flex-shrink-0 w-5 h-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>

        <button
          @click="mode = 'restore'"
          class="flex flex-row items-center gap-3 sm:gap-4 p-3 sm:p-4 bg-surface border border-default rounded-lg hover:bg-surface-hover hover:border-accent transition-colors text-left w-full"
        >
          <div class="flex-shrink-0 h-10 w-10 sm:h-12 sm:w-12 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg class="w-5 h-5 sm:w-6 sm:h-6 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0L8 8m4-4v12" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <h3 class="text-primary font-medium text-sm sm:text-base">Restore from Backup</h3>
            <p class="text-xs sm:text-sm text-secondary truncate sm:whitespace-normal">Import data from a previous Nosdesk backup file</p>
          </div>
          <svg class="flex-shrink-0 w-5 h-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>
      </div>

      <!-- Setup Form -->
      <template v-if="mode === 'setup'">
        <form v-if="isSetupStep" @submit.prevent="handleSetup" class="flex flex-col gap-4">
          <div>
            <label for="admin-name" class="block text-sm font-medium text-secondary">Administrator Name</label>
            <input
              id="admin-name"
              v-model="adminData.name"
              type="text"
              required
              autocomplete="name"
              :disabled="isLoading"
              class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 transition-colors"
              placeholder="Enter your full name"
            />
          </div>

          <div>
            <label for="admin-email" class="block text-sm font-medium text-secondary">Email Address</label>
            <input
              id="admin-email"
              v-model="adminData.email"
              type="email"
              required
              autocomplete="email"
              :disabled="isLoading"
              class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 transition-colors"
              placeholder="Enter your email address"
            />
          </div>

          <div>
            <label for="admin-password" class="block text-sm font-medium text-secondary">Password</label>
            <input
              id="admin-password"
              v-model="adminData.password"
              type="password"
              required
              autocomplete="new-password"
              :disabled="isLoading"
              class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 transition-colors"
              placeholder="Choose a secure password (8+ characters)"
            />
          </div>

          <div>
            <label for="confirm-password" class="block text-sm font-medium text-secondary">Confirm Password</label>
            <input
              id="confirm-password"
              v-model="confirmPassword"
              type="password"
              required
              autocomplete="new-password"
              :disabled="isLoading"
              class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 transition-colors"
              placeholder="Confirm your password"
            />
          </div>

          <div class="pt-2 flex gap-3">
            <button
              type="button"
              @click="goBackToChoice"
              class="px-4 py-3 border border-default rounded-lg text-sm font-medium text-secondary hover:bg-surface-hover transition-colors"
            >
              Back
            </button>
            <button
              type="submit"
              :disabled="!canSubmit"
              class="flex-1 flex justify-center py-3 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent focus:ring-offset-slate-900 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
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
          <div class="flex items-center justify-center w-16 h-16 bg-accent/10 rounded-full">
            <svg class="animate-spin w-8 h-8 text-accent" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
          </div>
          <div class="flex flex-col gap-2">
            <h3 class="text-lg font-semibold text-primary">Setting up your account</h3>
            <p class="text-secondary">This will only take a moment...</p>
          </div>
        </div>

        <!-- Completion State -->
        <div v-else-if="isComplete" class="flex flex-col items-center gap-6 text-center">
          <div class="flex items-center justify-center w-16 h-16 bg-status-success/10 rounded-full">
            <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
            </svg>
          </div>
          <div class="flex flex-col gap-2">
            <h3 class="text-lg font-semibold text-primary">Welcome to Nosdesk!</h3>
            <p class="text-secondary">Your administrator account has been created successfully.</p>
          </div>
        </div>
      </template>

      <!-- Restore Flow -->
      <template v-if="mode === 'restore'">
        <!-- Upload Step -->
        <div v-if="restoreStep === 'upload'" class="flex flex-col gap-4">
          <div
            @drop="handleDrop"
            @dragover="handleDragOver"
            @dragleave="handleDragLeave"
            :class="[
              'border-2 border-dashed rounded-lg p-8 text-center transition-colors cursor-pointer',
              isDragging ? 'border-accent bg-accent/10' : 'border-default hover:border-accent hover:bg-surface-hover'
            ]"
            @click="($refs.fileInput as HTMLInputElement).click()"
          >
            <input
              ref="fileInput"
              type="file"
              accept=".zip"
              class="hidden"
              @change="handleFileSelect"
            />
            <svg class="w-12 h-12 mx-auto text-tertiary mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
            </svg>
            <p class="text-primary font-medium mb-1">Drop your backup file here</p>
            <p class="text-sm text-secondary">or click to browse for a .zip backup file</p>
          </div>

          <div class="flex gap-3">
            <button
              type="button"
              @click="goBackToChoice"
              class="px-4 py-3 border border-default rounded-lg text-sm font-medium text-secondary hover:bg-surface-hover transition-colors"
            >
              Back
            </button>
          </div>
        </div>

        <!-- Password Step (for encrypted backups) -->
        <div v-else-if="restoreStep === 'password'" class="flex flex-col gap-4">
          <div class="bg-surface border border-default rounded-lg p-3 sm:p-4">
            <div class="flex flex-row items-start gap-3">
              <svg class="w-5 h-5 text-accent mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 0h12a2 2 0 002-2v-9a2 2 0 00-2-2H6a2 2 0 00-2 2v9a2 2 0 002 2zm10-12V6a4 4 0 00-8 0v3h8z"></path>
              </svg>
              <div class="flex-1 min-w-0">
                <h4 class="font-medium text-primary mb-1 text-sm">Encrypted Backup</h4>
                <p class="text-xs text-tertiary">
                  This backup contains encrypted sensitive data. Enter the password used when creating the backup.
                </p>
              </div>
            </div>
          </div>

          <div>
            <label for="restore-password" class="block text-sm font-medium text-secondary">Backup Password</label>
            <input
              id="restore-password"
              v-model="restorePassword"
              type="password"
              required
              :disabled="isLoading"
              class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 transition-colors"
              placeholder="Enter the backup encryption password"
            />
          </div>

          <div class="flex gap-3">
            <button
              type="button"
              @click="restoreStep = 'upload'; restoreFile = null; restoreUploadResponse = null"
              class="px-4 py-3 border border-default rounded-lg text-sm font-medium text-secondary hover:bg-surface-hover transition-colors"
            >
              Back
            </button>
            <button
              @click="restoreStep = 'preview'"
              :disabled="!restorePassword"
              class="flex-1 py-3 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              Continue
            </button>
          </div>
        </div>

        <!-- Preview Step -->
        <div v-else-if="restoreStep === 'preview' && restoreUploadResponse" class="flex flex-col gap-4">
          <div class="bg-surface border border-default rounded-lg divide-y divide-default">
            <div class="p-4">
              <h3 class="font-medium text-primary mb-2">Backup Information</h3>
              <dl class="grid grid-cols-2 gap-2 text-sm">
                <dt class="text-secondary">Created:</dt>
                <dd class="text-primary">{{ new Date(restoreUploadResponse.preview.manifest.created_at).toLocaleString() }}</dd>
                <dt class="text-secondary">Version:</dt>
                <dd class="text-primary">{{ restoreUploadResponse.preview.manifest.nosdesk_version }}</dd>
                <dt class="text-secondary">Files:</dt>
                <dd class="text-primary">{{ restoreUploadResponse.preview.manifest.files.total_count }} ({{ formatFileSize(restoreUploadResponse.preview.manifest.files.total_size_bytes) }})</dd>
              </dl>
            </div>

            <div class="p-4">
              <h3 class="font-medium text-primary mb-2">Data to Restore</h3>
              <div class="flex flex-wrap gap-2">
                <span
                  v-for="(info, table) in restoreUploadResponse.preview.manifest.tables"
                  :key="table"
                  class="inline-flex items-center gap-1 px-2 py-1 bg-accent/10 text-accent rounded text-xs"
                >
                  {{ table }} <span class="text-accent/60">({{ info.count }})</span>
                </span>
              </div>
            </div>

            <div v-if="restoreUploadResponse.preview.warnings.length > 0" class="p-4">
              <h3 class="font-medium text-status-warning mb-2">Warnings</h3>
              <ul class="text-sm text-status-warning space-y-1">
                <li v-for="warning in restoreUploadResponse.preview.warnings" :key="warning" class="flex items-start gap-2">
                  <svg class="w-4 h-4 mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.996-.833-2.768 0L3.232 16.5c-.77.833.192 2.5 1.732 2.5z" />
                  </svg>
                  {{ warning }}
                </li>
              </ul>
            </div>
          </div>

          <div class="flex gap-3">
            <button
              type="button"
              @click="goBackToChoice"
              class="px-4 py-3 border border-default rounded-lg text-sm font-medium text-secondary hover:bg-surface-hover transition-colors"
            >
              Cancel
            </button>
            <button
              @click="executeRestore"
              :disabled="isLoading"
              class="flex-1 py-3 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              Restore System
            </button>
          </div>
        </div>

        <!-- Restoring Step -->
        <div v-else-if="restoreStep === 'restoring'" class="flex flex-col items-center gap-6 text-center">
          <div class="flex items-center justify-center w-16 h-16 bg-accent/10 rounded-full">
            <svg class="animate-spin w-8 h-8 text-accent" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
          </div>
          <div class="flex flex-col gap-2">
            <h3 class="text-lg font-semibold text-primary">Restoring your system</h3>
            <p class="text-secondary">This may take a few moments...</p>
          </div>
        </div>

        <!-- Restore Complete -->
        <div v-else-if="restoreStep === 'complete' && restoreResult" class="flex flex-col items-center gap-6 text-center">
          <div class="flex items-center justify-center w-16 h-16 bg-status-success/10 rounded-full">
            <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
            </svg>
          </div>
          <div class="flex flex-col gap-2">
            <h3 class="text-lg font-semibold text-primary">System Restored!</h3>
            <p class="text-secondary">{{ restoreResult.message }}</p>
            <div class="text-sm text-tertiary mt-2">
              <p>{{ restoreResult.tables_restored }} tables restored</p>
              <p>{{ restoreResult.records_restored }} records imported</p>
              <p>{{ restoreResult.files_restored }} files restored</p>
            </div>
          </div>
          <button
            @click="goToLogin"
            class="w-full py-3 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover transition-colors"
          >
            Continue to Login
          </button>
        </div>
      </template>

      <!-- Security Notice (only show on choice and setup) -->
      <div v-if="mode === 'choose' || (mode === 'setup' && isSetupStep)" class="bg-surface border border-default rounded-lg p-3 sm:p-4 text-sm text-secondary">
        <div class="flex flex-row items-start gap-3">
          <svg class="w-5 h-5 text-accent mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 0h12a2 2 0 002-2v-9a2 2 0 00-2-2H6a2 2 0 00-2 2v9a2 2 0 002 2zm10-12V6a4 4 0 00-8 0v3h8z"></path>
          </svg>
          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-primary mb-1 text-sm">Security Notice</h4>
            <p class="text-xs text-tertiary">
              This will create the first administrator account for your Nosdesk installation.
              Choose a strong password as this account will have full system access.
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
