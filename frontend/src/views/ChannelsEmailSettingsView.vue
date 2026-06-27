<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <RouterLink
        :to="{ name: 'admin-channels' }"
        class="text-sm text-secondary hover:text-primary w-fit"
      >
        &lsaquo; {{ $t('admin-nav-channels-title') }}
      </RouterLink>
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-channels-email-title') }}</h1>
        <p class="text-secondary">
          {{ $t('admin-channels-email-description') }}
        </p>
      </div>

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
            <FormInput
              v-model="form.name"
              class="md:col-span-2"
              :label="$t('admin-channels-email-field-name-label')"
              :placeholder="$t('admin-channels-email-field-name-placeholder')"
              :description="$t('admin-channels-email-field-name-hint')"
              required
            />

            <FormInput
              v-model="form.host"
              :label="$t('admin-channels-email-field-host-label')"
              :placeholder="$t('admin-channels-email-field-host-placeholder')"
              required
              autocomplete="off"
            />

            <FormNumber
              :model-value="form.port"
              :label="$t('admin-channels-email-field-port-label')"
              :description="$t('admin-channels-email-field-port-hint')"
              integer
              :min="1"
              :max="65535"
              @update:model-value="(v) => (form.port = v ?? DEFAULT_CONFIG.port)"
            />

            <FormInput
              v-model="form.username"
              :label="$t('admin-channels-email-field-username-label')"
              :placeholder="$t('admin-channels-email-field-username-placeholder')"
              required
              autocomplete="off"
            />

            <FormInput
              v-model="form.mailbox"
              :label="$t('admin-channels-email-field-mailbox-label')"
              :placeholder="$t('admin-channels-email-field-mailbox-placeholder')"
              :description="$t('admin-channels-email-field-mailbox-hint')"
            />

            <FormInput
              v-model="form.reply_domain"
              class="md:col-span-2"
              :label="$t('admin-channels-email-field-reply-domain-label')"
              :placeholder="$t('admin-channels-email-field-reply-domain-placeholder')"
              :description="$t('admin-channels-email-field-reply-domain-hint')"
              required
              autocomplete="off"
            />

            <div class="flex flex-col gap-2 md:col-span-2">
              <FormInput
                v-model="form.password"
                type="password"
                :label="$t('admin-channels-email-field-password-label')"
                :description="channel?.has_credential ? $t('admin-channels-email-field-password-keep-existing') : undefined"
                :placeholder="channel?.has_credential ? $t('admin-channels-email-field-password-placeholder-stored') : $t('admin-channels-email-field-password-placeholder-new')"
                autocomplete="new-password"
              />
              <div v-if="channel?.has_credential" class="flex items-center gap-4">
                <Button variant="ghost-danger" size="sm" :loading="clearing" @click="requestClearCredential">
                  {{ clearing ? $t('admin-channels-email-removing-password') : $t('admin-channels-email-remove-password') }}
                </Button>
              </div>
            </div>
          </div>

          <!-- Advanced / dev options. Dev-only: the Skip-TLS-verification
               option is hidden (and server-rejected) outside development. -->
          <details v-if="insecureTlsAllowed" class="border-t border-default pt-4">
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
                @click="requestDeleteChannel"
              >
                {{ deleting ? $t('admin-channels-email-deleting') : $t('admin-channels-email-delete') }}
              </Button>
              <Button type="submit" :loading="saving" :disabled="!canSave">
                {{ submitLabel }}
              </Button>
            </div>
          </div>
        </form>

        <!-- Auto-acknowledgement. Workspace-wide setting (stored on
             site_settings) but the admin discovery path runs through
             "I configured email, now what does the customer see?",
             so the form lives here. Hidden when no email channel
             exists since the auto-ack won't fire without inbound
             mail to react to. -->
        <form
          v-if="channel"
          class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6"
          @submit.prevent="saveAutoAck"
        >
          <div class="flex flex-col gap-1">
            <h2 class="text-lg font-semibold text-primary">
              {{ $t('admin-channels-email-auto-ack-heading') }}
            </h2>
            <p class="text-sm text-secondary">
              {{ $t('admin-channels-email-auto-ack-subtitle') }}
            </p>
          </div>

          <ToggleSwitch
            v-model="autoAckEnabled"
            :label="$t('admin-channels-email-auto-ack-toggle-label')"
            :description="$t('admin-channels-email-auto-ack-toggle-description')"
          />

          <div class="flex flex-col gap-2">
            <FormTextarea
              v-model="autoAckTemplate"
              :label="$t('admin-channels-email-auto-ack-template-label')"
              :placeholder="$t('admin-channels-email-auto-ack-template-placeholder')"
              :description="$t('admin-channels-email-auto-ack-template-hint')"
              :rows="6"
              mono
              :disabled="!autoAckEnabled"
            />
            <p class="text-xs text-tertiary">
              {{ $t('admin-channels-email-auto-ack-variables-hint') }}
              <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;ticket_id&#125;&#125;</code>,
              <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;ticket_title&#125;&#125;</code>,
              <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;customer_name&#125;&#125;</code>,
              <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;customer_first_name&#125;&#125;</code>,
              <code class="text-[10px] bg-surface-alt px-1 rounded">&#123;&#123;app_name&#125;&#125;</code>
            </p>
          </div>

          <div class="flex justify-end border-t border-default pt-4">
            <Button type="submit" :loading="savingAutoAck" :disabled="!autoAckIsDirty">
              {{ savingAutoAck ? $t('admin-channels-email-auto-ack-saving') : $t('admin-channels-email-auto-ack-save') }}
            </Button>
          </div>
        </form>
      </div>
    </div>

    <ConfirmModal
      :show="confirmModalContent !== null"
      variant="danger"
      :title="confirmModalContent?.title ?? ''"
      :message="confirmModalContent?.message ?? ''"
      :confirm-label="confirmModalContent?.confirmLabel ?? ''"
      @confirm="executePendingAction"
      @close="pendingAction = null"
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
import FormInput from '@/components/common/FormInput.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import {
  channelsService,
  type Channel,
  type ImapChannelConfig,
  type ImapRuntimeState
} from '@nosdesk/core/services/channelsService';
import brandingService, { type BrandingConfig } from '@nosdesk/core/services/brandingService';
import apiClient from '@nosdesk/core/apiClient';
import { useToastStore } from '@/stores/toast';
import { createErrorFromResponse } from '@/utils/errors';
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils';
import FormTextarea from '@/components/common/FormTextarea.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

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

// Skip-TLS-verification is a development-only option; the server hard-blocks
// it in production (refuses to save, ignores it at connect time). Mirror that
// in the UI by reading the deployment environment and hiding the option
// entirely outside development. Defaults to hidden until the value loads, so
// the toggle never flashes on a production deployment.
const systemInfoQuery = useQuery({
  key: ['admin-system-info'],
  query: async () =>
    (await apiClient.get<{ environment: string }>('/admin/system/info')).data,
});
const insecureTlsAllowed = computed(() => {
  const env = systemInfoQuery.data.value?.environment;
  return env != null && env !== 'production';
});

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

// Auto-ack settings live on site_settings, not on the channel row,
// but they're conceptually a property of "what happens when a new
// email lands" so the admin form belongs here. We share the
// branding query key with BrandingSettingsView so a save here
// invalidates that view's cache and vice-versa.
const BRANDING_KEY = ['branding-config'] as const;
const brandingQuery = useQuery({
  key: BRANDING_KEY,
  query: () => brandingService.getBrandingConfig(),
});
const brandingConfig = computed<BrandingConfig | null>(
  () => brandingQuery.data.value ?? null,
);

const saving = ref(false);
const testing = ref(false);
const deleting = ref(false);
const clearing = ref(false);
const savingAutoAck = ref(false);
const form = ref<FormState>(emptyForm());
const autoAckEnabled = ref(true);
const autoAckTemplate = ref('');
const testResult = ref<'idle' | 'ok' | 'failed'>('idle');
const testErrorMessage = ref('');
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

// Same one-shot seed shape for the auto-ack form. Separate flag so
// the two queries can resolve in any order without one clobbering
// the other's seed.
const autoAckSeeded = ref(false);
watch(
  brandingQuery.data,
  (data) => {
    if (!data || autoAckSeeded.value) return;
    autoAckEnabled.value = data.channel_auto_ack_enabled;
    autoAckTemplate.value = data.channel_auto_ack_template ?? '';
    autoAckSeeded.value = true;
  },
  { immediate: true },
);

// Port is now bound directly through FormNumber, which is
// number-typed end-to-end. The old portStr string bridge that
// existed for FormInput's string-typed defineModel is no longer
// needed; the empty-clear case falls back to DEFAULT_CONFIG.port
// inline in the @update:model-value handler.

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

/** Transient "saved" feedback via the toast store (the convention for
 *  action feedback; page-level errors stay inline). */
function flashSuccess(key: string) {
  toast.success(t(key));
}

const submitLabel = computed(() => {
  if (saving.value) {
    return channel.value
      ? t('admin-channels-email-saving')
      : t('admin-channels-email-creating');
  }
  return channel.value ? t('admin-channels-email-save') : t('admin-channels-email-create');
});

const autoAckIsDirty = computed(() => {
  const cfg = brandingConfig.value;
  if (!cfg) return false;
  return (
    autoAckEnabled.value !== cfg.channel_auto_ack_enabled ||
    autoAckTemplate.value !== (cfg.channel_auto_ack_template ?? '')
  );
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
    // Never send true outside development: the server rejects it there, and
    // this also clears a legacy value if a prod deployment inherited one.
    insecure_skip_cert_verify: insecureTlsAllowed.value && f.insecure_skip_cert_verify
  };
  return cfg as unknown as Record<string, unknown>;
}

function clearMessages() {
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

async function saveAutoAck() {
  if (!autoAckIsDirty.value) return;
  clearMessages();
  savingAutoAck.value = true;
  try {
    const updated = await brandingService.updateBrandingConfig({
      channel_auto_ack_enabled: autoAckEnabled.value,
      // Empty string clears back to "use built-in FTL default".
      channel_auto_ack_template: autoAckTemplate.value,
    });
    // Keep the shared branding cache in lockstep so BrandingSettingsView
    // (which uses the same key) reflects the change without a refetch.
    queryCache.setQueryData(BRANDING_KEY, updated);
    flashSuccess('admin-channels-email-auto-ack-success-saved');
  } catch (e: unknown) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage();
  } finally {
    savingAutoAck.value = false;
  }
}

// Destructive actions share one confirm-modal driver. The
// `pendingAction` discriminator picks the copy + handler so the
// component only renders one ConfirmModal instance and the
// open/close plumbing isn't duplicated per action.
type PendingAction = 'clear-credential' | 'delete-channel' | null;
const pendingAction = ref<PendingAction>(null);

const confirmModalContent = computed(() => {
  switch (pendingAction.value) {
    case 'clear-credential':
      return {
        title: t('admin-channels-email-clear-credential-title'),
        message: t('admin-channels-email-clear-credential-message'),
        confirmLabel: t('admin-channels-email-clear-credential-confirm'),
      };
    case 'delete-channel':
      return {
        title: t('admin-channels-email-delete-title'),
        message: t('admin-channels-email-delete-message'),
        confirmLabel: t('admin-channels-email-delete-confirm'),
      };
    default:
      return null;
  }
});

function requestClearCredential() {
  if (!channel.value) return;
  pendingAction.value = 'clear-credential';
}

function requestDeleteChannel() {
  if (!channel.value) return;
  pendingAction.value = 'delete-channel';
}

async function executePendingAction() {
  const action = pendingAction.value;
  pendingAction.value = null;
  if (!channel.value || !action) return;
  if (action === 'clear-credential') {
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
  } else if (action === 'delete-channel') {
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
}

</script>
