<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-channels-email-title') }}</h1>
        <p class="text-secondary">
          {{ $t('admin-channels-email-description') }}
        </p>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Load error (initial fetch failed with no cached data) -->
      <AlertMessage v-if="loadError && !hasLoadedData" type="error" :message="loadError" />

      <!-- First-load skeleton: a couple of card shells mirroring the
           status + form layout. Cold cache only; revisits seed the
           form from cache instantly and revalidate silently. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-channels-email-loading')"
        class="flex flex-col gap-6"
      >
        <div
          v-for="n in 2"
          :key="n"
          class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4"
        >
          <SkeletonBar class="h-5 w-48 max-w-full" />
          <SkeletonBar class="h-4 w-full" />
          <SkeletonBar class="h-10 w-full" />
          <SkeletonBar class="h-10 w-5/6" />
        </div>
      </Skeleton>

      <div v-else-if="hasLoadedData" class="flex flex-col gap-6">
        <!-- Status card (only shown when a channel exists). -->
        <div
          v-if="channel"
          class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4"
        >
          <div class="flex items-start justify-between gap-4 flex-wrap">
            <div class="flex flex-col gap-1">
              <h2 class="text-lg font-semibold text-primary">{{ $t('admin-channels-email-status-heading') }}</h2>
              <p class="text-sm text-secondary">
                {{ $t('admin-channels-email-status-subtitle') }}
              </p>
            </div>
            <div class="flex items-center gap-2">
              <span
                class="inline-flex items-center gap-1.5 text-xs font-medium px-2.5 py-1 rounded-full"
                :class="
                  channel.enabled
                    ? 'bg-status-success-bg text-status-success'
                    : 'bg-status-muted-bg text-status-muted'
                "
              >
                <span
                  class="inline-block w-1.5 h-1.5 rounded-full"
                  :class="channel.enabled ? 'bg-status-success' : 'bg-status-muted'"
                ></span>
                {{ channel.enabled ? $t('admin-channels-email-status-enabled') : $t('admin-channels-email-status-disabled') }}
              </span>
            </div>
          </div>

          <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div class="flex flex-col gap-1">
              <span class="text-xs uppercase tracking-wide text-tertiary">{{ $t('admin-channels-email-status-last-polled') }}</span>
              <span class="text-sm text-primary">
                {{ channel.last_polled_at ? formatRelativeTime(channel.last_polled_at) : $t('admin-channels-email-status-never') }}
              </span>
            </div>
            <div class="flex flex-col gap-1">
              <span class="text-xs uppercase tracking-wide text-tertiary">{{ $t('admin-channels-email-status-last-uid') }}</span>
              <span class="text-sm text-primary font-mono">
                {{ runtimeState.last_seen_uid ?? 0 }}
              </span>
            </div>
            <div class="flex flex-col gap-1">
              <span class="text-xs uppercase tracking-wide text-tertiary">{{ $t('admin-channels-email-status-uid-validity') }}</span>
              <span class="text-sm text-primary font-mono">
                {{ runtimeState.uid_validity ?? '-' }}
              </span>
            </div>
          </div>

          <div
            v-if="runtimeState.last_error"
            class="bg-status-error-bg border border-status-error-border rounded-lg p-3 flex flex-col gap-1"
          >
            <span class="text-xs uppercase tracking-wide text-status-error font-semibold">
              {{ $t('admin-channels-email-status-last-error') }}
            </span>
            <span class="text-sm text-status-error font-mono break-words">
              {{ runtimeState.last_error }}
            </span>
            <span class="text-xs text-status-error">
              {{ $t('admin-channels-email-status-last-error-hint') }}
            </span>
          </div>
        </div>

        <!-- Config form. Same surface for create + edit; the button at the
             bottom differs. -->
        <form
          class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6"
          @submit.prevent="save"
        >
          <div class="flex flex-col gap-1">
            <h2 class="text-lg font-semibold text-primary">
              {{ channel ? $t('admin-channels-email-form-heading-edit') : $t('admin-channels-email-form-heading-create') }}
            </h2>
            <p class="text-sm text-secondary">
              {{ $t('admin-channels-email-form-subtitle') }}
            </p>
          </div>

          <ToggleSwitch
            v-if="channel"
            v-model="form.enabled"
            :label="$t('admin-channels-email-toggle-enabled-label')"
            :description="$t('admin-channels-email-toggle-enabled-description')"
          />

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="channel-name" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-name-label') }}
              </label>
              <input
                id="channel-name"
                v-model="form.name"
                type="text"
                :placeholder="$t('admin-channels-email-field-name-placeholder')"
                required
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">
                {{ $t('admin-channels-email-field-name-hint') }}
              </p>
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-host" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-host-label') }}
              </label>
              <input
                id="channel-host"
                v-model="form.host"
                type="text"
                :placeholder="$t('admin-channels-email-field-host-placeholder')"
                required
                autocomplete="off"
                :class="inputClasses"
              />
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-port" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-port-label') }}
              </label>
              <input
                id="channel-port"
                v-model.number="form.port"
                type="number"
                min="1"
                max="65535"
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">{{ $t('admin-channels-email-field-port-hint') }}</p>
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-username" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-username-label') }}
              </label>
              <input
                id="channel-username"
                v-model="form.username"
                type="text"
                :placeholder="$t('admin-channels-email-field-username-placeholder')"
                required
                autocomplete="off"
                :class="inputClasses"
              />
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-mailbox" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-mailbox-label') }}
              </label>
              <input
                id="channel-mailbox"
                v-model="form.mailbox"
                type="text"
                :placeholder="$t('admin-channels-email-field-mailbox-placeholder')"
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">{{ $t('admin-channels-email-field-mailbox-hint') }}</p>
            </div>

            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="channel-reply-domain" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-reply-domain-label') }}
              </label>
              <input
                id="channel-reply-domain"
                v-model="form.reply_domain"
                type="text"
                :placeholder="$t('admin-channels-email-field-reply-domain-placeholder')"
                required
                autocomplete="off"
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">
                {{ $t('admin-channels-email-field-reply-domain-hint') }}
              </p>
            </div>

            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="channel-password" class="text-sm font-medium text-primary">
                {{ $t('admin-channels-email-field-password-label') }}
                <span v-if="channel?.has_credential" class="text-tertiary font-normal">
                  {{ $t('admin-channels-email-field-password-keep-existing') }}
                </span>
              </label>
              <input
                id="channel-password"
                v-model="form.password"
                type="password"
                autocomplete="new-password"
                :placeholder="channel?.has_credential ? $t('admin-channels-email-field-password-placeholder-stored') : $t('admin-channels-email-field-password-placeholder-new')"
                :class="inputClasses"
              />
              <div v-if="channel?.has_credential" class="flex items-center gap-4 mt-1">
                <Button variant="ghost-danger" size="sm" :loading="clearing" @click="clearCredential">
                  {{ clearing ? $t('admin-channels-email-removing-password') : $t('admin-channels-email-remove-password') }}
                </Button>
              </div>
            </div>
          </div>

          <!-- Advanced / dev options -->
          <details class="border-t border-default pt-4">
            <summary class="cursor-pointer text-sm text-secondary hover:text-primary">
              {{ $t('admin-channels-email-advanced') }}
            </summary>
            <div class="pt-4 flex flex-col gap-3">
              <ToggleSwitch
                v-model="form.insecure_skip_cert_verify"
                :label="$t('admin-channels-email-toggle-insecure-label')"
                :description="$t('admin-channels-email-toggle-insecure-description')"
              />
            </div>
          </details>

          <div class="flex items-center justify-between gap-4 flex-wrap border-t border-default pt-4">
            <div class="flex items-center gap-3">
              <Button
                v-if="channel"
                variant="secondary"
                :loading="testing"
                :disabled="!canTest"
                :title="formIsDirty ? $t('admin-channels-email-test-dirty-hint') : undefined"
                @click="testConnection"
              >
                {{ testing ? $t('admin-channels-email-testing') : $t('admin-channels-email-test') }}
              </Button>
              <span
                v-if="formIsDirty && channel"
                class="text-sm text-tertiary"
              >
                {{ $t('admin-channels-email-test-dirty-hint') }}
              </span>
              <span v-else-if="testResult === 'ok'" class="text-sm text-status-success inline-flex items-center gap-1.5">
                <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-success"></span>
                {{ $t('admin-channels-email-test-connected') }}
              </span>
              <span
                v-else-if="testResult === 'failed'"
                class="text-sm text-status-error inline-flex items-center gap-1.5"
                :title="testErrorMessage"
              >
                <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-error"></span>
                {{ testErrorMessage || $t('admin-channels-email-test-failed') }}
              </span>
            </div>
            <div class="flex items-center gap-3">
              <Button
                v-if="channel"
                variant="ghost-danger"
                :loading="deleting"
                @click="confirmAndDelete"
              >
                {{ deleting ? $t('admin-channels-email-deleting') : $t('admin-channels-email-delete') }}
              </Button>
              <Button type="submit" :loading="saving" :disabled="!canSave">
                {{ submitLabel }}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>

    <ConfirmModal
      :show="showClearCredentialConfirm"
      variant="danger"
      :title="$t('admin-channels-email-clear-credential-title')"
      :message="$t('admin-channels-email-clear-credential-message')"
      :confirm-label="$t('admin-channels-email-clear-credential-confirm')"
      @confirm="doClearCredential"
      @close="showClearCredentialConfirm = false"
    />

    <ConfirmModal
      :show="showDeleteChannelConfirm"
      variant="danger"
      :title="$t('admin-channels-email-delete-title')"
      :message="$t('admin-channels-email-delete-message')"
      :confirm-label="$t('admin-channels-email-delete-confirm')"
      @confirm="doDeleteChannel"
      @close="showDeleteChannelConfirm = false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import {
  channelsService,
  type Channel,
  type ImapChannelConfig,
  type ImapRuntimeState
} from '@/services/channelsService';
import { createErrorFromResponse } from '@/utils/errors';
import { formatRelativeTime } from '@/utils/dateUtils';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const EMAIL_PROVIDER = 'email_imap';

/** Matches the backend's `ImapChannelConfig` defaults. */
const DEFAULT_CONFIG = {
  port: 993,
  mailbox: 'INBOX',
  use_tls: true,
  insecure_skip_cert_verify: false
};

interface FormState {
  name: string;
  enabled: boolean;
  host: string;
  port: number;
  username: string;
  mailbox: string;
  reply_domain: string;
  password: string;
  insecure_skip_cert_verify: boolean;
}

function emptyForm(): FormState {
  return {
    name: '',
    enabled: true,
    host: '',
    port: DEFAULT_CONFIG.port,
    username: '',
    mailbox: DEFAULT_CONFIG.mailbox,
    reply_domain: '',
    password: '',
    insecure_skip_cert_verify: false
  };
}

// The email channel config is cached by Pinia Colada so revisits
// render the form instantly from cache and revalidate silently in
// the background. The skeleton only shows on a genuine cold cache.
const CHANNELS_EMAIL_KEY = ['channels-email-config'] as const;
const queryCache = useQueryCache();
const channelQuery = useQuery({
  key: CHANNELS_EMAIL_KEY,
  query: async () => {
    const list = await channelsService.list();
    return list.find((c) => c.provider === EMAIL_PROVIDER) ?? null;
  },
});
const channel = computed<Channel | null>(() => channelQuery.data.value ?? null);
const isFirstLoad = computed(
  () => channelQuery.status.value === 'pending' && channelQuery.data.value === undefined,
);
// Surface backend detail when available; fall back to a generic
// message only when the error doesn't carry one (e.g. network drop).
const loadError = computed(() => {
  const err = channelQuery.error.value;
  if (!err) return '';
  return createErrorFromResponse(err).getUserMessage() || t('admin-channels-email-error-load');
});
// Distinguishes a genuine fetch failure (no data ever received) from
// a successful "no email channel configured" result, which is null.
const hasLoadedData = computed(() => channelQuery.data.value !== undefined);

const saving = ref(false);
const testing = ref(false);
const deleting = ref(false);
const clearing = ref(false);
const form = ref<FormState>(emptyForm());
const testResult = ref<'idle' | 'ok' | 'failed'>('idle');
const testErrorMessage = ref('');
const successMessage = ref('');
const errorMessage = ref('');

// Seed the editable form once per component lifetime from the
// cached query. Subsequent background revalidations are ignored so
// they can't clobber in-progress edits. On nav-back the component
// remounts, `seeded` resets, and the immediate watch reseeds.
const seeded = ref(false);
watch(
  channelQuery.data,
  (data) => {
    if (data === undefined || seeded.value) return;
    if (data) populateForm(data);
    seeded.value = true;
  },
  { immediate: true },
);

// Mirrors the FormInput field styling so these (label-associated, number,
// and password) inputs match the shared primitive's look + focus ring.
const inputClasses =
  'w-full bg-surface-alt border border-subtle rounded-lg px-3 py-2 text-primary placeholder-tertiary transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent';

const runtimeState = computed<ImapRuntimeState>(() => {
  if (!channel.value) return {};
  return (channel.value.runtime_state ?? {}) as ImapRuntimeState;
});

// Password isn't part of the save-validity gate: creating without one
// is allowed (admin sets it later) and editing without one preserves
// the stored secret. See `save()` for the "send-only-if-non-empty" rule.
const canSave = computed(() => {
  const f = form.value;
  return (
    f.name.trim().length > 0 &&
    f.host.trim().length > 0 &&
    f.username.trim().length > 0 &&
    f.reply_domain.trim().length > 0
  );
});

// Test-connection is enabled when we have either a candidate password
// on the form or a stored one on the channel. Without either there's
// nothing to authenticate with. We also disable it when the form is
// dirty against the saved config: the test endpoint authenticates
// against the *stored* settings, so testing unsaved edits would
// silently check the wrong host/port and confuse the admin.
const formIsDirty = computed(() => {
  const ch = channel.value;
  if (!ch) return false;
  const cfg = (ch.config ?? {}) as unknown as ImapChannelConfig;
  const f = form.value;
  return (
    f.name !== ch.name ||
    f.enabled !== ch.enabled ||
    f.host !== (cfg.host ?? '') ||
    f.port !== (cfg.port ?? DEFAULT_CONFIG.port) ||
    f.username !== (cfg.username ?? '') ||
    f.mailbox !== (cfg.mailbox ?? DEFAULT_CONFIG.mailbox) ||
    f.reply_domain !== (cfg.reply_domain ?? '') ||
    f.insecure_skip_cert_verify !== (cfg.insecure_skip_cert_verify ?? false)
  );
});
const canTest = computed(() => {
  if (formIsDirty.value) return false;
  return form.value.password.length > 0 || (channel.value?.has_credential ?? false);
});

// Any edit invalidates a previous test result. The green "Connected"
// pip would otherwise survive an admin changing the host away from
// the value that actually authenticated.
watch(
  () => {
    const f = form.value;
    return [
      f.host,
      f.port,
      f.username,
      f.mailbox,
      f.reply_domain,
      f.password,
      f.insecure_skip_cert_verify,
    ];
  },
  () => {
    testResult.value = 'idle';
    testErrorMessage.value = '';
  },
);

/**
 * Show a success message and auto-clear it after 3s. The `=== msg`
 * guard prevents a later success from being cleared by an earlier
 * timer, which would happen if two saves landed within the window.
 */
function flashSuccess(key: string) {
  const msg = t(key);
  successMessage.value = msg;
  setTimeout(() => {
    if (successMessage.value === msg) successMessage.value = '';
  }, 3000);
}

const submitLabel = computed(() => {
  if (saving.value) {
    return channel.value
      ? t('admin-channels-email-saving')
      : t('admin-channels-email-creating');
  }
  return channel.value ? t('admin-channels-email-save') : t('admin-channels-email-create');
});

function populateForm(ch: Channel) {
  const cfg = (ch.config ?? {}) as unknown as ImapChannelConfig;
  form.value = {
    name: ch.name,
    enabled: ch.enabled,
    host: cfg.host ?? '',
    port: cfg.port ?? DEFAULT_CONFIG.port,
    username: cfg.username ?? '',
    mailbox: cfg.mailbox ?? DEFAULT_CONFIG.mailbox,
    reply_domain: cfg.reply_domain ?? '',
    insecure_skip_cert_verify: cfg.insecure_skip_cert_verify ?? false,
    password: '',
  };
}

// Typed locally, then widened to the `Record<string, unknown>` shape the
// service accepts. The channels endpoint is generic over providers and
// only `email_imap`'s shape is validated server-side.
function buildConfig(): Record<string, unknown> {
  const f = form.value;
  const cfg: ImapChannelConfig = {
    host: f.host.trim(),
    port: f.port,
    username: f.username.trim(),
    mailbox: f.mailbox.trim() || DEFAULT_CONFIG.mailbox,
    use_tls: DEFAULT_CONFIG.use_tls,
    reply_domain: f.reply_domain.trim(),
    insecure_skip_cert_verify: f.insecure_skip_cert_verify
  };
  return cfg as unknown as Record<string, unknown>;
}

function clearMessages() {
  successMessage.value = '';
  errorMessage.value = '';
}

async function save() {
  clearMessages();
  testResult.value = 'idle';
  if (!canSave.value) return;
  saving.value = true;
  const f = form.value;
  try {
    if (channel.value) {
      const updated = await channelsService.update(channel.value.id, {
        name: f.name.trim(),
        enabled: f.enabled,
        config: buildConfig(),
        // Only send password when the admin typed one. An empty string
        // must not nuke the stored secret. See channelsService's
        // `UpdateChannelRequest` contract.
        ...(f.password.length > 0 ? { password: f.password } : {})
      });
      // Keep the cache in lockstep so a later revisit shows the saved
      // values without a network round-trip.
      queryCache.setQueryData(CHANNELS_EMAIL_KEY, updated);
      populateForm(updated);
      flashSuccess('admin-channels-email-success-update');
    } else {
      const created = await channelsService.create({
        provider: EMAIL_PROVIDER,
        name: f.name.trim(),
        enabled: f.enabled,
        config: buildConfig(),
        ...(f.password.length > 0 ? { password: f.password } : {})
      });
      queryCache.setQueryData(CHANNELS_EMAIL_KEY, created);
      populateForm(created);
      flashSuccess('admin-channels-email-success-create');
    }
  } catch (e: unknown) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage();
  } finally {
    saving.value = false;
  }
}

async function testConnection() {
  if (!channel.value || !canTest.value) return;
  clearMessages();
  testing.value = true;
  testResult.value = 'idle';
  testErrorMessage.value = '';
  try {
    const result = await channelsService.testConnection(
      channel.value.id,
      form.value.password.length > 0 ? form.value.password : undefined
    );
    if (result.ok) {
      testResult.value = 'ok';
    } else {
      testResult.value = 'failed';
      testErrorMessage.value = result.error ?? t('admin-channels-email-test-unknown-error');
    }
  } catch (e: unknown) {
    testResult.value = 'failed';
    testErrorMessage.value = createErrorFromResponse(e).getUserMessage();
  } finally {
    testing.value = false;
  }
}

const showClearCredentialConfirm = ref(false);
const showDeleteChannelConfirm = ref(false);

function clearCredential() {
  if (!channel.value) return;
  showClearCredentialConfirm.value = true;
}

async function doClearCredential() {
  showClearCredentialConfirm.value = false;
  if (!channel.value) return;
  clearMessages();
  clearing.value = true;
  try {
    await channelsService.clearCredential(channel.value.id);
    await queryCache.invalidateQueries({ key: CHANNELS_EMAIL_KEY });
    flashSuccess('admin-channels-email-success-password-removed');
  } catch (e: unknown) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage();
  } finally {
    clearing.value = false;
  }
}

function confirmAndDelete() {
  if (!channel.value) return;
  showDeleteChannelConfirm.value = true;
}

async function doDeleteChannel() {
  showDeleteChannelConfirm.value = false;
  if (!channel.value) return;
  clearMessages();
  deleting.value = true;
  try {
    await channelsService.remove(channel.value.id);
    queryCache.setQueryData(CHANNELS_EMAIL_KEY, null);
    form.value = emptyForm();
    flashSuccess('admin-channels-email-success-delete');
  } catch (e: unknown) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage();
  } finally {
    deleting.value = false;
  }
}

</script>
