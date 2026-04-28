<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { usePasskeys } from '@/composables/usePasskeys';
import { useClipboard } from '@/composables/useClipboard';
import { useAuthStore } from '@/stores/auth';
import { useMfaSetupStore } from '@/stores/mfaSetup';
import { passkeySetupService } from '@/services/passkeyService';
import { logger } from '@/utils/logger';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';

// Props
const props = defineProps<{
  isLoginSetup?: boolean;
}>();

// Emits
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Use passkeys composable (for normal flow and browser support check)
const {
  registering,
  error,
  isSupported,
  registerPasskey,
  clearMessages,
} = usePasskeys();

const authStore = useAuthStore();
const mfaSetupStore = useMfaSetupStore();

// Local state
const passkeyName = ref('');
const setupComplete = ref(false);
const setupStep = ref<'setup' | 'backup-codes' | 'success'>('setup');
const localRegistering = ref(false);
const localError = ref<string | null>(null);
const backupCodes = ref<string[]>([]);


// Computed
const isSecureContext = computed(() => window?.isSecureContext ?? false);
const isRegistering = computed(() => props.isLoginSetup ? localRegistering.value : registering.value);

const deviceName = computed(() => {
  const ua = navigator.userAgent;
  if (ua.includes('iPhone')) return 'iPhone';
  if (ua.includes('iPad')) return 'iPad';
  if (ua.includes('Mac')) return 'Mac';
  if (ua.includes('Windows')) return 'Windows PC';
  if (ua.includes('Android')) return 'Android Device';
  if (ua.includes('Linux')) return 'Linux PC';
  return 'This Device';
});

// Start passkey registration
const handleRegisterPasskey = async () => {
  clearMessages();
  localError.value = null;

  const name = passkeyName.value.trim() || deviceName.value;

  // Use different flow based on whether this is MFA setup or normal registration
  if (props.isLoginSetup) {
    // MFA setup flow - use credential-based endpoints
    const credentials = mfaSetupStore.getCredentials;
    if (!credentials) {
      localError.value = 'Session expired. Please log in again.';
      emit('error', localError.value);
      return;
    }

    localRegistering.value = true;
    try {
      const result = await passkeySetupService.registerPasskey(
        { email: credentials.email, password: credentials.password },
        name
      );

      if (result.success) {
        // Set auth provider for consistency with other login flows
        authStore.setAuthProvider('local');

        // Backend has set the cookies, fetch user data to populate the store
        // If this fails, we still proceed since the passkey is registered and cookies are set
        try {
          await authStore.fetchUserData();
        } catch (fetchErr) {
          logger.warn('Failed to fetch user data after passkey setup, continuing anyway', { error: fetchErr });
        }

        // Show backup codes if returned
        if (result.backup_codes && result.backup_codes.length > 0) {
          backupCodes.value = result.backup_codes;
          setupStep.value = 'backup-codes';
        } else {
          setupStep.value = 'success';
        }
        setupComplete.value = true;
        emit('success', 'Passkey created successfully');
      }
    } catch (err: unknown) {
      logger.error('Passkey setup registration failed', { error: err });

      if (err instanceof Error) {
        if (err.name === 'NotAllowedError') {
          localError.value = 'Registration was cancelled or not allowed';
        } else if (err.name === 'InvalidStateError') {
          localError.value = 'This passkey is already registered';
        } else if (err.message.includes('cancelled')) {
          localError.value = 'Registration was cancelled';
        } else {
          // Try to extract error message from API response
          const axiosErr = err as { response?: { data?: { error?: string } } };
          localError.value = axiosErr.response?.data?.error || err.message || 'Failed to register passkey';
        }
      } else {
        localError.value = 'Failed to register passkey';
      }

      emit('error', localError.value);
    } finally {
      localRegistering.value = false;
    }
  } else {
    // Normal flow - use JWT-authenticated endpoints
    const success = await registerPasskey(name);

    if (success) {
      setupStep.value = 'success';
      setupComplete.value = true;
      emit('success', 'Passkey created successfully');
    } else if (error.value) {
      emit('error', error.value);
    }
  }
};

// Copy backup codes to clipboard
const { copied: backupCodesCopied, copy: clipboardCopy } = useClipboard();
const copyBackupCodes = () => clipboardCopy(backupCodes.value.join('\n'));

// Download backup codes as text file
const downloadBackupCodes = () => {
  const text = `Nosdesk Recovery Codes\n${'='.repeat(30)}\n\nSave these codes in a safe place. Each code can only be used once.\n\n${backupCodes.value.join('\n')}\n`;
  const blob = new Blob([text], { type: 'text/plain' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'nosdesk-recovery-codes.txt';
  a.click();
  URL.revokeObjectURL(url);
};

// Proceed from backup codes to success
const acknowledgeBackupCodes = () => {
  setupStep.value = 'success';
};

// Complete setup and continue
const completeSetup = () => {
  mfaSetupStore.clearCredentials();
  emit('success', 'setup-complete');
};

// Initialize
onMounted(() => {
  // Set default name to device
  passkeyName.value = deviceName.value;
});
</script>

<template>
  <div class="bg-surface rounded-xl border border-default overflow-hidden">
    <div class="p-5 sm:p-6">
      <!-- Browser not supported -->
      <div v-if="!isSupported" class="bg-status-warning/10 border border-status-warning/20 rounded-lg p-4">
        <div class="flex items-start gap-3">
          <span class="text-status-warning flex-shrink-0 mt-0.5 inline-flex">
            <Icon name="warning" size="md" />
          </span>
          <div>
            <p class="text-status-warning font-medium">Passkeys Not Available</p>
            <p class="text-sm text-tertiary mt-1">
              <template v-if="!isSecureContext">
                Passkeys require a secure connection (HTTPS). You're currently on an insecure connection.
              </template>
              <template v-else>
                Your browser does not support passkeys. Please use a modern browser like Chrome, Safari, Firefox, or Edge, or choose the authenticator app option instead.
              </template>
            </p>
          </div>
        </div>
      </div>

      <!-- Setup Step -->
      <div v-else-if="setupStep === 'setup'" class="flex flex-col gap-5">
        <div class="flex flex-col items-center text-center gap-3">
          <div class="w-16 h-16 rounded-full bg-accent/10 flex items-center justify-center">
            <svg class="w-8 h-8 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M7.864 4.243A7.5 7.5 0 0119.5 10.5c0 2.92-.556 5.709-1.568 8.268M5.742 6.364A7.465 7.465 0 004.5 10.5a7.464 7.464 0 01-1.15 3.993m1.989 3.559A11.209 11.209 0 008.25 10.5a3.75 3.75 0 117.5 0c0 .527-.021 1.049-.064 1.565M12 10.5a14.94 14.94 0 01-3.6 9.75m6.633-4.596a18.666 18.666 0 01-2.485 5.33" />
            </svg>
          </div>
          <div>
            <h3 class="text-base font-semibold text-primary mb-1">Set Up Passkey</h3>
            <p class="text-secondary text-sm">
              Sign in securely using Face ID, Touch ID, Windows Hello, or a security key.
            </p>
          </div>
        </div>

        <div>
          <label class="text-sm font-medium text-secondary block mb-1.5">Passkey Name</label>
          <input
            v-model="passkeyName"
            type="text"
            class="w-full px-3 py-2 bg-surface text-primary rounded-lg border border-default focus:ring-1 focus:ring-accent focus:border-accent focus:outline-none transition-colors"
            placeholder="e.g., MacBook Pro, iPhone"
            maxlength="100"
          />
          <p class="text-xs text-tertiary mt-1.5">A name to identify this passkey later</p>
        </div>

        <button
          @click="handleRegisterPasskey"
          :disabled="isRegistering"
          class="w-full py-3 px-4 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
        >
          <Spinner v-if="isRegistering" />
          {{ isRegistering ? 'Creating Passkey...' : 'Create Passkey' }}
        </button>
      </div>

      <!-- Backup Codes Step -->
      <div v-else-if="setupStep === 'backup-codes'" class="flex flex-col gap-5">
        <div class="flex flex-col items-center text-center gap-3">
          <div class="w-16 h-16 rounded-full bg-status-warning/10 flex items-center justify-center">
            <svg class="w-8 h-8 text-status-warning" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
          </div>
          <div>
            <h3 class="text-base font-semibold text-primary mb-1">Save Your Recovery Codes</h3>
            <p class="text-secondary text-sm">
              If you lose access to your passkey, you can use one of these codes to sign in. Each code can only be used once.
            </p>
          </div>
        </div>

        <div class="bg-surface border border-default rounded-lg p-4">
          <div class="grid grid-cols-2 gap-2">
            <code
              v-for="code in backupCodes"
              :key="code"
              class="text-sm font-mono text-primary bg-app px-3 py-1.5 rounded text-center"
            >{{ code }}</code>
          </div>
        </div>

        <div class="flex gap-2">
          <button
            @click="copyBackupCodes"
            class="flex-1 py-2 px-3 border border-default rounded-lg text-sm font-medium text-secondary bg-surface hover:bg-surface-hover transition-colors flex items-center justify-center gap-1.5"
          >
            <span v-if="backupCodesCopied" class="text-status-success inline-flex">
              <Icon name="check" />
            </span>
            <Icon v-else name="copy" />
            {{ backupCodesCopied ? 'Copied!' : 'Copy' }}
          </button>
          <button
            @click="downloadBackupCodes"
            class="flex-1 py-2 px-3 border border-default rounded-lg text-sm font-medium text-secondary bg-surface hover:bg-surface-hover transition-colors flex items-center justify-center gap-1.5"
          >
            <Icon name="download" />
            Download
          </button>
        </div>

        <button
          @click="acknowledgeBackupCodes"
          class="w-full py-3 px-4 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent font-medium transition-colors"
        >
          I've saved my recovery codes
        </button>
      </div>

      <!-- Success Step -->
      <div v-else-if="setupStep === 'success'" class="flex flex-col gap-5">
        <div class="flex flex-col items-center text-center gap-3">
          <div class="w-16 h-16 rounded-full bg-status-success/10 flex items-center justify-center">
            <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
            </svg>
          </div>
          <div>
            <h3 class="text-base font-semibold text-primary mb-1">Passkey Created!</h3>
            <p class="text-secondary text-sm">
              Your passkey "{{ passkeyName }}" is ready to use.
            </p>
          </div>
        </div>

        <div class="bg-surface border border-default rounded-lg p-3 sm:p-4">
          <div class="flex flex-row items-start gap-3">
            <svg class="w-5 h-5 text-status-success mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
            </svg>
            <div class="flex-1 min-w-0">
              <h4 class="font-medium text-primary mb-1 text-sm">Your account is protected</h4>
              <p class="text-xs text-tertiary">
                Next time you sign in, just use your fingerprint, face, or security key instead of a password.
              </p>
            </div>
          </div>
        </div>

        <button
          @click="completeSetup"
          class="w-full py-3 px-4 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent font-medium transition-colors"
        >
          Start Using Nosdesk!
        </button>
      </div>
    </div>
  </div>
</template>
