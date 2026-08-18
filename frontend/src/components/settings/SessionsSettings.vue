<!--
SessionsSettings — the user's active sign-in sessions.

Self-only by construction: every /auth/sessions endpoint derives the
user from the caller's JWT (not a target uuid), so this card cannot
show another user's sessions. ProfileSettingsView only renders it for
the current user (`v-if="!isAdminMode"`), so unlike the sibling cards
it takes no `targetUserUuid` prop.

Per-session "Sign out" is a plain revoke + cache invalidate. "Sign out
all other sessions" is high-blast-radius and the backend gates it
behind step-up re-auth (full session + a fresh credential), so it
opens a ConfirmModal that asks for the one credential the account
actually has: the local password if set, else an MFA code, else
nothing (OAuth-only without MFA).

The session list is cache-first via Pinia Colada so revisiting the tab
is instant; revokes invalidate the query to refetch.
-->
<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import authService, { type SessionInfo } from '@nosdesk/core/services/authService';
import { extractErrorMessage } from '@/utils/errors';
import { logger } from '@nosdesk/core/utils/logger';
import { formatRelativeTime, formatDate } from '@nosdesk/core/utils/dateUtils';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';

const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const queryCache = useQueryCache();

const SESSIONS_KEY = ['auth', 'sessions'];

const sessionsQuery = useQuery({
  key: () => SESSIONS_KEY,
  query: () => authService.getSessions(),
});

const sessions = computed<SessionInfo[]>(() => sessionsQuery.data.value ?? []);
const loading = computed(
  () => sessionsQuery.status.value === 'pending' && sessions.value.length === 0,
);
const otherSessionsCount = computed(() => sessions.value.filter((s) => !s.is_current).length);

function refresh() {
  queryCache.invalidateQueries({ key: SESSIONS_KEY });
}

// Which credential the bulk-revoke step-up should ask for. Prefer the
// local password (the mfa_disable precedent); fall back to a TOTP/backup
// code for accounts without one; 'none' when there's nothing to step up
// (OAuth-only, no MFA). Loaded once on mount, best-effort.
const hasLocalPassword = ref(false);
const mfaEnabled = ref(false);
const stepUpMethod = computed<'password' | 'mfa' | 'none'>(() => {
  if (hasLocalPassword.value) return 'password';
  if (mfaEnabled.value) return 'mfa';
  return 'none';
});

onMounted(async () => {
  try {
    const identities = await authService.getUserAuthIdentities();
    hasLocalPassword.value = identities.some((i) => i.provider_type === 'local');
  } catch (err) {
    logger.debug('Could not load auth identities for session step-up', { error: err });
  }
  try {
    const status = await authService.getMFAStatus();
    mfaEnabled.value = status.enabled;
  } catch (err) {
    logger.debug('Could not load MFA status for session step-up', { error: err });
  }
});

/**
 * Human label for a session row. Prefer the backend-stored
 * device_name; otherwise derive a coarse "Browser on OS" from the
 * user-agent; otherwise a generic fallback. Deliberately minimal —
 * full UA parsing is not worth a dependency for a settings list.
 */
function deviceLabel(session: SessionInfo): string {
  if (session.device_name) return session.device_name;
  const ua = session.user_agent ?? '';
  if (!ua) return t('settings-sessions-unknown-device');
  const browser =
    /Edg/i.test(ua) ? 'Edge'
    : /OPR|Opera/i.test(ua) ? 'Opera'
    : /Firefox/i.test(ua) ? 'Firefox'
    : /Chrome|Chromium/i.test(ua) ? 'Chrome'
    : /Safari/i.test(ua) ? 'Safari'
    : null;
  const os =
    /Windows/i.test(ua) ? 'Windows'
    : /iPhone|iPad|iOS/i.test(ua) ? 'iOS'
    : /Mac OS X|Macintosh/i.test(ua) ? 'macOS'
    : /Android/i.test(ua) ? 'Android'
    : /Linux/i.test(ua) ? 'Linux'
    : null;
  if (browser && os) return `${browser} ${t('settings-sessions-on')} ${os}`;
  return browser ?? os ?? t('settings-sessions-unknown-device');
}

// --- Per-session revoke ---
const revokingId = ref<string | null>(null);

async function handleRevoke(session: SessionInfo) {
  revokingId.value = session.session_id;
  try {
    await authService.revokeSession(session.session_id);
    refresh();
    emit('success', t('settings-sessions-revoke-success'));
  } catch (err) {
    emit('error', extractErrorMessage(err, t('settings-sessions-revoke-error')));
    logger.error('Failed to revoke session', { error: err, sessionId: session.session_id });
  } finally {
    revokingId.value = null;
  }
}

// --- Sign out all other sessions (step-up re-auth) ---
const showRevokeAllModal = ref(false);
const stepUpCredential = ref('');
const revokingAll = ref(false);
const modalError = ref<string | null>(null);

const revokeAllConfirmDisabled = computed(
  () => stepUpMethod.value !== 'none' && stepUpCredential.value.trim().length === 0,
);

function openRevokeAllModal() {
  stepUpCredential.value = '';
  modalError.value = null;
  showRevokeAllModal.value = true;
}

function closeRevokeAllModal() {
  showRevokeAllModal.value = false;
  stepUpCredential.value = '';
  modalError.value = null;
}

async function handleRevokeAll() {
  if (revokeAllConfirmDisabled.value) return;
  revokingAll.value = true;
  modalError.value = null;
  try {
    const credential =
      stepUpMethod.value === 'password'
        ? { password: stepUpCredential.value }
        : stepUpMethod.value === 'mfa'
          ? { mfa_code: stepUpCredential.value.trim() }
          : {};
    await authService.revokeAllOtherSessions(credential);
    closeRevokeAllModal();
    refresh();
    emit('success', t('settings-sessions-revoke-others-success'));
  } catch (err) {
    // Keep the modal open so the user can correct the credential.
    modalError.value = extractErrorMessage(err, t('settings-sessions-revoke-others-error'));
  } finally {
    revokingAll.value = false;
  }
}
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ $t('settings-sessions-section-title') }}</template>
    <template #headerActions>
      <Button
        variant="ghost-danger"
        size="sm"
        :disabled="otherSessionsCount === 0"
        @click="openRevokeAllModal"
      >
        {{ $t('settings-sessions-revoke-others-button') }}
      </Button>
    </template>

    <div>
      <div v-if="loading" class="flex items-center justify-center py-8 text-accent">
        <Spinner size="lg" />
      </div>

      <div v-else-if="sessions.length === 0" class="py-2">
        <p class="text-sm text-secondary">{{ $t('settings-sessions-empty') }}</p>
      </div>

      <div v-else class="flex flex-col gap-2">
        <div
          v-for="session in sessions"
          :key="session.session_id"
          class="flex items-center justify-between gap-3 p-3 bg-surface-alt rounded-lg"
        >
          <div class="flex items-center gap-3 min-w-0">
            <div class="w-10 h-10 rounded-lg bg-surface-hover flex items-center justify-center flex-shrink-0">
              <Icon name="device" size="md" class="text-secondary" />
            </div>
            <div class="min-w-0">
              <div class="text-sm font-medium text-primary truncate">
                {{ deviceLabel(session) }}
                <span
                  v-if="session.is_current"
                  class="ml-2 px-2 py-1 bg-status-success/20 text-status-success rounded text-xs"
                >
                  {{ $t('settings-sessions-current-badge') }}
                </span>
              </div>
              <div class="text-xs text-tertiary truncate">
                <span v-if="session.location">{{ session.location }} &middot; </span>
                <span v-else-if="session.ip_address">{{ session.ip_address }} &middot; </span>
                {{ $t('settings-sessions-last-active', { time: formatRelativeTime(session.last_active, { addSuffix: true }) }) }}
              </div>
              <div class="text-xs text-tertiary truncate">
                {{ $t('settings-sessions-signed-in', { date: formatDate(session.created_at) }) }}
              </div>
            </div>
          </div>

          <span v-if="session.is_current" class="text-xs text-tertiary flex-shrink-0">
            {{ $t('settings-sessions-this-device') }}
          </span>
          <Button
            v-else
            variant="ghost-danger"
            size="sm"
            class="flex-shrink-0"
            :loading="revokingId === session.session_id"
            :disabled="revokingId !== null"
            :aria-label="$t('settings-sessions-revoke-aria', { device: deviceLabel(session) })"
            @click="handleRevoke(session)"
          >
            {{ $t('settings-sessions-revoke') }}
          </Button>
        </div>
      </div>
    </div>
  </SectionCard>

  <!-- Sign out all other sessions: confirm + step-up re-auth -->
  <ConfirmModal
    :show="showRevokeAllModal"
    variant="danger"
    :title="$t('settings-sessions-revoke-others-modal-title')"
    :message="$t('settings-sessions-revoke-others-modal-description')"
    :confirm-label="$t('settings-sessions-revoke-others-confirm')"
    :cancel-label="$t('settings-sessions-modal-cancel')"
    :confirm-disabled="revokeAllConfirmDisabled || revokingAll"
    :loading="revokingAll"
    @confirm="handleRevokeAll"
    @close="closeRevokeAllModal"
  >
    <template #body>
      <div v-if="stepUpMethod !== 'none'" class="mt-3 flex flex-col gap-1.5">
        <label for="sessions-revoke-all-stepup" class="text-xs font-medium text-secondary">
          {{ stepUpMethod === 'password'
            ? $t('settings-sessions-revoke-others-stepup-password')
            : $t('settings-sessions-revoke-others-stepup-mfa') }}
        </label>
        <FormInput
          id="sessions-revoke-all-stepup"
          v-model="stepUpCredential"
          :type="stepUpMethod === 'password' ? 'password' : 'text'"
          :inputmode="stepUpMethod === 'mfa' ? 'numeric' : undefined"
          :autocomplete="stepUpMethod === 'password' ? 'current-password' : 'one-time-code'"
          @keyup.enter="handleRevokeAll"
        />
        <p v-if="modalError" class="text-sm text-status-error">{{ modalError }}</p>
      </div>
      <p v-else-if="modalError" class="mt-3 text-sm text-status-error">{{ modalError }}</p>
    </template>
  </ConfirmModal>
</template>
