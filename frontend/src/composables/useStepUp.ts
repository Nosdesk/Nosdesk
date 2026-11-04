import { computed, onScopeDispose, ref, watch } from 'vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { useFluent } from 'fluent-vue';
import {
  STEP_UP_CHANNEL,
  STEP_UP_POPUP_FEATURES,
  STEP_UP_POPUP_NAME,
  STEP_UP_POPUP_RESULTS,
  STEP_UP_PROVIDER_URL,
  type StepUpChannelMessage,
  type StepUpCredential,
  type StepUpMethod,
  type StepUpPopupResult,
  type StepUpStatus,
} from '@/services/stepUpService';
import passkeyService from '@/services/passkeyService';
import { useAccountApi } from '@/composables/useAccountApi';
import { extractErrorMessage } from '@/utils/errors';

export const STEP_UP_KEY = ['auth', 'step-up'] as const;

/** How often to ask the server while the sign-in window is open. */
export const STEP_UP_POLL_MS = 2000;
/** After this long, ask less often: the popup is still open, but slowly. */
export const STEP_UP_SLOW_AFTER_MS = 5 * 60 * 1000;
/** How often to ask once the popup has been open a while. */
export const STEP_UP_SLOW_POLL_MS = 15_000;
/**
 * How long to wait for the sign-in window: as long as the sign-in it started
 * stays valid on the server (`OAUTH_STATE_LIFETIME`), since a first sign-in
 * can mean a password change and MFA set-up.
 */
export const STEP_UP_WAIT_MS = 30 * 60 * 1000;

/**
 * "Confirm it's you" for the signed-in session, ahead of a high-risk action.
 *
 * Reads the session's confirmation status (fresh on every mount, so a stale
 * "confirmed" from an earlier dialog is never trusted) and confirms with
 * whichever method the server offers: a password or code typed here, one of
 * the user's passkeys, or a fresh sign-in at their identity provider in a
 * popup. `confirmed` follows `confirmed_until` and flips back when it passes.
 *
 * The popup cannot report back through `window.opener` (the app sends COOP
 * same-origin, which cuts the link), so while it is open this listens on a
 * BroadcastChannel that `/auth/confirmed` posts to, re-checks on window focus,
 * and polls the status as a backstop.
 */
export function useStepUp() {
  const fluent = useFluent();
  const queryCache = useQueryCache();
  const account = useAccountApi();

  const query = useQuery({
    key: STEP_UP_KEY,
    query: () => account.stepUp.status(),
    staleTime: 0,
  });

  const status = computed<StepUpStatus | undefined>(() => query.data.value);
  /** True until the first status arrives; callers keep their button off. */
  const loading = computed(() => status.value === undefined && query.status.value !== 'error');
  const loadFailed = computed(() => status.value === undefined && query.status.value === 'error');

  // Read once: support does not change under a mounted dialog, and the check
  // logs when it is missing.
  const passkeysSupported = passkeyService.isSupported();
  const methods = computed<StepUpMethod[]>(() =>
    (status.value?.methods ?? []).filter((m) => m !== 'passkey' || passkeysSupported),
  );
  const provider = computed(() => status.value?.provider ?? null);
  const email = computed(() => status.value?.email ?? null);
  const windowMinutes = computed(() => status.value?.window_minutes ?? 10);

  // --- Confirmed, and its expiry ---
  const now = ref(Date.now());
  const confirmedUntilMs = computed(() => {
    const until = status.value?.confirmed_until;
    if (!until) return null;
    const ms = Date.parse(until);
    return Number.isNaN(ms) ? null : ms;
  });
  const confirmed = computed(() => confirmedUntilMs.value !== null && confirmedUntilMs.value > now.value);

  let expiryTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleExpiry() {
    if (expiryTimer) clearTimeout(expiryTimer);
    expiryTimer = null;
    now.value = Date.now();
    const until = confirmedUntilMs.value;
    if (until !== null && until > now.value) {
      expiryTimer = setTimeout(scheduleExpiry, until - now.value + 50);
    }
  }
  watch(confirmedUntilMs, scheduleExpiry, { immediate: true });

  function setConfirmedUntil(until: string) {
    const current = queryCache.getQueryData<StepUpStatus>(STEP_UP_KEY);
    if (current) {
      queryCache.setQueryData<StepUpStatus>(STEP_UP_KEY, { ...current, confirmed_until: until });
    }
  }

  async function refresh() {
    await query.refetch();
    scheduleExpiry();
  }

  // --- Typed credential and passkey ---
  const busy = ref(false);
  /** The server's answer to the last credential or passkey attempt. */
  const error = ref<string | null>(null);
  /** How the last sign-in popup ended, when it was not a success. */
  const popupResult = ref<Exclude<StepUpPopupResult, 'ok'> | null>(null);

  function failed(): string {
    return fluent.$t('step-up-result-failed');
  }

  async function confirmWithCredential(credential: StepUpCredential): Promise<boolean> {
    busy.value = true;
    error.value = null;
    popupResult.value = null;
    try {
      const result = await account.stepUp.confirm(credential);
      setConfirmedUntil(result.confirmed_until);
      return true;
    } catch (err) {
      error.value = extractErrorMessage(err, failed());
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function confirmWithPasskey(): Promise<boolean> {
    busy.value = true;
    error.value = null;
    popupResult.value = null;
    try {
      const result = await account.stepUp.confirmWithPasskey();
      setConfirmedUntil(result.confirmed_until);
      return true;
    } catch (err) {
      // Dismissing the browser's passkey sheet is a choice, not a failure.
      const name = (err as { name?: string } | null)?.name;
      if (name !== 'NotAllowedError' && name !== 'AbortError') {
        error.value = (err as { response?: unknown })?.response ? extractErrorMessage(err, failed()) : failed();
      }
      return false;
    } finally {
      busy.value = false;
    }
  }

  // --- Sign in again at the identity provider, in a popup ---
  const waiting = ref(false);

  let channel: BroadcastChannel | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let deadlineTimer: ReturnType<typeof setTimeout> | null = null;

  const onFocus = () => void refresh();

  function stopWaiting() {
    waiting.value = false;
    channel?.close();
    channel = null;
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = null;
    if (deadlineTimer) clearTimeout(deadlineTimer);
    deadlineTimer = null;
    window.removeEventListener('focus', onFocus);
  }

  async function onPopupResult(result: StepUpPopupResult) {
    if (!waiting.value) return;
    if (result === 'ok') {
      await refresh();
      if (!confirmed.value) {
        stopWaiting();
        popupResult.value = 'failed';
      }
      return;
    }
    stopWaiting();
    popupResult.value = result;
  }

  /** The sign-in window's flow has lapsed, so it can no longer confirm. */
  function giveUp() {
    stopWaiting();
    popupResult.value = 'failed';
  }

  function startWaiting() {
    stopWaiting();
    waiting.value = true;
    const startedAt = Date.now();
    const poll = () => {
      void refresh();
      const slow = Date.now() - startedAt >= STEP_UP_SLOW_AFTER_MS;
      pollTimer = setTimeout(poll, slow ? STEP_UP_SLOW_POLL_MS : STEP_UP_POLL_MS);
    };
    if (typeof BroadcastChannel !== 'undefined') {
      channel = new BroadcastChannel(STEP_UP_CHANNEL);
      channel.onmessage = (event: MessageEvent<StepUpChannelMessage>) => {
        const data = event.data;
        if (data?.type === 'step-up' && STEP_UP_POPUP_RESULTS.includes(data.result)) {
          void onPopupResult(data.result);
        }
      };
    }
    window.addEventListener('focus', onFocus);
    pollTimer = setTimeout(poll, STEP_UP_POLL_MS);
    deadlineTimer = setTimeout(giveUp, STEP_UP_WAIT_MS);
  }

  /**
   * Opens the provider sign-in. Call it straight from the click handler, with
   * nothing awaited first, or the browser blocks the popup.
   */
  function openProviderPopup() {
    window.open(STEP_UP_PROVIDER_URL, STEP_UP_POPUP_NAME, STEP_UP_POPUP_FEATURES);
    error.value = null;
    popupResult.value = null;
    startWaiting();
  }

  /** However it was confirmed (poll, focus, the channel), stop waiting. */
  watch(confirmed, (isConfirmed) => {
    if (isConfirmed && waiting.value) stopWaiting();
  });

  function clearMessages() {
    error.value = null;
    popupResult.value = null;
  }

  onScopeDispose(() => {
    stopWaiting();
    if (expiryTimer) clearTimeout(expiryTimer);
  });

  return {
    loading,
    loadFailed,
    confirmed,
    methods,
    provider,
    email,
    windowMinutes,
    busy,
    error,
    waiting,
    popupResult,
    refresh,
    confirmWithCredential,
    confirmWithPasskey,
    openProviderPopup,
    cancelWaiting: stopWaiting,
    clearMessages,
  };
}
