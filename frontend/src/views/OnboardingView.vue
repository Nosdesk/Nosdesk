<script setup lang="ts">
import { ref, onMounted, computed, onUnmounted, nextTick, watch } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useAutoLogin } from '@/composables/useAutoLogin';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import AuthLayout from '@/components/auth/AuthLayout.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import PasswordInput from '@/components/common/PasswordInput.vue';
import CodeBlock from '@/components/common/CodeBlock.vue';
import authService, {
  type AdminSetupRequest,
} from '@nosdesk/core/services/authService';
import { useThemeStore } from '@/stores/theme';
import { initialsFrom } from '@/utils/monogram';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

// Maps the backend's machine-readable `code` (see
// handlers::auth::setup_initial_admin) to a localised Fluent message,
// so onboarding errors render in the operator's language instead of the
// raw English server string.
const ERROR_CODE_KEYS: Record<string, string> = {
  BOOTSTRAP_TOKEN_REQUIRED: 'onboarding-error-token-required',
  BOOTSTRAP_TOKEN_EXPIRED: 'onboarding-error-token-expired',
  BOOTSTRAP_TOKEN_MISMATCH: 'onboarding-error-token-mismatch',
  BOOTSTRAP_TOKEN_NOT_PRESENT: 'onboarding-error-token-not-present',
  VALIDATION_FAILED: 'onboarding-error-validation',
  EMAIL_TAKEN: 'onboarding-error-email-taken',
  SETUP_COMPLETE: 'onboarding-error-setup-complete',
};

const router = useRouter();

// Tab-scoped stash for the bootstrap token after we strip it from
// the URL. Survives refresh within the same tab; cleared on tab
// close. Not sent in requests (unlike cookies) and not written to
// disk long-term (unlike localStorage).
const BOOTSTRAP_TOKEN_SESSION_KEY = 'nosdesk-bootstrap-token';

function persistBootstrapToken(token: string) {
  try {
    sessionStorage.setItem(BOOTSTRAP_TOKEN_SESSION_KEY, token.trim());
  } catch {
    // Private mode / quota — degrade to in-memory only.
  }
}

function loadPersistedBootstrapToken(): string | null {
  try {
    const stored = sessionStorage.getItem(BOOTSTRAP_TOKEN_SESSION_KEY);
    return stored?.trim() || null;
  } catch {
    return null;
  }
}

function clearPersistedBootstrapToken() {
  try {
    sessionStorage.removeItem(BOOTSTRAP_TOKEN_SESSION_KEY);
  } catch {
    // ignore
  }
}

// Auto-login composable. The onboarding view drives its own step
// state, so only `attemptLogin` is consumed here.
// No redirect: the finished screen offers the workspace, or bringing one
// over from another Nosdesk while this one is still empty.
const { attemptLogin } = useAutoLogin({ source: 'onboarding', redirect: false });

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

// The hero names a second-factor step only when the server will ask for one
// (REQUIRE_ADMIN_MFA). Until the status answers, assume it will, as the
// server does by default.
const adminMfaRequired = ref(true);

const isSetupStep = computed(() => currentStep.value === 'setup');
const isLoggingIn = computed(() => currentStep.value === 'logging-in');
const isComplete = computed(() => currentStep.value === 'complete');

// The hero describes what comes next. Before the account exists that is the
// setup itself; once the admin is signed in it is the workspace.
const heroTitleKey = computed(() =>
  isComplete.value ? 'onboarding-complete-hero-title' : 'onboarding-hero-title'
);
const heroSubtitleKey = computed(() => {
  if (isComplete.value) return 'onboarding-complete-hero-subtitle';
  return adminMfaRequired.value ? 'onboarding-hero-subtitle' : 'onboarding-hero-subtitle-no-mfa';
});

// The finished screen is a new page in all but the URL, so focus moves to its
// heading as it would on navigation. The signed-in line is the live region,
// so the announcement and the focused heading say different things.
const completeHeading = ref<HTMLHeadingElement | null>(null);
watch(isComplete, async (done) => {
  if (!done) return;
  await nextTick();
  completeHeading.value?.focus({ preventScroll: true });
});

const adminInitials = computed(() => initialsFrom(adminData.value.name));

// Entering the workspace runs the sync bootstrap, a few seconds on a fresh
// install, so the button says so rather than appearing to do nothing. The
// view unmounts on arrival; the flag only resets if the navigation fails.
const isOpeningWorkspace = ref(false);
const openWorkspace = async () => {
  if (isOpeningWorkspace.value) return;
  isOpeningWorkspace.value = true;
  try {
    await router.push('/');
  } catch (error) {
    console.error('Failed to open the workspace:', error);
  } finally {
    isOpeningWorkspace.value = false;
  }
};

const validateForm = (): boolean => {
  if (!bootstrapToken.value.trim()) {
    errorMessage.value = t('onboarding-validation-token');
    return false;
  }
  if (!adminData.value.name.trim()) {
    errorMessage.value = t('onboarding-validation-name');
    return false;
  }
  if (!adminData.value.email.trim()) {
    errorMessage.value = t('onboarding-validation-email');
    return false;
  }
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!emailRegex.test(adminData.value.email)) {
    errorMessage.value = t('onboarding-validation-email-format');
    return false;
  }
  if (adminData.value.password.length < 8) {
    errorMessage.value = t('onboarding-validation-password-length');
    return false;
  }
  if (adminData.value.password !== confirmPassword.value) {
    errorMessage.value = t('onboarding-validation-password-mismatch');
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
    persistBootstrapToken(bootstrapToken.value);
    tokenFromUrl.value = true;
    params.delete('token');
    const cleanQuery = params.toString();
    const cleanUrl = `${window.location.pathname}${cleanQuery ? '?' + cleanQuery : ''}${window.location.hash}`;
    window.history.replaceState({}, '', cleanUrl);
  } else {
    const stored = loadPersistedBootstrapToken();
    if (stored) {
      bootstrapToken.value = stored;
      tokenFromUrl.value = true;
    }
  }

  try {
    const status = await authService.checkSetupStatus();
    adminMfaRequired.value = status.admin_mfa_required !== false;
    if (!status.requires_setup) {
      router.push('/login');
    } else {
      // Fresh install: clear any theme a prior install left in this
      // browser so onboarding renders in the default appearance rather
      // than inheriting someone else's persisted theme. Mirrors the
      // logout reset; respects a deliberate device-local pin.
      useThemeStore().resetToDefault();
    }
  } catch (error) {
    console.error('Error checking setup status:', error);
    errorMessage.value = t('onboarding-error-setup-status');
  }
});

const clearFormSecrets = () => {
  adminData.value.password = '';
  confirmPassword.value = '';
};

const discardBootstrapToken = () => {
  bootstrapToken.value = '';
  clearPersistedBootstrapToken();
};

const handleLoginFallback = () => {
  errorMessage.value = '';
  successMessage.value = t('onboarding-success-fallback');
  setTimeout(() => {
    router.push({
      path: '/login',
      query: {
        message: t('onboarding-success-fallback-redirect'),
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
      discardBootstrapToken();
      currentStep.value = 'logging-in';
      autoLoginAttempted.value = true;

      const loginSuccess = await attemptLogin(adminData.value.email, adminData.value.password);

      if (loginSuccess) {
        currentStep.value = 'complete';
        clearFormSecrets();
      } else {
        handleLoginFallback();
      }
    } else {
      errorMessage.value = response.message || t('onboarding-error-setup-failed');
      currentStep.value = 'setup';
    }
  } catch (error) {
    console.error('Setup error:', error);
    currentStep.value = 'setup';

    const axiosError = error as {
      response?: { status?: number; data?: { error?: string; code?: string; message?: string } };
    };
    const data = axiosError.response?.data;
    const fluentKey = data?.code ? ERROR_CODE_KEYS[data.code] : undefined;
    if (fluentKey) {
      // Localised message keyed on the backend's stable error code.
      errorMessage.value = t(fluentKey);
    } else if (data?.error || data?.message) {
      // Unmapped code: fall back to the server-provided human string.
      errorMessage.value = data.error || data.message || t('onboarding-error-setup-failed');
    } else {
      errorMessage.value = t('onboarding-error-unexpected');
    }
    // 401 from this endpoint means the bootstrap token was
    // rejected (missing, mismatched, or expired). Surface the
    // manual paste field so the operator can correct rather than
    // staring at "click the URL from logs" when their URL is the
    // problem.
    if (axiosError.response?.status === 401 && tokenFromUrl.value) {
      tokenFromUrl.value = false;
      discardBootstrapToken();
    }
  } finally {
    isLoading.value = false;
  }
};

onUnmounted(() => {
  // Passwords only — do not clear the sessionStorage token here.
  // A refresh unmounts this component first; wiping the stash
  // would defeat the refresh-survival behaviour above.
  clearFormSecrets();
});
</script>

<template>
  <AuthLayout>
    <template #logo>
      <LogoIcon class="h-9 w-auto text-accent" :aria-label="$t('nav-logo-alt')" />
    </template>
    <template #pill>{{ $t('auth-hero-pill') }}</template>

    <!-- The hero says what happens after this screen, and nothing else.
         Setup-token help sits by the field on every breakpoint. -->
    <template #hero-title>{{ $t(heroTitleKey) }}</template>
    <template #hero-subtitle>{{ $t(heroSubtitleKey) }}</template>

    <!-- Finished: the account exists and the admin is signed in. This block
         owns the column, so the setup header above the form is gone and the
         page says welcome once, now that it is true. -->
    <div v-if="isComplete" class="flex flex-col gap-8">
      <header class="flex flex-col items-center gap-3 text-center lg:items-start lg:text-left">
        <p class="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wide text-status-success">
          <Icon name="check" size="sm" aria-hidden="true" />
          {{ $t('onboarding-complete-eyebrow') }}
        </p>
        <div class="flex flex-col gap-1.5">
          <h1
            ref="completeHeading"
            tabindex="-1"
            class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary outline-none"
          >
            {{ $t('onboarding-complete-title') }}
          </h1>
          <p role="status" class="text-base text-secondary">{{ $t('onboarding-complete-subtitle') }}</p>
        </div>
      </header>

      <!-- What was set up: the identity this Nosdesk now has an owner under. -->
      <div
        class="flex items-center gap-3 rounded-lg border border-default bg-surface-alt px-4 py-3"
        data-test="onboarding-account"
      >
        <span
          aria-hidden="true"
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-accent-muted text-sm font-semibold text-accent"
        >
          {{ adminInitials }}
        </span>
        <div class="flex min-w-0 flex-1 flex-col">
          <span class="truncate text-sm font-medium text-primary">{{ adminData.name }}</span>
          <span class="truncate text-xs text-secondary">{{ adminData.email }}</span>
        </div>
        <span class="shrink-0 text-xs text-tertiary">{{ $t('onboarding-complete-role') }}</span>
      </div>

      <div class="flex flex-col gap-3">
        <Button
          block
          size="lg"
          data-test="onboarding-open"
          :loading="isOpeningWorkspace"
          @click="openWorkspace"
        >
          {{ isOpeningWorkspace ? $t('onboarding-complete-opening') : $t('onboarding-complete-open') }}
        </Button>

        <!-- The other way in. A card rather than a second button, so the
             import reads as a path with a condition, not a lesser version of
             the one above. -->
        <button
          type="button"
          data-test="onboarding-import"
          :disabled="isOpeningWorkspace"
          class="group flex w-full items-start gap-3 rounded-lg border border-default p-4 text-left transition-colors hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface disabled:pointer-events-none disabled:opacity-60"
          @click="router.push({ name: 'admin-workspace-import' })"
        >
          <span
            aria-hidden="true"
            class="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-surface-alt text-secondary"
          >
            <Icon name="move" size="md" />
          </span>
          <span class="flex min-w-0 flex-1 flex-col gap-1">
            <span class="text-sm font-medium text-primary">{{ $t('onboarding-complete-import') }}</span>
            <span class="text-xs leading-relaxed text-secondary">{{ $t('onboarding-complete-import-note') }}</span>
          </span>
          <Icon
            name="chevronRight"
            size="sm"
            aria-hidden="true"
            class="mt-0.5 shrink-0 text-tertiary transition-colors group-hover:text-secondary"
          />
        </button>
      </div>
    </div>

    <div v-else class="flex flex-col gap-6">
      <header class="flex flex-col gap-1.5 text-center lg:text-left">
        <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
          {{ $t('onboarding-welcome-title') }}
        </h1>
        <p class="text-base text-secondary">{{ $t('onboarding-welcome-subtitle') }}</p>
      </header>

      <!-- Sign-in fallback: the account exists but the session did not start,
           so the login page takes over in a moment. -->
      <div v-if="successMessage" class="bg-status-success/20 border border-status-success/50 text-status-success px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-3">
          <Icon name="check" size="lg" class="flex-shrink-0" />
          <span>{{ successMessage }}</span>
        </div>
      </div>

      <!-- Error Message -->
      <div v-if="errorMessage" class="bg-status-error/20 border border-status-error/50 text-status-error px-4 py-3 rounded-lg text-sm">
        <div class="flex items-center gap-3">
          <Icon name="warning" size="lg" class="flex-shrink-0" />
          <span>{{ errorMessage }}</span>
        </div>
      </div>

      <!-- Setup Form -->
      <form v-if="isSetupStep" @submit.prevent="handleSetup" class="flex flex-col gap-5">
        <!--
          Bootstrap token field. Hidden on the happy path where
          the operator arrived via the setup URL from server
          logs (the token is already in our state and stripped
          from the address bar). Visible only when no URL token
          was supplied, or when verification just rejected one
          we did supply.
        -->
        <div v-if="!tokenFromUrl" class="flex flex-col gap-1.5">
          <FormInput
            v-model="bootstrapToken"
            :label="$t('onboarding-token-label')"
            type="password"
            required
            autocomplete="off"
            :disabled="isLoading"
            class="font-mono"
            :placeholder="$t('onboarding-token-placeholder')"
          />
          <div class="flex flex-col gap-1.5">
            <p class="text-xs text-tertiary">{{ $t('onboarding-token-hint') }}</p>
            <CodeBlock code="docker compose exec nosdesk nosdesk-cli setup-token" />
          </div>
        </div>

        <FormInput
          v-model="adminData.name"
          :label="$t('onboarding-name-label')"
          type="text"
          required
          autocomplete="name"
          :disabled="isLoading"
          :placeholder="$t('onboarding-name-placeholder')"
        />

        <FormInput
          v-model="adminData.email"
          :label="$t('onboarding-email-label')"
          type="email"
          required
          autocomplete="email"
          :disabled="isLoading"
          :placeholder="$t('onboarding-email-placeholder')"
        />

        <div class="flex flex-col gap-1.5">
          <label for="admin-password" class="text-xs font-medium text-tertiary uppercase tracking-wide">
            {{ $t('onboarding-password-label') }}
          </label>
          <PasswordInput
            id="admin-password"
            v-model="adminData.password"
            required
            autocomplete="new-password"
            :disabled="isLoading"
            :placeholder="$t('onboarding-password-placeholder')"
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label for="confirm-password" class="text-xs font-medium text-tertiary uppercase tracking-wide">
            {{ $t('onboarding-confirm-password-label') }}
          </label>
          <PasswordInput
            id="confirm-password"
            v-model="confirmPassword"
            required
            autocomplete="new-password"
            :disabled="isLoading"
            :placeholder="$t('onboarding-confirm-password-placeholder')"
          />
        </div>

        <Button type="submit" variant="primary" block :disabled="!canSubmit" :loading="isLoading">
          {{ isLoading ? $t('onboarding-submit-loading') : $t('onboarding-submit') }}
        </Button>
      </form>

      <!-- Signing the new admin in. Shown between the account's creation and
           the finished screen; the fallback banner above replaces it when the
           session did not start. -->
      <div v-else-if="isLoggingIn && !successMessage" class="flex flex-col items-center gap-6 text-center lg:items-start lg:text-left">
        <div class="flex items-center justify-center w-16 h-16 bg-accent/10 rounded-full">
          <Spinner size="lg" class="text-accent" />
        </div>
        <div class="flex flex-col gap-2">
          <h2 class="text-lg font-semibold text-primary">{{ $t('onboarding-progress-title') }}</h2>
          <p class="text-secondary">{{ $t('onboarding-progress-subtitle') }}</p>
        </div>
      </div>
    </div>
  </AuthLayout>
</template>
