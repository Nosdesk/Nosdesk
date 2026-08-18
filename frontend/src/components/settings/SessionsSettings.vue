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

Device labels are built here, not server-side, so they can be
translated. Native clients (which know their real device) send a name
the backend stores and this list prefers; browsers send none.
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

// The backend orders by last activity. Pin the caller's own session to the
// top: it's the one they need to identify first when deciding what to revoke,
// and it's the only row without a Sign out button.
const sessions = computed<SessionInfo[]>(() =>
  [...(sessionsQuery.data.value ?? [])].sort(
    (a, b) => Number(b.is_current) - Number(a.is_current),
  ),
);
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
 * Coarse OS family from a user-agent, shared by the label and the icon so the
 * two can never disagree. Deliberately minimal: full UA parsing is not worth a
 * dependency for a settings list, and modern user-agents are frozen anyway.
 */
function osFamily(ua: string): 'windows' | 'ios' | 'macos' | 'android' | 'linux' | null {
  if (/Windows/i.test(ua)) return 'windows';
  if (/iPhone|iPad|iOS/i.test(ua)) return 'ios';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'macos';
  if (/Android/i.test(ua)) return 'android';
  if (/Linux/i.test(ua)) return 'linux';
  return null;
}

const OS_LABELS: Record<NonNullable<ReturnType<typeof osFamily>>, string> = {
  windows: 'Windows',
  ios: 'iOS',
  macos: 'macOS',
  android: 'Android',
  linux: 'Linux',
};

/**
 * Human label for a session row. Native clients send their own name, which
 * always wins; browsers send none, so a "Browser on OS" label is derived here
 * where it can be translated. The backend deliberately stores no label of its
 * own.
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
  const family = osFamily(ua);
  const os = family ? OS_LABELS[family] : null;
  if (browser && os) return t('settings-sessions-device-label', { browser, os });
  return browser ?? os ?? t('settings-sessions-unknown-device');
}

/**
 * How close to expiry a session has to be before the row says so. Sessions
 * carry two clocks: a sliding 7-day idle window and a hard ceiling measured
 * from sign-in. `expires_at` is whichever comes first, so a session in regular
 * use always sits a full week out and never trips this. The warning therefore
 * only appears when the session really is about to lapse, from disuse or from
 * reaching the ceiling.
 */
const EXPIRY_WARNING_DAYS = 3;

function expiringSoon(session: SessionInfo): boolean {
  const remaining = new Date(session.expires_at).getTime() - Date.now();
  return remaining > 0 && remaining <= EXPIRY_WARNING_DAYS * 24 * 60 * 60 * 1000;
}

/**
 * Icon matching the device kind, so a phone session doesn't show a desktop
 * monitor. Falls back to the generic endpoint icon when the platform is
 * unknown, including for named native sessions we can't classify.
 */
function deviceIcon(session: SessionInfo): 'phone' | 'laptop' | 'device' {
  const hint = `${session.device_name ?? ''} ${session.user_agent ?? ''}`;
  const family = osFamily(hint);
  if (family === 'ios' || family === 'android') return 'phone';
  if (family === 'macos') return 'laptop';
  return 'device';
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

      <div v-else class="flex flex-col gap-4">
        <div
          v-for="session in sessions"
          :key="session.session_id"
          class="flex items-center justify-between gap-3 p-4 bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors"
        >
          <div class="flex items-center gap-4 min-w-0">
            <div class="w-10 h-10 rounded-full bg-accent/10 flex items-center justify-center flex-shrink-0">
              <Icon :name="deviceIcon(session)" size="md" class="text-accent" />
            </div>
            <div class="min-w-0">
              <!-- wrap so the badge drops below the name at phone widths
                   rather than squeezing it: the current session is the row
                   the user most needs to read in full. -->
              <div class="flex items-center gap-2 flex-wrap">
                <p class="text-sm font-medium text-primary truncate">{{ deviceLabel(session) }}</p>
                <span
                  v-if="session.is_current"
                  class="inline-flex items-center gap-1 text-xs text-status-success flex-shrink-0"
                >
                  <Icon name="check" size="xs" />
                  {{ $t('settings-sessions-current-badge') }}
                </span>
              </div>
              <!-- Recency leads, because it's what tells the user whether a
                   row is them, and this line wraps instead of truncating: at
                   phone widths a clipped "Activ…" loses exactly the token
                   they came for. Location follows, so it's the part pushed to
                   a second line. -->
              <p class="text-xs text-tertiary mt-0.5">
                {{ $t('settings-sessions-last-active', { time: formatRelativeTime(session.last_active, { addSuffix: true }) }) }}
                <!-- nowrap keeps the separator attached to what follows it,
                     so a wrap moves "· Sydney" down whole instead of
                     stranding the dot at the end of the line above. -->
                <span v-if="session.location || session.ip_address" class="whitespace-nowrap">
                  &middot; {{ session.location ?? session.ip_address }}
                </span>
              </p>
              <p class="text-xs text-tertiary truncate mt-0.5">
                {{ $t('settings-sessions-signed-in', { date: formatDate(session.created_at) }) }}
              </p>
              <p
                v-if="expiringSoon(session)"
                class="inline-flex items-center gap-1 text-xs text-status-warning mt-0.5"
              >
                <Icon name="warning" size="xs" class="flex-shrink-0" />
                {{ $t('settings-sessions-expires-soon', { time: formatRelativeTime(session.expires_at, { addSuffix: true }) }) }}
              </p>
            </div>
          </div>

          <Button
            v-if="!session.is_current"
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
