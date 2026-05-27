<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useAuthStore } from '@/stores/auth';
import authService from '@/services/authService';
import userService from '@/services/userService';
import { formatDate, formatRelativeTime } from '@/utils/dateUtils';
import { extractErrorMessage } from '@/utils/errors';
import { logger } from '@/utils/logger';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
  targetUserUuid?: string;
}>();

interface AuthMethod {
  id: string;
  type: 'local' | 'microsoft';
  identifier: string;
  isPrimary: boolean;
  createdAt?: string;
}

interface ActiveSession {
  id: number;
  device_name: string | null;
  ip_address: string | null;
  user_agent: string | null;
  location: string | null;
  created_at: string;
  last_active: string;
  expires_at: string;
  is_current: boolean;
}

// State
const authStore = useAuthStore();
const authMethods = ref<AuthMethod[]>([]);
const activeSessions = ref<ActiveSession[]>([]);
const loading = ref(false);

const isManagingOtherUser = computed(() => {
  return !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid;
});

// Session state
const sessionsLoading = ref(true);
const revokingSessionId = ref<number | null>(null);
const showRevokeAllModal = ref(false);
const revokeAllInFlight = ref(false);
const stepUpCredential = ref('');
const mfaEnabled = ref(false);

// Computed properties
const hasMicrosoftConnection = computed(() => authMethods.value.some(m => m.type === 'microsoft'));

const otherSessionsCount = computed(
  () => activeSessions.value.filter((s) => !s.is_current).length,
);

// Which credential the "sign out everywhere else" step-up should ask for.
// Prefer the local password (the mfa_disable precedent); fall back to a
// TOTP/backup code for accounts without one; 'none' when there's nothing
// to step up with (OAuth-only, no MFA).
const hasLocalPassword = computed(() => authMethods.value.some((m) => m.type === 'local'));
const stepUpMethod = computed<'password' | 'mfa' | 'none'>(() => {
  if (hasLocalPassword.value) return 'password';
  if (mfaEnabled.value) return 'mfa';
  return 'none';
});
const revokeAllConfirmDisabled = computed(
  () => stepUpMethod.value !== 'none' && stepUpCredential.value.trim().length === 0,
);

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Load data on mount
onMounted(() => loadAuthData());

const getAuthMethodLabel = (type: string) => {
  switch (type) {
    case 'local': return t('settings-auth-methods-type-local');
    case 'microsoft': return t('settings-auth-methods-type-microsoft');
    default: return type;
  }
};

// Load auth methods (works for both self and admin paths)
const loadAuthData = async () => {
  try {
    loading.value = true;

    let identities: { id: number; provider_type: string; email?: string | null; provider_name?: string; created_at: string }[];

    if (isManagingOtherUser.value && props.targetUserUuid) {
      const securityInfo = await userService.getUserSecurityInfo(props.targetUserUuid);
      identities = securityInfo.auth_identities;
    } else {
      identities = await authService.getUserAuthIdentities();
    }

    authMethods.value = identities.map((identity) => ({
      id: identity.id.toString(),
      type: identity.provider_type as AuthMethod['type'],
      identifier: identity.email || `${identity.provider_name} Account`,
      isPrimary: identity.provider_type === 'local',
      createdAt: identity.created_at,
    }));

    if (!isManagingOtherUser.value) {
      await loadActiveSessions();
      // Decide which credential the bulk-revoke step-up asks for when the
      // account has no local password (best-effort; defaults to none).
      try {
        const status = await authService.getMFAStatus();
        mfaEnabled.value = status.enabled;
      } catch (mfaErr) {
        logger.debug('Could not load MFA status for session step-up', { error: mfaErr });
      }
    }
  } catch (error) {
    logger.error('Failed to load auth methods', { error });
  } finally {
    loading.value = false;
  }
};

const loadActiveSessions = async () => {
  try {
    sessionsLoading.value = true;
    const sessions = await authService.getSessions();
    logger.debug('Loaded active sessions', { count: sessions?.length || 0 });
    activeSessions.value = sessions;
  } catch (error) {
    logger.error('Failed to load active sessions', { error });
    emit('error', t('settings-auth-methods-sessions-load-error'));
  } finally {
    sessionsLoading.value = false;
  }
};

// Auth method functions
const addAuthMethod = async (type: 'microsoft') => {
  loading.value = true;
  const providerName = type.charAt(0).toUpperCase() + type.slice(1);
  try {
    // Use authService to connect OAuth provider
    const data = await authService.connectOAuthProvider(type);

    if (data.auth_url) {
      // Redirect to OAuth provider - when user returns, the page will reload
      // and the new connected account will be displayed
      window.location.href = data.auth_url;
      return;
    }

    emit('success', t('settings-auth-methods-link-success', { provider: providerName }));
    // Reload auth methods to show the newly connected account
    await loadAuthData();
  } catch (err) {
    emit('error', t('settings-auth-methods-link-error', { provider: providerName }));
    logger.error('Failed to link account', { error: err, type });
  } finally {
    loading.value = false;
  }
};

const removeAuthMethod = async (methodId: string, methodType: string) => {
  loading.value = true;
  try {
    if (methodType === 'microsoft') {
      // Handle Microsoft identity removal using authService
      await authService.deleteUserAuthIdentity(parseInt(methodId));
    }

    // Reload auth methods after deletion
    await loadAuthData();
    emit('success', t('settings-auth-methods-remove-success'));
  } catch (err) {
    // Extract error message from backend response
    const axiosError = err as { response?: { data?: { message?: string } } };
    const errorMessage = axiosError.response?.data?.message || t('settings-auth-methods-remove-error');
    emit('error', errorMessage);
    logger.error('Failed to remove auth method', { error: err, methodId, methodType });
  } finally {
    loading.value = false;
  }
};

// Admin: remove an auth method for the target user
const adminRemoveAuthMethod = async (methodId: string) => {
  if (!props.targetUserUuid) return;
  loading.value = true;
  try {
    await userService.adminDeleteUserAuthIdentity(props.targetUserUuid, parseInt(methodId));
    await loadAuthData();
    emit('success', t('settings-auth-methods-remove-success'));
  } catch (err) {
    const axiosError = err as { response?: { data?: { message?: string } } };
    const errorMessage = axiosError.response?.data?.message || t('settings-auth-methods-remove-error');
    emit('error', errorMessage);
    logger.error('Failed to remove auth method (admin)', { error: err, methodId });
  } finally {
    loading.value = false;
  }
};

// Single-session revoke is low-friction (one click, per-row in-flight
// state) per session-management UX norms. The high-blast-radius "sign
// out everywhere else" goes through the confirm dialog + step-up
// re-auth below.
const revokeSession = async (sessionId: number) => {
  revokingSessionId.value = sessionId;
  try {
    await authService.revokeSession(sessionId);
    await loadActiveSessions();
    emit('success', t('settings-auth-methods-sessions-revoke-success'));
  } catch (err) {
    emit('error', extractErrorMessage(err, t('settings-auth-methods-sessions-revoke-error')));
    logger.error('Failed to revoke session', { error: err, sessionId });
  } finally {
    revokingSessionId.value = null;
  }
};

const openRevokeAllModal = () => {
  stepUpCredential.value = '';
  showRevokeAllModal.value = true;
};

const confirmRevokeAll = async () => {
  if (revokeAllConfirmDisabled.value) return;
  revokeAllInFlight.value = true;
  try {
    const credential =
      stepUpMethod.value === 'password'
        ? { password: stepUpCredential.value }
        : stepUpMethod.value === 'mfa'
          ? { mfa_code: stepUpCredential.value.trim() }
          : {};
    await authService.revokeAllOtherSessions(credential);
    showRevokeAllModal.value = false;
    await loadActiveSessions();
    emit('success', t('settings-auth-methods-sessions-revoke-all-success'));
  } catch (err) {
    emit('error', extractErrorMessage(err, t('settings-auth-methods-sessions-revoke-all-error')));
    logger.error('Failed to revoke all sessions', { error: err });
  } finally {
    revokeAllInFlight.value = false;
  }
};

// Utility functions
const getAuthMethodIcon = (type: string) => {
  switch (type) {
    case 'microsoft':
      return 'microsoft';
    default:
      return 'local';
  }
};
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- Authentication Methods -->
    <SectionCard content-padding="p-4 sm:p-6">
      <template #title>{{ $t('settings-auth-methods-section-title') }}</template>

      <div class="flex flex-col gap-3">
        <!-- Auth Methods -->
        <div class="flex flex-col gap-2">
            <div v-for="method in authMethods" :key="method.id" class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
              <div class="flex items-center gap-3">
                <!-- Email Icon -->
                <div v-if="getAuthMethodIcon(method.type) === 'local'" class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                  <span class="text-accent inline-flex">
                    <Icon name="email" size="md" />
                  </span>
                </div>
                <!-- Microsoft Icon (4-square grid pattern) -->
                <div v-else-if="getAuthMethodIcon(method.type) === 'microsoft'" class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 24 24" fill="none">
                    <rect x="4" y="4" width="7" height="7" class="fill-accent" />
                    <rect x="13" y="4" width="7" height="7" class="fill-accent" />
                    <rect x="4" y="13" width="7" height="7" class="fill-accent" />
                    <rect x="13" y="13" width="7" height="7" class="fill-accent" />
                  </svg>
                </div>
                <div>
                  <div class="text-sm font-medium text-primary">
                    {{ getAuthMethodLabel(method.type) }}
                    <span v-if="method.isPrimary" class="ml-2 px-2 py-1 bg-accent/20 text-accent rounded text-xs">{{ $t('settings-auth-methods-primary-badge') }}</span>
                  </div>
                  <div v-if="method.identifier" class="text-xs text-tertiary">
                    {{ method.identifier }}<template v-if="method.createdAt"> {{ $t('settings-auth-methods-added-suffix', { date: formatDate(method.createdAt, 'MMM d, yyyy') }) }}</template>
                  </div>
                </div>
              </div>
              <Button
                v-if="!method.isPrimary && authMethods.length > 1"
                variant="ghost-danger"
                size="sm"
                :disabled="loading"
                @click="isManagingOtherUser ? adminRemoveAuthMethod(method.id) : removeAuthMethod(method.id, method.type)"
              >
                {{ $t('settings-auth-methods-remove') }}
              </Button>
            </div>
        </div>

        <!-- Add Auth Method (hidden for admin viewing another user) -->
        <button
          v-if="!isManagingOtherUser"
          @click="addAuthMethod('microsoft')"
          :disabled="loading || hasMicrosoftConnection"
          class="flex items-center gap-3 p-3 bg-surface-alt hover:bg-surface-hover rounded-lg border border-dashed border-subtle hover:border-default transition-colors disabled:opacity-50 w-full"
        >
          <div class="w-8 h-8 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none">
              <rect x="4" y="4" width="7" height="7" class="fill-accent" />
              <rect x="13" y="4" width="7" height="7" class="fill-accent" />
              <rect x="4" y="13" width="7" height="7" class="fill-accent" />
              <rect x="13" y="13" width="7" height="7" class="fill-accent" />
            </svg>
          </div>
          <div class="flex flex-col items-start">
            <span class="text-sm font-medium text-primary">{{ $t('settings-auth-methods-connect-microsoft') }}</span>
            <span class="text-xs text-tertiary">
              {{ hasMicrosoftConnection ? $t('settings-auth-methods-connect-microsoft-already') : $t('settings-auth-methods-connect-microsoft-provider') }}
            </span>
          </div>
        </button>
      </div>
    </SectionCard>

    <!-- Active Sessions (hidden for admin viewing another user) -->
    <SectionCard v-if="!isManagingOtherUser" content-padding="p-4 sm:p-6">
      <template #title>{{ $t('settings-auth-methods-sessions-section-title') }}</template>
      <template #headerActions>
        <Button
          variant="ghost-danger"
          size="sm"
          :disabled="otherSessionsCount === 0"
          @click="openRevokeAllModal"
        >
          {{ $t('settings-auth-methods-sessions-revoke-all') }}
        </Button>
      </template>

      <!-- Loading skeleton mirrors the eventual row layout -->
      <Skeleton v-if="sessionsLoading" class="flex flex-col gap-2">
        <div v-for="n in 2" :key="n" class="flex items-center gap-3 p-3 bg-surface-alt rounded-lg">
          <SkeletonBar class="h-10 w-10 rounded-lg" />
          <div class="flex flex-col gap-2 flex-1">
            <SkeletonBar class="h-4 w-40" />
            <SkeletonBar class="h-3 w-56" />
          </div>
        </div>
      </Skeleton>

      <div v-else class="flex flex-col gap-2">
            <div v-for="session in activeSessions" :key="session.id" class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
              <div class="flex items-center gap-3 min-w-0">
                <div class="w-10 h-10 bg-surface-hover rounded-lg flex items-center justify-center flex-shrink-0">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                  </svg>
                </div>
                <div class="min-w-0">
                  <div class="text-sm font-medium text-primary truncate">
                    {{ session.device_name || session.user_agent || $t('settings-auth-methods-sessions-unknown-device') }}
                    <span v-if="session.is_current" class="ml-2 px-2 py-1 bg-status-success/20 text-status-success rounded text-xs">{{ $t('settings-auth-methods-sessions-current-badge') }}</span>
                  </div>
                  <div class="text-xs text-tertiary truncate">
                    {{ $t('settings-auth-methods-sessions-last-active', { location: session.location || session.ip_address || $t('settings-auth-methods-sessions-unknown-location'), date: formatRelativeTime(session.last_active) }) }}
                  </div>
                </div>
              </div>
              <Button
                v-if="!session.is_current"
                variant="ghost-danger"
                size="sm"
                class="flex-shrink-0 ml-3"
                :loading="revokingSessionId === session.id"
                :disabled="revokingSessionId !== null"
                :aria-label="$t('settings-auth-methods-sessions-revoke-aria', { device: session.device_name || session.user_agent || $t('settings-auth-methods-sessions-unknown-device') })"
                @click="revokeSession(session.id)"
              >
                {{ $t('settings-auth-methods-sessions-revoke') }}
              </Button>
            </div>
      </div>
    </SectionCard>

    <!-- "Sign out everywhere else": confirm + step-up re-auth -->
    <ConfirmModal
      :show="showRevokeAllModal"
      variant="danger"
      :title="$t('settings-auth-methods-sessions-revoke-all-title')"
      :message="$t('settings-auth-methods-sessions-revoke-all-message', { count: otherSessionsCount })"
      :confirm-label="$t('settings-auth-methods-sessions-revoke-all-confirm')"
      :confirm-disabled="revokeAllConfirmDisabled || revokeAllInFlight"
      @confirm="confirmRevokeAll"
      @close="showRevokeAllModal = false"
    >
      <template #body>
        <div v-if="stepUpMethod !== 'none'" class="mt-3 flex flex-col gap-1.5">
          <!-- Sentence-style prompt kept as an external label (FormInput's
               own label styling is for short uppercase field names). -->
          <label for="revoke-all-stepup" class="text-xs font-medium text-secondary">
            {{ stepUpMethod === 'password' ? $t('settings-auth-methods-sessions-stepup-password') : $t('settings-auth-methods-sessions-stepup-mfa') }}
          </label>
          <FormInput
            id="revoke-all-stepup"
            v-model="stepUpCredential"
            :type="stepUpMethod === 'password' ? 'password' : 'text'"
            :inputmode="stepUpMethod === 'mfa' ? 'numeric' : undefined"
            :autocomplete="stepUpMethod === 'password' ? 'current-password' : 'one-time-code'"
            @keyup.enter="confirmRevokeAll"
          />
        </div>
      </template>
    </ConfirmModal>
  </div>
</template>
