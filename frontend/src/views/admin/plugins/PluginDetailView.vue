<script setup lang="ts">
/**
 * Per-plugin detail page: identity header, settings form,
 * lifecycle actions, and metadata. Replaces the inline modal
 * that used to live in the list view; deep-linkable so admins
 * can paste a URL straight to a plugin's settings.
 *
 * Settings use an explicit Save button with a dirty-state guard:
 * fields are local until the user commits, the button is
 * disabled until something has actually changed, and the whole
 * batch goes up in parallel. This is more conventional than
 * auto-save-on-blur for sensitive admin config (PATs, API keys)
 * where the user wants to see exactly when their input was
 * transmitted. Secrets keep the "Configured" / "Update" affordance
 * because the backend never sends the value back to the client.
 */
import { computed, ref, watch } from 'vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Modal from '@/components/Modal.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import AsyncBoundary from '@/components/common/AsyncBoundary.vue';
import PluginIcon from '@/components/plugins/PluginIcon.vue';
import PluginStateBadge from '@/components/plugins/PluginStateBadge.vue';
import PluginTrustBadge from '@/components/plugins/PluginTrustBadge.vue';
import pluginService from '@nosdesk/core/services/pluginService';
import { unloadPlugin, getLoadedPlugin } from '@/plugins/loader';
import { logger } from '@nosdesk/core/utils/logger';
import { useDateStore } from '@nosdesk/core/stores/dateStore';
import { resolvePluginI18n } from '@nosdesk/core/utils/pluginI18n';
import type { Plugin, PluginSetting } from '@nosdesk/core/types/plugin';
import { canonicalSlotName } from '@nosdesk/core/types/plugin';
import PluginPermissionList from '@/components/plugins/PluginPermissionList.vue';
import PluginSlot from '@/plugins/components/PluginSlot.vue';

const route = useRoute();
const router = useRouter();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const errorMessage = ref('');
const successMessage = ref('');

const settings = ref<PluginSetting[]>([]);
const settingsLoading = ref(false);
const settingValues = ref<Record<string, unknown>>({});
/** Snapshot of `settingValues` after the last successful load or
 * save. Drives the dirty check and the Discard action. */
const originalValues = ref<Record<string, unknown>>({});
const editingSecrets = ref<Set<string>>(new Set());
const saveInFlight = ref(false);
const saveError = ref('');

const uuid = computed(() => route.params.uuid as string);

const queryCache = useQueryCache();
// Cache-first: the plugin detail is keyed by uuid, so a revisit renders
// instantly from cache then refreshes silently (SWR). Settings load
// reactively once the plugin (and its manifest) resolves.
const pluginQuery = useQuery({
  key: () => ['plugin', uuid.value],
  query: () => pluginService.getPlugin(uuid.value),
  enabled: () => !!uuid.value,
});
const plugin = computed<Plugin | null>(() => pluginQuery.data.value ?? null);

// Resolve a plugin-authored `%key%` string against the manifest's i18n tables.
const dateStore = useDateStore();
const l10n = (value: string | null | undefined): string =>
  resolvePluginI18n(value, plugin.value?.manifest.i18n, dateStore.locale);

// Show the plugin's own config panel (`settings.integrations.page`) when it both
// declares that component AND is loaded into the slot registry (i.e. enabled) —
// so a disabled plugin that declares one doesn't render an empty heading.
const showIntegrationPanel = computed(() => {
  const p = plugin.value;
  if (!p || !getLoadedPlugin(p.uuid)) return false;
  return Object.values(p.manifest.components).some(
    (c) => canonicalSlotName(c.slot) === 'settings.integrations.page',
  );
});
const loadOp = computed(() => ({
  isPending: pluginQuery.asyncStatus.value === 'loading',
  isError: pluginQuery.state.value.status === 'error',
  error: pluginQuery.error.value,
}));
watch(
  () => pluginQuery.data.value,
  (p) => {
    if (p) void loadSettings();
  },
  { immediate: true },
);

async function loadSettings() {
  if (!plugin.value || plugin.value.manifest.settings.length === 0) {
    settings.value = [];
    settingValues.value = {};
    originalValues.value = {};
    return;
  }
  settingsLoading.value = true;
  editingSecrets.value = new Set();
  saveError.value = '';
  try {
    settings.value = await pluginService.getPluginSettings(uuid.value);
    const next: Record<string, unknown> = {};
    for (const s of settings.value) next[s.key] = s.value;
    for (const def of plugin.value.manifest.settings) {
      if (!(def.key in next)) next[def.key] = def.default;
    }
    settingValues.value = next;
    originalValues.value = { ...next };
  } catch (e) {
    logger.error('Failed to load settings', { error: e });
  } finally {
    settingsLoading.value = false;
  }
}

function announce(msg: string) {
  successMessage.value = msg;
  setTimeout(() => (successMessage.value = ''), 3000);
}

// Consent gate: untrusted-tier plugins land in `awaiting_consent` and don't run
// until an admin approves their requested permission scope.
const isAwaitingConsent = computed(() => plugin.value?.state === 'awaiting_consent');
const consentInFlight = ref(false);
async function approveConsent() {
  if (!plugin.value) return;
  consentInFlight.value = true;
  errorMessage.value = '';
  try {
    await pluginService.consentToPlugin(uuid.value);
    await pluginQuery.refetch();
    announce(t('plugin-detail-consent-approve'));
  } catch (error) {
    logger.error('Failed to consent to plugin', { error, uuid: uuid.value });
    errorMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    consentInFlight.value = false;
  }
}

function isSecretConfigured(key: string): boolean {
  return settings.value.find((s) => s.key === key)?.is_secret === true;
}

function editSecret(key: string) {
  editingSecrets.value = new Set(editingSecrets.value).add(key);
  settingValues.value[key] = '';
}

function cancelEditSecret(key: string) {
  const next = new Set(editingSecrets.value);
  next.delete(key);
  editingSecrets.value = next;
  settingValues.value[key] = originalValues.value[key] ?? null;
}

/**
 * The set of keys whose current value diverges from the last
 * persisted value. For non-secrets this is a value comparison; for
 * secrets the backend never returns the persisted value, so the
 * only signal of "user wants to change this" is an active edit
 * with a non-empty input.
 */
const dirtyKeys = computed<string[]>(() => {
  if (!plugin.value) return [];
  const keys: string[] = [];
  for (const def of plugin.value.manifest.settings) {
    const current = settingValues.value[def.key];
    if (def.type === 'secret') {
      if (editingSecrets.value.has(def.key) && current) keys.push(def.key);
    } else if (!Object.is(current, originalValues.value[def.key])) {
      keys.push(def.key);
    }
  }
  return keys;
});

const isDirty = computed(() => dirtyKeys.value.length > 0);

const missingRequired = computed<string[]>(() => {
  if (!plugin.value) return [];
  const missing: string[] = [];
  for (const def of plugin.value.manifest.settings) {
    if (!def.required) continue;
    // Secrets are required to be *configured*, not necessarily
    // edited right now; a previously-set secret stays valid.
    if (def.type === 'secret') {
      if (!isSecretConfigured(def.key) && !editingSecrets.value.has(def.key)) {
        missing.push(def.key);
        continue;
      }
      if (editingSecrets.value.has(def.key) && !settingValues.value[def.key]) {
        missing.push(def.key);
      }
      continue;
    }
    const v = settingValues.value[def.key];
    if (v === null || v === undefined || v === '') missing.push(def.key);
  }
  return missing;
});

const canSave = computed(
  () => isDirty.value && missingRequired.value.length === 0 && !saveInFlight.value,
);

async function saveSettings() {
  if (!plugin.value || !canSave.value) return;
  const targetUuid = plugin.value.uuid;
  const keys = dirtyKeys.value;
  saveInFlight.value = true;
  saveError.value = '';
  try {
    await Promise.all(
      keys.map((key) =>
        pluginService.setPluginSetting(targetUuid, {
          key,
          value: settingValues.value[key],
        }),
      ),
    );
    // Re-fetch so secret rows flip back to the "Configured" state
    // and any server-side coercion (e.g. number normalisation) is
    // reflected in originalValues.
    settings.value = await pluginService.getPluginSettings(targetUuid);
    editingSecrets.value = new Set();
    const fresh: Record<string, unknown> = {};
    for (const s of settings.value) fresh[s.key] = s.value;
    for (const def of plugin.value.manifest.settings) {
      if (!(def.key in fresh)) fresh[def.key] = def.default;
    }
    settingValues.value = fresh;
    originalValues.value = { ...fresh };
    announce(t('plugin-detail-toast-saved', { count: keys.length }));
  } catch (e) {
    saveError.value = t('plugin-detail-error-save');
    logger.error('Failed to save settings', { error: e, keys });
  } finally {
    saveInFlight.value = false;
  }
}

function discardSettingChanges() {
  settingValues.value = { ...originalValues.value };
  editingSecrets.value = new Set();
  saveError.value = '';
}

async function toggle() {
  if (!plugin.value) return;
  if (plugin.value.state !== 'installed' && plugin.value.state !== 'disabled') return;
  const enable = plugin.value.state === 'disabled';
  try {
    const updated = await pluginService.updatePlugin(plugin.value.uuid, { enabled: enable });
    await queryCache.invalidateQueries({ key: ['plugin', uuid.value] });
    if (updated.state !== 'installed') unloadPlugin(updated.uuid);
    announce(
      updated.state === 'installed'
        ? t('plugin-detail-toast-enabled')
        : t('plugin-detail-toast-disabled'),
    );
  } catch (e) {
    errorMessage.value = t('plugin-detail-error-toggle');
    logger.error('Failed to toggle', { error: e });
  }
}

const showUninstallConfirm = ref(false);

async function executeUninstall() {
  if (!plugin.value) return;
  try {
    await pluginService.uninstallPlugin(plugin.value.uuid);
    unloadPlugin(plugin.value.uuid);
    showUninstallConfirm.value = false;
    router.replace('/admin/plugins');
  } catch (e) {
    errorMessage.value = t('plugin-detail-error-uninstall');
    logger.error('Failed to uninstall', { error: e });
  }
}

const toggleLabel = computed(() => {
  if (!plugin.value) return '';
  return plugin.value.state === 'installed'
    ? t('plugin-detail-action-disable')
    : t('plugin-detail-action-enable');
});

const requiredAriaLabel = computed(() => t('plugin-detail-required-aria'));
const saveButtonLabel = computed(() =>
  saveInFlight.value ? t('plugin-detail-action-saving') : t('plugin-detail-action-save'),
);
</script>

<template>
  <div class="mx-auto flex w-full max-w-4xl flex-1 flex-col gap-4 px-4 py-4 sm:px-6">
    <RouterLink
      to="/admin/plugins"
      class="inline-flex items-center gap-1.5 text-sm text-secondary transition-colors hover:text-primary"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="h-4 w-4"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="2"
        aria-hidden="true"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
      {{ t('plugin-detail-back') }}
    </RouterLink>

    <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <AsyncBoundary :op="loadOp" :has-data="!!plugin">
      <template #pending>
        <LoadingSpinner :text="t('plugin-detail-loading')" />
      </template>
      <template #error>
        <AlertMessage type="error" :message="t('plugin-detail-error-load')" />
      </template>

    <template v-if="plugin">
      <!-- Header card -->
      <header class="flex flex-col gap-4 rounded-xl border border-default bg-surface p-5 sm:flex-row sm:items-start">
        <PluginIcon :uuid="plugin.uuid" :alt="l10n(plugin.display_name)" size="lg" />
        <div class="min-w-0 flex-1">
          <h1 class="text-2xl font-bold text-primary">{{ l10n(plugin.display_name) }}</h1>
          <p class="mt-1 font-mono text-xs text-tertiary">{{ plugin.name }}</p>
          <div class="mt-3 flex flex-wrap items-center gap-2 text-sm">
            <code class="rounded bg-surface-alt px-1.5 py-0.5 font-mono text-xs text-secondary">
              v{{ plugin.version }}
            </code>
            <PluginTrustBadge :level="plugin.trust_level" />
            <PluginStateBadge :state="plugin.state" />
          </div>
          <p v-if="plugin.description" class="mt-3 text-sm text-secondary">
            {{ plugin.description }}
          </p>
        </div>
      </header>

      <!-- Consent gate: pending admin approval of the requested permission scope -->
      <section
        v-if="isAwaitingConsent"
        class="rounded-xl border border-status-warning/40 bg-status-warning/5 p-5"
      >
        <h2 class="font-semibold text-primary">{{ t('plugin-detail-consent-pending-title') }}</h2>
        <p class="mt-1 text-sm text-secondary">{{ t('plugin-detail-consent-pending-body') }}</p>
        <div class="mt-4">
          <h3 class="mb-2 text-xs tracking-wide text-tertiary uppercase">
            {{ t('plugin-detail-consent-heading') }}
          </h3>
          <PluginPermissionList
            :permissions="plugin.manifest.permissions"
            :reasons="plugin.manifest.permission_reasons"
          />
        </div>
        <div class="mt-4">
          <Button variant="primary" :disabled="consentInFlight" @click="approveConsent">
            {{ consentInFlight ? t('plugin-detail-consent-approving') : t('plugin-detail-consent-approve') }}
          </Button>
        </div>
      </section>

      <!-- Lifecycle actions -->
      <section
        aria-labelledby="lifecycle-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="lifecycle-heading" class="mb-3 font-semibold text-primary">{{ t('plugin-detail-lifecycle-heading') }}</h2>
        <div class="flex flex-wrap gap-2">
          <button
            v-if="plugin.state === 'installed' || plugin.state === 'disabled'"
            type="button"
            @click="toggle"
            class="rounded-lg border border-default bg-surface-alt px-3 py-1.5 text-sm text-primary transition-colors hover:bg-surface-hover focus:ring-2 focus:ring-accent/30 focus:outline-none"
          >
            {{ toggleLabel }}
          </button>
          <button
            type="button"
            @click="showUninstallConfirm = true"
            class="rounded-lg border border-status-error/30 bg-status-error/10 px-3 py-1.5 text-sm text-status-error transition-colors hover:bg-status-error/20 focus:ring-2 focus:ring-status-error/30 focus:outline-none"
          >
            {{ t('plugin-detail-action-uninstall') }}
          </button>
        </div>
      </section>

      <!-- Settings -->
      <section
        v-if="plugin.manifest.settings.length > 0"
        aria-labelledby="settings-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="settings-heading" class="mb-4 font-semibold text-primary">{{ t('plugin-detail-settings-heading') }}</h2>

        <LoadingSpinner v-if="settingsLoading" :text="t('plugin-detail-loading-settings')" />

        <form
          v-else
          class="flex flex-col gap-5"
          @submit.prevent="saveSettings"
        >
          <div
            v-for="def in plugin.manifest.settings"
            :key="def.key"
            class="flex flex-col gap-2"
          >
            <div>
              <label :for="`setting-${def.key}`" class="block text-sm font-medium text-primary">
                {{ l10n(def.label) }}
                <span v-if="def.required" class="text-status-error" :aria-label="requiredAriaLabel">*</span>
              </label>
              <p v-if="def.description" class="mt-1 text-xs text-tertiary">
                {{ l10n(def.description) }}
              </p>
            </div>

            <input
              v-if="def.type === 'string'"
              :id="`setting-${def.key}`"
              v-model="settingValues[def.key]"
              type="text"
              class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-primary placeholder-tertiary focus:border-transparent focus:ring-2 focus:ring-accent focus:outline-none"
            />

            <div v-else-if="def.type === 'secret'">
              <div
                v-if="isSecretConfigured(def.key) && !editingSecrets.has(def.key)"
                class="flex items-center gap-3"
              >
                <div
                  class="flex flex-1 items-center gap-2 rounded-lg border border-default bg-surface-alt px-3 py-2"
                >
                  <svg
                    class="h-4 w-4 flex-shrink-0 text-status-success"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    stroke-width="2"
                    aria-hidden="true"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"
                    />
                  </svg>
                  <span class="text-sm text-secondary">{{ t('plugin-detail-secret-configured') }}</span>
                </div>
                <button
                  type="button"
                  @click="editSecret(def.key)"
                  class="rounded-lg px-3 py-2 text-sm text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                >
                  {{ t('plugin-detail-secret-update') }}
                </button>
              </div>
              <div v-else class="flex items-center gap-2">
                <input
                  :id="`setting-${def.key}`"
                  v-model="settingValues[def.key]"
                  type="password"
                  autocomplete="off"
                  spellcheck="false"
                  :placeholder="editingSecrets.has(def.key) ? t('plugin-detail-secret-placeholder-new') : t('plugin-detail-secret-placeholder')"
                  class="flex-1 rounded-lg border border-default bg-surface-alt px-3 py-2 text-primary placeholder-tertiary focus:border-transparent focus:ring-2 focus:ring-accent focus:outline-none"
                />
                <button
                  v-if="editingSecrets.has(def.key)"
                  type="button"
                  @click="cancelEditSecret(def.key)"
                  class="rounded-lg px-3 py-2 text-sm text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                >
                  {{ t('plugin-detail-secret-cancel') }}
                </button>
              </div>
            </div>

            <FormNumber
              v-else-if="def.type === 'number'"
              :id="`setting-${def.key}`"
              :model-value="(settingValues[def.key] as number | null | undefined) ?? null"
              size="sm"
              @update:model-value="(v) => (settingValues[def.key] = v)"
            />

            <Checkbox
              v-else-if="def.type === 'boolean'"
              :model-value="!!settingValues[def.key]"
              :id="`setting-${def.key}`"
              size="sm"
              :label="t('plugin-detail-boolean-enabled')"
              @update:model-value="(v: boolean) => (settingValues[def.key] = v)"
            />

            <BaseDropdown
              v-else-if="def.type === 'select' && def.options"
              :model-value="String(settingValues[def.key] ?? '')"
              :options="def.options"
              size="sm"
              @update:model-value="(v) => (settingValues[def.key] = String(v))"
            />
          </div>

          <p v-if="saveError" class="text-sm text-status-error" role="alert">
            {{ saveError }}
          </p>

          <footer
            class="flex items-center justify-between gap-3 border-t border-default pt-4"
          >
            <p class="text-xs text-tertiary" aria-live="polite">
              <template v-if="missingRequired.length > 0">
                {{ t('plugin-detail-status-missing-required', { count: missingRequired.length }) }}
              </template>
              <template v-else-if="isDirty">
                {{ t('plugin-detail-status-unsaved', { count: dirtyKeys.length }) }}
              </template>
              <template v-else>{{ t('plugin-detail-status-all-saved') }}</template>
            </p>
            <div class="flex items-center gap-2">
              <button
                v-if="isDirty"
                type="button"
                @click="discardSettingChanges"
                :disabled="saveInFlight"
                class="px-3 py-1.5 text-sm text-secondary transition-colors hover:text-primary disabled:opacity-50"
              >
                {{ t('plugin-detail-action-discard') }}
              </button>
              <button
                type="submit"
                :disabled="!canSave"
                class="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-on-accent transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
              >
                {{ saveButtonLabel }}
              </button>
            </div>
          </footer>
        </form>
      </section>

      <!-- Plugin-rendered configuration page (settings.integrations.page). A
           sandboxed panel the plugin fills; scoped to this plugin only. -->
      <section
        v-if="showIntegrationPanel"
        aria-labelledby="integration-page-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="integration-page-heading" class="mb-4 font-semibold text-primary">
          {{ t('plugin-detail-integration-page-heading') }}
        </h2>
        <PluginSlot target="settings.integrations.page" :plugin-uuid="plugin.uuid" />
      </section>

      <!-- Metadata -->
      <section
        aria-labelledby="metadata-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="metadata-heading" class="mb-3 font-semibold text-primary">{{ t('plugin-detail-metadata-heading') }}</h2>
        <dl class="grid grid-cols-1 gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
          <div>
            <dt class="text-xs tracking-wide text-tertiary uppercase">{{ t('plugin-detail-metadata-source') }}</dt>
            <dd class="mt-1 text-secondary">{{ plugin.source }}</dd>
          </div>
          <div class="sm:col-span-2">
            <dt class="text-xs tracking-wide text-tertiary uppercase">{{ t('plugin-detail-metadata-permissions') }}</dt>
            <dd class="mt-2">
              <PluginPermissionList
                :permissions="plugin.manifest.permissions"
                :reasons="plugin.manifest.permission_reasons"
              />
              <span v-if="!plugin.manifest.permissions.length" class="text-secondary">
                {{ t('plugin-detail-metadata-permissions-count', { count: 0 }) }}
              </span>
            </dd>
          </div>
          <div v-if="plugin.manifest.repository" class="sm:col-span-2">
            <dt class="text-xs tracking-wide text-tertiary uppercase">{{ t('plugin-detail-metadata-repository') }}</dt>
            <dd class="mt-1">
              <a
                :href="plugin.manifest.repository"
                target="_blank"
                rel="noopener noreferrer"
                class="text-accent hover:underline focus:underline focus:outline-none"
              >
                {{ plugin.manifest.repository }}
              </a>
            </dd>
          </div>
        </dl>
      </section>
    </template>
    </AsyncBoundary>

    <!-- Uninstall confirmation -->
    <Modal
      :show="showUninstallConfirm && !!plugin"
      :title="t('plugin-detail-uninstall-title')"
      size="sm"
      @close="showUninstallConfirm = false"
    >
      <p class="text-sm text-secondary">
        {{ t('plugin-detail-uninstall-prompt-prefix') }}
        <strong class="text-primary">{{ plugin?.display_name }}</strong
        >{{ t('plugin-detail-uninstall-prompt-mid') }}
        <code class="rounded bg-surface-alt px-1 text-xs">on_uninstall</code>
        {{ t('plugin-detail-uninstall-prompt-suffix') }}
      </p>
      <template #footer>
        <div class="flex justify-end gap-2">
          <Button variant="ghost" @click="showUninstallConfirm = false">
            {{ t('plugin-detail-uninstall-cancel') }}
          </Button>
          <Button variant="danger" @click="executeUninstall">
            {{ t('plugin-detail-uninstall-confirm') }}
          </Button>
        </div>
      </template>
    </Modal>
  </div>
</template>
