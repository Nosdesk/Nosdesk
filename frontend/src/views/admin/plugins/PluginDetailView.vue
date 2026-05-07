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
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import PluginIcon from '@/components/plugins/PluginIcon.vue';
import PluginStateBadge from '@/components/plugins/PluginStateBadge.vue';
import PluginTrustBadge from '@/components/plugins/PluginTrustBadge.vue';
import pluginService from '@/services/pluginService';
import { unloadPlugin } from '@/plugins/loader';
import { logger } from '@/utils/logger';
import type { Plugin, PluginSetting } from '@/types/plugin';

const route = useRoute();
const router = useRouter();

const plugin = ref<Plugin | null>(null);
const loading = ref(true);
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

watch(uuid, () => void load(), { immediate: false });
onMounted(load);

async function load() {
  loading.value = true;
  errorMessage.value = '';
  try {
    plugin.value = await pluginService.getPlugin(uuid.value);
    await loadSettings();
  } catch (e) {
    errorMessage.value = 'Failed to load plugin';
    logger.error('Failed to load plugin', { error: e });
  } finally {
    loading.value = false;
  }
}

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
    // edited right now — a previously-set secret stays valid.
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
    announce(`${keys.length === 1 ? 'Setting' : 'Settings'} saved`);
  } catch (e) {
    saveError.value = 'Failed to save settings. Try again.';
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
    plugin.value = await pluginService.updatePlugin(plugin.value.uuid, { enabled: enable });
    if (plugin.value.state !== 'installed') unloadPlugin(plugin.value.uuid);
    announce(`Plugin ${plugin.value.state === 'installed' ? 'enabled' : 'disabled'}`);
  } catch (e) {
    errorMessage.value = 'Failed to toggle plugin';
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
    errorMessage.value = 'Failed to uninstall plugin';
    logger.error('Failed to uninstall', { error: e });
  }
}
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
      Back to plugins
    </RouterLink>

    <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <LoadingSpinner v-if="loading" text="Loading plugin..." />

    <template v-else-if="plugin">
      <!-- Header card -->
      <header class="flex flex-col gap-4 rounded-xl border border-default bg-surface p-5 sm:flex-row sm:items-start">
        <PluginIcon :uuid="plugin.uuid" :alt="plugin.display_name" size="lg" />
        <div class="min-w-0 flex-1">
          <h1 class="text-2xl font-bold text-primary">{{ plugin.display_name }}</h1>
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

      <!-- Lifecycle actions -->
      <section
        aria-labelledby="lifecycle-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="lifecycle-heading" class="mb-3 font-semibold text-primary">Lifecycle</h2>
        <div class="flex flex-wrap gap-2">
          <button
            v-if="plugin.state === 'installed' || plugin.state === 'disabled'"
            type="button"
            @click="toggle"
            class="rounded-lg border border-default bg-surface-alt px-3 py-1.5 text-sm text-primary transition-colors hover:bg-surface-hover focus:ring-2 focus:ring-accent/30 focus:outline-none"
          >
            {{ plugin.state === 'installed' ? 'Disable' : 'Enable' }}
          </button>
          <button
            type="button"
            @click="showUninstallConfirm = true"
            class="rounded-lg border border-status-error/30 bg-status-error/10 px-3 py-1.5 text-sm text-status-error transition-colors hover:bg-status-error/20 focus:ring-2 focus:ring-status-error/30 focus:outline-none"
          >
            Uninstall
          </button>
        </div>
      </section>

      <!-- Settings -->
      <section
        v-if="plugin.manifest.settings.length > 0"
        aria-labelledby="settings-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="settings-heading" class="mb-4 font-semibold text-primary">Settings</h2>

        <LoadingSpinner v-if="settingsLoading" text="Loading settings..." />

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
                {{ def.label }}
                <span v-if="def.required" class="text-status-error" aria-label="required">*</span>
              </label>
              <p v-if="def.description" class="mt-1 text-xs text-tertiary">
                {{ def.description }}
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
                  <span class="text-sm text-secondary">Configured</span>
                </div>
                <button
                  type="button"
                  @click="editSecret(def.key)"
                  class="rounded-lg px-3 py-2 text-sm text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                >
                  Update
                </button>
              </div>
              <div v-else class="flex items-center gap-2">
                <input
                  :id="`setting-${def.key}`"
                  v-model="settingValues[def.key]"
                  type="password"
                  autocomplete="off"
                  spellcheck="false"
                  :placeholder="editingSecrets.has(def.key) ? 'Enter new value' : 'Enter value'"
                  class="flex-1 rounded-lg border border-default bg-surface-alt px-3 py-2 text-primary placeholder-tertiary focus:border-transparent focus:ring-2 focus:ring-accent focus:outline-none"
                />
                <button
                  v-if="editingSecrets.has(def.key)"
                  type="button"
                  @click="cancelEditSecret(def.key)"
                  class="rounded-lg px-3 py-2 text-sm text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                >
                  Cancel
                </button>
              </div>
            </div>

            <input
              v-else-if="def.type === 'number'"
              :id="`setting-${def.key}`"
              v-model.number="settingValues[def.key]"
              type="number"
              class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-primary focus:border-transparent focus:ring-2 focus:ring-accent focus:outline-none"
            />

            <label v-else-if="def.type === 'boolean'" class="flex items-center gap-2">
              <input
                :id="`setting-${def.key}`"
                v-model="settingValues[def.key]"
                type="checkbox"
                class="rounded border-default text-accent focus:ring-accent"
              />
              <span class="text-sm text-secondary">Enabled</span>
            </label>

            <select
              v-else-if="def.type === 'select' && def.options"
              :id="`setting-${def.key}`"
              v-model="settingValues[def.key]"
              class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-primary focus:border-transparent focus:ring-2 focus:ring-accent focus:outline-none"
            >
              <option v-for="opt in def.options" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>
          </div>

          <p v-if="saveError" class="text-sm text-status-error" role="alert">
            {{ saveError }}
          </p>

          <footer
            class="flex items-center justify-between gap-3 border-t border-default pt-4"
          >
            <p class="text-xs text-tertiary" aria-live="polite">
              <template v-if="missingRequired.length > 0">
                {{ missingRequired.length }} required
                {{ missingRequired.length === 1 ? 'field' : 'fields' }} missing
              </template>
              <template v-else-if="isDirty">
                {{ dirtyKeys.length }} unsaved
                {{ dirtyKeys.length === 1 ? 'change' : 'changes' }}
              </template>
              <template v-else>All changes saved</template>
            </p>
            <div class="flex items-center gap-2">
              <button
                v-if="isDirty"
                type="button"
                @click="discardSettingChanges"
                :disabled="saveInFlight"
                class="px-3 py-1.5 text-sm text-secondary transition-colors hover:text-primary disabled:opacity-50"
              >
                Discard
              </button>
              <button
                type="submit"
                :disabled="!canSave"
                class="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
              >
                {{ saveInFlight ? 'Saving...' : 'Save changes' }}
              </button>
            </div>
          </footer>
        </form>
      </section>

      <!-- Metadata -->
      <section
        aria-labelledby="metadata-heading"
        class="rounded-xl border border-default bg-surface p-5"
      >
        <h2 id="metadata-heading" class="mb-3 font-semibold text-primary">Metadata</h2>
        <dl class="grid grid-cols-1 gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
          <div>
            <dt class="text-xs tracking-wide text-tertiary uppercase">Source</dt>
            <dd class="mt-1 text-secondary">{{ plugin.source }}</dd>
          </div>
          <div>
            <dt class="text-xs tracking-wide text-tertiary uppercase">Permissions</dt>
            <dd class="mt-1 text-secondary">{{ plugin.manifest.permissions.length }} declared</dd>
          </div>
          <div v-if="plugin.manifest.repository" class="sm:col-span-2">
            <dt class="text-xs tracking-wide text-tertiary uppercase">Repository</dt>
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

    <!-- Uninstall confirmation -->
    <Teleport to="body">
      <div
        v-if="showUninstallConfirm && plugin"
        class="fixed inset-0 z-overlay flex items-center justify-center bg-black/50 p-4"
        role="dialog"
        aria-modal="true"
        aria-labelledby="uninstall-title"
        @click.self="showUninstallConfirm = false"
      >
        <div class="w-full max-w-md rounded-xl border border-default bg-surface p-5">
          <h2 id="uninstall-title" class="font-semibold text-primary">Uninstall plugin</h2>
          <p class="mt-3 text-sm text-secondary">
            Uninstall <strong class="text-primary">{{ plugin.display_name }}</strong
            >? The plugin's
            <code class="rounded bg-surface-alt px-1 text-xs">on_uninstall</code>
            policy decides whether its data is preserved or removed.
          </p>
          <div class="mt-5 flex justify-end gap-2">
            <button
              type="button"
              @click="showUninstallConfirm = false"
              class="px-4 py-2 text-sm text-secondary transition-colors hover:text-primary"
            >
              Cancel
            </button>
            <button
              type="button"
              @click="executeUninstall"
              class="rounded-lg bg-status-error px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-status-error/90"
            >
              Uninstall
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
