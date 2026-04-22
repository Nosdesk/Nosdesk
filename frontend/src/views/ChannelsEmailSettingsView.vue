<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Email Ingestion</h1>
        <p class="text-secondary">
          Poll a support mailbox over IMAP and turn inbound messages into tickets.
          Replies from techs are relayed back through the same thread.
        </p>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <LoadingSpinner v-if="loading" text="Loading channel..." />

      <div v-else class="flex flex-col gap-6">
        <!-- Status card (only shown when a channel exists). -->
        <div
          v-if="channel"
          class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4"
        >
          <div class="flex items-start justify-between gap-4 flex-wrap">
            <div class="flex flex-col gap-1">
              <h2 class="text-lg font-semibold text-primary">Status</h2>
              <p class="text-sm text-secondary">
                Live view of what the ingestion worker last did.
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
                {{ channel.enabled ? 'Enabled' : 'Disabled' }}
              </span>
            </div>
          </div>

          <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div class="flex flex-col gap-1">
              <span class="text-xs uppercase tracking-wide text-tertiary">Last polled</span>
              <span class="text-sm text-primary">
                {{ channel.last_polled_at ? formatRelative(channel.last_polled_at) : 'never' }}
              </span>
            </div>
            <div class="flex flex-col gap-1">
              <span class="text-xs uppercase tracking-wide text-tertiary">Last seen UID</span>
              <span class="text-sm text-primary font-mono">
                {{ runtimeState.last_seen_uid ?? 0 }}
              </span>
            </div>
            <div class="flex flex-col gap-1">
              <span class="text-xs uppercase tracking-wide text-tertiary">UIDVALIDITY</span>
              <span class="text-sm text-primary font-mono">
                {{ runtimeState.uid_validity ?? '—' }}
              </span>
            </div>
          </div>

          <div
            v-if="runtimeState.last_error"
            class="bg-status-error-bg border border-status-error-border rounded-lg p-3 flex flex-col gap-1"
          >
            <span class="text-xs uppercase tracking-wide text-status-error font-semibold">
              Last error
            </span>
            <span class="text-sm text-status-error font-mono break-words">
              {{ runtimeState.last_error }}
            </span>
            <span class="text-xs text-status-error">
              The worker will keep retrying with exponential backoff. Fix the
              underlying issue and it'll clear on the next successful poll.
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
              {{ channel ? 'Configuration' : 'Connect a mailbox' }}
            </h2>
            <p class="text-sm text-secondary">
              IMAP over TLS only. For self-hosted test servers with a
              self-signed cert, see the advanced toggle below.
            </p>
          </div>

          <ToggleSwitch
            v-if="channel"
            label="Enabled"
            description="When off, the worker stops polling but stored config + credentials are preserved."
            :model-value="form.enabled"
            @update:model-value="form.enabled = $event"
          />

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="channel-name" class="text-sm font-medium text-primary">
                Display name
              </label>
              <input
                id="channel-name"
                v-model="form.name"
                type="text"
                placeholder="e.g. Support Inbox"
                required
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">
                Only shown in the admin UI. Customers never see it.
              </p>
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-host" class="text-sm font-medium text-primary">
                IMAP host
              </label>
              <input
                id="channel-host"
                v-model="form.host"
                type="text"
                placeholder="imap.example.com"
                required
                autocomplete="off"
                :class="inputClasses"
              />
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-port" class="text-sm font-medium text-primary">
                Port
              </label>
              <input
                id="channel-port"
                v-model.number="form.port"
                type="number"
                min="1"
                max="65535"
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">993 for IMAPS. 143 requires STARTTLS (not supported yet).</p>
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-username" class="text-sm font-medium text-primary">
                Username
              </label>
              <input
                id="channel-username"
                v-model="form.username"
                type="text"
                placeholder="support@example.com"
                required
                autocomplete="off"
                :class="inputClasses"
              />
            </div>

            <div class="flex flex-col gap-2">
              <label for="channel-mailbox" class="text-sm font-medium text-primary">
                Mailbox
              </label>
              <input
                id="channel-mailbox"
                v-model="form.mailbox"
                type="text"
                placeholder="INBOX"
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">Gmail users may want "[Gmail]/All Mail".</p>
            </div>

            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="channel-reply-domain" class="text-sm font-medium text-primary">
                Reply domain
              </label>
              <input
                id="channel-reply-domain"
                v-model="form.reply_domain"
                type="text"
                placeholder="example.com"
                required
                autocomplete="off"
                :class="inputClasses"
              />
              <p class="text-xs text-tertiary">
                Used when we stamp Message-IDs on outbound replies so the
                customer's reply threads back to the same ticket. Usually
                the same domain as the username.
              </p>
            </div>

            <div class="flex flex-col gap-2 md:col-span-2">
              <label for="channel-password" class="text-sm font-medium text-primary">
                Password
                <span v-if="channel?.has_credential" class="text-tertiary font-normal">
                  (leave blank to keep existing)
                </span>
              </label>
              <input
                id="channel-password"
                v-model="form.password"
                type="password"
                autocomplete="new-password"
                :placeholder="channel?.has_credential ? '•••••••••• (stored)' : 'App password or account password'"
                :class="inputClasses"
              />
              <div v-if="channel?.has_credential" class="flex items-center gap-4 mt-1">
                <button
                  type="button"
                  class="text-xs text-status-error hover:underline"
                  :disabled="clearing"
                  @click="clearCredential"
                >
                  {{ clearing ? 'Removing...' : 'Remove stored password' }}
                </button>
              </div>
            </div>
          </div>

          <!-- Advanced / dev options -->
          <details class="border-t border-default pt-4">
            <summary class="cursor-pointer text-sm text-secondary hover:text-primary">
              Advanced
            </summary>
            <div class="pt-4 flex flex-col gap-3">
              <ToggleSwitch
                label="Skip TLS certificate verification"
                description="ONLY for Greenmail or self-hosted test servers with a self-signed cert. Leave off in production."
                :model-value="form.insecure_skip_cert_verify"
                @update:model-value="form.insecure_skip_cert_verify = $event"
              />
            </div>
          </details>

          <div class="flex items-center justify-between gap-4 flex-wrap border-t border-default pt-4">
            <div class="flex items-center gap-3">
              <button
                v-if="channel"
                type="button"
                class="px-4 py-2 bg-surface-alt border border-default rounded-lg hover:border-strong text-sm font-medium text-primary disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
                :disabled="testing || !canTest"
                @click="testConnection"
              >
                <LoadingSpinner v-if="testing" size="sm" variant="inline" />
                {{ testing ? 'Testing...' : 'Test connection' }}
              </button>
              <span v-if="testResult === 'ok'" class="text-sm text-status-success inline-flex items-center gap-1.5">
                <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-success"></span>
                Connected
              </span>
              <span
                v-else-if="testResult === 'failed'"
                class="text-sm text-status-error inline-flex items-center gap-1.5"
                :title="testErrorMessage"
              >
                <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-error"></span>
                {{ testErrorMessage || 'Failed' }}
              </span>
            </div>
            <div class="flex items-center gap-3">
              <button
                v-if="channel"
                type="button"
                class="px-4 py-2 border border-status-error-border text-status-error rounded-lg hover:bg-status-error-bg text-sm font-medium disabled:opacity-50"
                :disabled="deleting"
                @click="confirmAndDelete"
              >
                {{ deleting ? 'Deleting...' : 'Delete' }}
              </button>
              <button
                type="submit"
                class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 text-sm font-medium"
                :disabled="saving || !canSave"
              >
                <LoadingSpinner v-if="saving" size="sm" variant="inline" />
                {{ channel ? (saving ? 'Saving...' : 'Save changes') : (saving ? 'Creating...' : 'Create channel') }}
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>

    <ConfirmModal
      :show="showClearCredentialConfirm"
      variant="danger"
      title="Remove stored password?"
      message="The worker will stop authenticating until a new one is saved."
      confirm-label="Remove"
      @confirm="doClearCredential"
      @close="showClearCredentialConfirm = false"
    />

    <ConfirmModal
      :show="showDeleteChannelConfirm"
      variant="danger"
      title="Delete this email channel?"
      message="Tickets already opened from it stay intact, but no new messages will be ingested. This cannot be undone."
      confirm-label="Delete channel"
      @confirm="doDeleteChannel"
      @close="showDeleteChannelConfirm = false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import {
  channelsService,
  type Channel,
  type ImapChannelConfig,
  type ImapRuntimeState
} from '@/services/channelsService';
import { createErrorFromResponse } from '@/utils/errors';

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

const loading = ref(true);
const saving = ref(false);
const testing = ref(false);
const deleting = ref(false);
const clearing = ref(false);
const channel = ref<Channel | null>(null);
const form = reactive<FormState>(emptyForm());
const testResult = ref<'idle' | 'ok' | 'failed'>('idle');
const testErrorMessage = ref('');
const successMessage = ref('');
const errorMessage = ref('');

const inputClasses =
  'w-full bg-surface-alt border border-default rounded-lg px-3 py-2.5 text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent transition-colors';

const runtimeState = computed<ImapRuntimeState>(() => {
  if (!channel.value) return {};
  return (channel.value.runtime_state ?? {}) as ImapRuntimeState;
});

// Password isn't part of the save-validity gate: creating without one
// is allowed (admin sets it later) and editing without one preserves
// the stored secret. See `save()` for the "send-only-if-non-empty" rule.
const canSave = computed(() => {
  return (
    form.name.trim().length > 0 &&
    form.host.trim().length > 0 &&
    form.username.trim().length > 0 &&
    form.reply_domain.trim().length > 0
  );
});

// Test-connection is enabled when we have either a candidate password
// on the form or a stored one on the channel. Without either there's
// nothing to authenticate with.
const canTest = computed(() => {
  return form.password.length > 0 || (channel.value?.has_credential ?? false);
});

function populateForm(ch: Channel) {
  const cfg = (ch.config ?? {}) as unknown as ImapChannelConfig;
  form.name = ch.name;
  form.enabled = ch.enabled;
  form.host = cfg.host ?? '';
  form.port = cfg.port ?? DEFAULT_CONFIG.port;
  form.username = cfg.username ?? '';
  form.mailbox = cfg.mailbox ?? DEFAULT_CONFIG.mailbox;
  form.reply_domain = cfg.reply_domain ?? '';
  form.insecure_skip_cert_verify = cfg.insecure_skip_cert_verify ?? false;
  form.password = '';
}

// Typed locally, then widened to the `Record<string, unknown>` shape the
// service accepts — the channels endpoint is generic over providers and
// only `email_imap`'s shape is validated server-side.
function buildConfig(): Record<string, unknown> {
  const cfg: ImapChannelConfig = {
    host: form.host.trim(),
    port: form.port,
    username: form.username.trim(),
    mailbox: form.mailbox.trim() || DEFAULT_CONFIG.mailbox,
    use_tls: true,
    reply_domain: form.reply_domain.trim(),
    insecure_skip_cert_verify: form.insecure_skip_cert_verify
  };
  return cfg as unknown as Record<string, unknown>;
}

function clearMessages() {
  successMessage.value = '';
  errorMessage.value = '';
}

function formatRelative(iso: string): string {
  const then = new Date(iso).getTime();
  const now = Date.now();
  const diffSec = Math.max(0, Math.floor((now - then) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

onMounted(async () => {
  await loadExisting();
});

async function loadExisting() {
  loading.value = true;
  try {
    const list = await channelsService.list();
    const match = list.find((c) => c.provider === EMAIL_PROVIDER) ?? null;
    channel.value = match;
    if (match) populateForm(match);
  } catch {
    errorMessage.value = 'Failed to load email channel';
  } finally {
    loading.value = false;
  }
}

async function save() {
  clearMessages();
  testResult.value = 'idle';
  if (!canSave.value) return;
  saving.value = true;
  try {
    if (channel.value) {
      const updated = await channelsService.update(channel.value.id, {
        name: form.name.trim(),
        enabled: form.enabled,
        config: buildConfig(),
        // Only send password when the admin typed one — empty string
        // must not nuke the stored secret. See channelsService's
        // `UpdateChannelRequest` contract.
        ...(form.password.length > 0 ? { password: form.password } : {})
      });
      channel.value = updated;
      populateForm(updated);
      successMessage.value = 'Channel updated';
    } else {
      const created = await channelsService.create({
        provider: EMAIL_PROVIDER,
        name: form.name.trim(),
        enabled: form.enabled,
        config: buildConfig(),
        ...(form.password.length > 0 ? { password: form.password } : {})
      });
      channel.value = created;
      populateForm(created);
      successMessage.value = 'Channel created';
    }
    setTimeout(() => (successMessage.value = ''), 3000);
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
      form.password.length > 0 ? form.password : undefined
    );
    if (result.ok) {
      testResult.value = 'ok';
    } else {
      testResult.value = 'failed';
      testErrorMessage.value = result.error ?? 'Unknown error';
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
    await loadExisting();
    successMessage.value = 'Password removed';
    setTimeout(() => (successMessage.value = ''), 3000);
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
    channel.value = null;
    Object.assign(form, emptyForm());
    successMessage.value = 'Channel deleted';
    setTimeout(() => (successMessage.value = ''), 3000);
  } catch (e: unknown) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage();
  } finally {
    deleting.value = false;
  }
}

</script>
