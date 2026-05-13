<script setup lang="ts">
import { ref, onMounted, computed, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useAutoLogin } from '@/composables/useAutoLogin';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import authService, {
  type AdminSetupRequest,
} from '@/services/authService';

const router = useRouter();

// Auto-login composable. The onboarding view drives its own step
// state, so only `attemptLogin` is consumed here.
const { attemptLogin } = useAutoLogin({ source: 'onboarding' });

const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

const currentStep = ref<'setup' | 'logging-in' | 'complete'>('setup');
const autoLoginAttempted = ref(false);

const adminData = ref<AdminSetupRequest>({
  name: '',
  email: '',
  password: '',
});
const confirmPassword = ref('');
const bootstrapToken = ref('');
// True when the token arrived via `?token=` in the URL (the
// server-logged setup URL flow). The manual-paste UI hides in
// that case to keep the form clean. If verification later fails,
// we flip this back to false so the operator can correct.
const tokenFromUrl = ref(false);

const isSetupStep = computed(() => currentStep.value === 'setup');
const isLoggingIn = computed(() => currentStep.value === 'logging-in');
const isComplete = computed(() => currentStep.value === 'complete');

const validateForm = (): boolean => {
  if (!bootstrapToken.value.trim()) {
    errorMessage.value = 'Bootstrap token is required';
    return false;
  }
  if (!adminData.value.name.trim()) {
    errorMessage.value = 'Administrator name is required';
    return false;
  }
  if (!adminData.value.email.trim()) {
    errorMessage.value = 'Email address is required';
    return false;
  }
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

const validateFormComputed = computed((): boolean => {
  if (!bootstrapToken.value.trim()) return false;
  if (!adminData.value.name.trim()) return false;
  if (!adminData.value.email.trim()) return false;
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!emailRegex.test(adminData.value.email)) return false;
  if (adminData.value.password.length < 8) return false;
  if (adminData.value.password !== confirmPassword.value) return false;
  return true;
});

const canSubmit = computed(() => !isLoading.value && validateFormComputed.value);

onMounted(async () => {
  // Pick up the bootstrap token if the operator arrived via the
  // setup URL logged at server startup. We strip the token from
  // the URL bar before the form ever renders, so it doesn't get
  // captured in browser history, the Referer header on the
  // eventual POST, or shoulder-surfing screenshots.
  const params = new URLSearchParams(window.location.search);
  const tokenParam = params.get('token');
  if (tokenParam && tokenParam.trim()) {
    bootstrapToken.value = tokenParam.trim();
    tokenFromUrl.value = true;
    params.delete('token');
    const cleanQuery = params.toString();
    const cleanUrl = `${window.location.pathname}${cleanQuery ? '?' + cleanQuery : ''}${window.location.hash}`;
    window.history.replaceState({}, '', cleanUrl);
  }

  try {
    const status = await authService.checkSetupStatus();
    if (!status.requires_setup) {
      router.push('/login');
    }
  } catch (error) {
    console.error('Error checking setup status:', error);
    errorMessage.value = 'Failed to verify setup status. Please try again.';
  }
});

const clearSensitiveData = () => {
  adminData.value.password = '';
  confirmPassword.value = '';
  bootstrapToken.value = '';
};

const handleLoginFallback = () => {
  errorMessage.value = '';
  successMessage.value = 'Account created successfully. Please log in with your credentials.';
  setTimeout(() => {
    router.push({
      path: '/login',
      query: {
        message: 'Account created successfully. Please log in.',
        email: adminData.value.email,
      },
    });
  }, 2500);
};

const handleSetup = async () => {
  if (isLoading.value || autoLoginAttempted.value) return;

  errorMessage.value = '';
  successMessage.value = '';

  if (!validateForm()) {
    return;
  }

  isLoading.value = true;
  currentStep.value = 'setup';

  try {
    const response = await authService.setupInitialAdmin(adminData.value, bootstrapToken.value.trim());

    if (response.success) {
      authService.clearSetupStatusCache();
      currentStep.value = 'logging-in';
      successMessage.value = 'Admin account created. Logging you in...';
      autoLoginAttempted.value = true;

      const loginSuccess = await attemptLogin(adminData.value.email, adminData.value.password);

      if (loginSuccess) {
        currentStep.value = 'complete';
        clearSensitiveData();
      } else {
        handleLoginFallback();
      }
    } else {
      errorMessage.value = response.message || 'Setup failed. Please try again.';
      currentStep.value = 'setup';
    }
  } catch (error) {
    console.error('Setup error:', error);
    currentStep.value = 'setup';

    const axiosError = error as { response?: { status?: number; data?: { message?: string; status?: string } } };
    if (axiosError.response?.data?.message) {
      errorMessage.value = axiosError.response.data.message;
    } else if (axiosError.response?.data?.status === 'error') {
      errorMessage.value = axiosError.response.data.message || 'Setup failed. Please try again.';
    } else {
      errorMessage.value = 'An unexpected error occurred. Please try again.';
    }
    // 401 from this endpoint means the bootstrap token was
    // rejected (missing, mismatched, or expired). Surface the
    // manual paste field so the operator can correct rather than
    // staring at "click the URL from logs" when their URL is the
    // problem.
    if (axiosError.response?.status === 401 && tokenFromUrl.value) {
      tokenFromUrl.value = false;
      bootstrapToken.value = '';
    }
  } finally {
    isLoading.value = false;
  }
};

onUnmounted(() => {
  clearSensitiveData();
});
</script>

<template>
  <div class="min-h-screen w-full flex flex-col items-center justify-center bg-app py-8">
    <div class="flex flex-col gap-6 w-full max-w-lg px-8">
      <!-- Logo / Brand -->
      <div class="flex flex-col gap-2 items-center">
        <LogoIcon class="h-12 px-4 text-accent" aria-label="Nosdesk Logo" />
        <h1 class="text-2xl font-bold text-primary mt-4">Welcome to Nosdesk</h1>
        <p class="text-secondary text-center">
          Let's get started by creating your administrator account
        </p>
      </div>

      <!-- Success Message -->
      <div v-if="successMessage" class="bg-status-success/20 border border-status-success/50 text-status-success px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-2">
          <Icon name="check" size="md" />
          {{ successMessage }}
        </div>
      </div>

      <!-- Error Message -->
      <div v-if="errorMessage" class="bg-status-error/20 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-2">
          <Icon name="warning" size="md" />
          {{ errorMessage }}
        </div>
      </div>

      <!-- Setup Form -->
      <form v-if="isSetupStep" @submit.prevent="handleSetup" class="flex flex-col gap-4">
        <!--
          Bootstrap token field. Hidden on the happy path where
          the operator arrived via the setup URL from server
          logs (the token is already in our state and stripped
          from the address bar). Visible only when no URL token
          was supplied, or when verification just rejected one
          we did supply.
        -->
        <div v-if="!tokenFromUrl">
          <label for="bootstrap-token" class="block text-sm font-medium text-secondary">Bootstrap Token</label>
          <input
            id="bootstrap-token"
            v-model="bootstrapToken"
            type="password"
            required
            autocomplete="off"
            :disabled="isLoading"
            class="mt-1 block w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent disabled:opacity-50 transition-colors font-mono text-sm"
            placeholder="Paste the one-shot token from the server"
          />
          <p class="text-xs text-tertiary mt-1">
            Check the server startup logs for a setup URL, or retrieve manually with
            <code class="text-secondary bg-app rounded px-1 py-0.5">docker compose exec backend cat /app/uploads/bootstrap.token</code>
          </p>
        </div>

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

        <div class="pt-2">
          <button
            type="submit"
            :disabled="!canSubmit"
            class="w-full flex justify-center py-3 px-4 border border-transparent rounded-lg shadow-sm text-sm font-medium text-white bg-accent hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent focus:ring-offset-slate-900 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            <span v-if="isLoading" class="flex items-center gap-2">
              <Spinner class="-ml-1 mr-2 text-white" />
              Creating Administrator...
            </span>
            <span v-else>Create Administrator Account</span>
          </button>
        </div>
      </form>

      <!-- Auto-login Progress -->
      <div v-else-if="isLoggingIn" class="flex flex-col items-center gap-6 text-center">
        <div class="flex items-center justify-center w-16 h-16 bg-accent/10 rounded-full">
          <Spinner size="lg" class="text-accent" />
        </div>
        <div class="flex flex-col gap-2">
          <h3 class="text-lg font-semibold text-primary">Setting up your account</h3>
          <p class="text-secondary">This will only take a moment...</p>
        </div>
      </div>

      <!-- Completion State -->
      <div v-else-if="isComplete" class="flex flex-col items-center gap-6 text-center">
        <div class="flex items-center justify-center w-16 h-16 bg-status-success/10 rounded-full">
          <Icon name="check" size="lg" class="text-status-success" />
        </div>
        <div class="flex flex-col gap-2">
          <h3 class="text-lg font-semibold text-primary">Welcome to Nosdesk</h3>
          <p class="text-secondary">Your administrator account is ready.</p>
        </div>
      </div>

      <!-- Migration / Restore Hint -->
      <div v-if="isSetupStep" class="bg-surface border border-default rounded-lg p-3 sm:p-4 text-sm text-secondary">
        <div class="flex flex-row items-start gap-3">
          <Icon name="info" size="md" class="text-tertiary mt-0.5 flex-shrink-0" />
          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-primary mb-1 text-sm">Migrating from another Nosdesk instance?</h4>
            <p class="text-xs text-tertiary">
              Create an admin here, then run
              <code class="text-primary bg-app rounded px-1 py-0.5">docker compose exec backend nosdesk-cli db restore /path/to/backup.zip</code>
              on the host. The restore replaces the admin with the
              imported users.
            </p>
          </div>
        </div>
      </div>

      <!-- Security Notice -->
      <div v-if="isSetupStep" class="bg-surface border border-default rounded-lg p-3 sm:p-4 text-sm text-secondary">
        <div class="flex flex-row items-start gap-3">
          <svg class="w-5 h-5 text-accent mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
          </svg>
          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-primary mb-1 text-sm">Security Notice</h4>
            <p class="text-xs text-tertiary">
              This will create the first administrator account for your
              Nosdesk installation. Choose a strong password; this account
              will have full system access.
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
