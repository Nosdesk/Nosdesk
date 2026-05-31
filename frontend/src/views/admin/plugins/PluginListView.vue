<script setup lang="ts">
/**
 * Installed plugins list. Sticky filter sidebar (search +
 * multi-select state checkboxes) + plugin grid. Per-card actions
 * are state-aware: a Disabled plugin shows Enable + Uninstall;
 * an Active plugin shows Disable + Settings + Source + Uninstall;
 * Quarantined / Uninstalled rows are read-only with their own
 * controls (restore, hard-delete, reinstall) on the detail view.
 *
 * Composition:
 *   - usePlugins() owns the data + lifecycle mutations
 *   - PluginCard renders the row identity
 *   - This view composes the header, sidebar, grid, and the
 *     uninstall-confirm dialog
 *
 * Keeping the dialog inline (vs extracting to a separate
 * component) is deliberate KISS: it's bound to a single piece of
 * local state and used in exactly one place.
 */
import { computed, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useBrandingStore } from '@/stores/branding';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

import AlertMessage from '@/components/common/AlertMessage.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import PluginCard from '@/components/plugins/PluginCard.vue';
import PluginSigningOverview from '@/components/plugins/PluginSigningOverview.vue';
import { usePlugins } from '@/composables/usePlugins';
import { usePluginAdminConfig } from '@/composables/usePluginAdminConfig';
import type { Plugin, PluginState } from '@/types/plugin';

const router = useRouter();
const brandingStore = useBrandingStore();
const { plugins, loading, error, load, toggle, uninstall } = usePlugins();
const { config: adminConfig, load: loadAdminConfig } = usePluginAdminConfig();

const searchQuery = ref('');
const activeStates = ref<Set<PluginState>>(new Set());

const successMessage = ref('');
const actionError = ref('');
const uninstallTarget = ref<Plugin | null>(null);

onMounted(() => {
  void load();
  void loadAdminConfig();
});

function announce(msg: string) {
  successMessage.value = msg;
  setTimeout(() => (successMessage.value = ''), 3000);
}

const stateCounts = computed(() => {
  const counts: Record<PluginState, number> = {
    installed: 0,
    disabled: 0,
    quarantined: 0,
    uninstalled: 0,
  };
  for (const p of plugins.value) counts[p.state]++;
  return counts;
});

const visiblePlugins = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  return plugins.value.filter((p) => {
    if (activeStates.value.size > 0 && !activeStates.value.has(p.state)) return false;
    if (!q) return true;
    return (
      p.name.toLowerCase().includes(q) ||
      p.display_name.toLowerCase().includes(q) ||
      (p.description ?? '').toLowerCase().includes(q)
    );
  });
});

const filtersActive = computed(
  () => searchQuery.value.trim().length > 0 || activeStates.value.size > 0,
);

function toggleState(state: PluginState) {
  const next = new Set(activeStates.value);
  if (next.has(state)) next.delete(state);
  else next.add(state);
  activeStates.value = next;
}

function resetFilters() {
  searchQuery.value = '';
  activeStates.value = new Set();
}

async function handleToggle(plugin: Plugin) {
  try {
    await toggle(plugin);
    const after = plugins.value.find((p) => p.uuid === plugin.uuid);
    announce(`Plugin ${after?.state === 'installed' ? 'enabled' : 'disabled'}`);
  } catch {
    actionError.value = 'Failed to update plugin';
  }
}

async function executeUninstall() {
  if (!uninstallTarget.value) return;
  const target = uninstallTarget.value;
  try {
    await uninstall(target);
    announce('Plugin uninstalled');
    uninstallTarget.value = null;
  } catch {
    actionError.value = 'Failed to uninstall plugin';
  }
}

const STATE_LABELS: Record<PluginState, string> = {
  installed: 'Active',
  disabled: 'Disabled',
  quarantined: 'Quarantined',
  uninstalled: 'Uninstalled',
};

const STATE_FILTER_ORDER: PluginState[] = ['installed', 'disabled', 'quarantined', 'uninstalled'];
</script>

<template>
  <div class="mx-auto flex w-full max-w-8xl flex-1 flex-col gap-4 px-4 py-4 sm:px-6">
    <!-- Header -->
    <header class="mb-2 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <h1 class="text-xl font-bold text-primary sm:text-2xl">{{ t('admin-plugins-list-title') }}</h1>
        <p class="mt-1 text-sm text-secondary sm:text-base">
          Manage installed plugins. Browse the
          <RouterLink to="/admin/plugins/registry" class="text-accent hover:underline">
            registry
          </RouterLink>
          <template v-if="adminConfig?.web_sideload_enabled">
            for one-click installs, or
            <RouterLink to="/admin/plugins/install" class="text-accent hover:underline">
              sideload a signed zip
            </RouterLink>
          </template>
          <template v-else>
            for one-click installs
          </template>
          .
        </p>
      </div>
      <RouterLink
        v-if="plugins.length > 0"
        to="/admin/plugins/registry"
        class="flex items-center gap-1.5 self-start rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-on-accent transition-colors hover:bg-accent-hover sm:self-auto"
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
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z"
          />
        </svg>
        Browse registry
      </RouterLink>
    </header>

    <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
    <AlertMessage v-if="error || actionError" type="error" :message="error ?? actionError" />

    <LoadingSpinner v-if="loading" text="Loading plugins..." />

    <EmptyState
      v-else-if="plugins.length === 0"
      icon="plugin"
      :title="$t('empty-plugins-installed-title')"
      :description="$t('empty-plugins-installed-description', { app: brandingStore.appName })"
      action-label="Browse registry"
      variant="card"
      @action="router.push('/admin/plugins/registry')"
    />

    <div v-else class="lg:grid lg:grid-cols-[16rem_1fr] lg:gap-6">
      <!-- Sidebar: search + state filters + signing inventory -->
      <aside class="mb-6 flex flex-col gap-4 lg:sticky lg:top-4 lg:mb-0 lg:self-start" :aria-label="t('admin-plugins-list-aria-filter')">
        <PluginSigningOverview />
        <div class="flex flex-col gap-4 rounded-xl border border-default bg-surface p-4">
          <div class="relative">
            <label for="plugin-search" class="sr-only">Search plugins</label>
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-tertiary"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2"
              aria-hidden="true"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z"
              />
            </svg>
            <input
              id="plugin-search"
              v-model="searchQuery"
              type="search"
              :placeholder="t('admin-plugins-list-search-placeholder')"
              class="w-full rounded-lg border border-default bg-surface-alt py-2 pr-3 pl-9 text-sm text-primary placeholder:text-tertiary focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
            />
          </div>

          <fieldset>
            <legend class="text-xs font-semibold tracking-wide text-tertiary uppercase">
              Lifecycle state
            </legend>
            <ul class="mt-2 flex flex-col gap-1" role="list">
              <li
                v-for="state in STATE_FILTER_ORDER"
                :key="state"
                class="flex items-center gap-2.5 rounded px-2 py-1.5 text-sm hover:bg-surface-alt focus-within:bg-surface-alt"
              >
                <Checkbox
                  size="sm"
                  :model-value="activeStates.has(state)"
                  :label="STATE_LABELS[state]"
                  class="flex-1"
                  @update:model-value="toggleState(state)"
                />
                <span class="text-xs tabular-nums text-tertiary">{{ stateCounts[state] }}</span>
              </li>
            </ul>
          </fieldset>

          <button
            v-if="filtersActive"
            type="button"
            @click="resetFilters"
            class="self-start text-xs text-accent hover:underline focus:underline focus:outline-none"
          >
            Reset filters
          </button>
        </div>
      </aside>

      <!-- Grid -->
      <section :aria-busy="loading">
        <p class="mb-3 text-sm text-tertiary" aria-live="polite">
          {{ visiblePlugins.length }} of {{ plugins.length }}
          plugin{{ plugins.length === 1 ? '' : 's' }}
        </p>

        <div
          v-if="visiblePlugins.length === 0"
          role="status"
          class="rounded-xl border border-default bg-surface p-10 text-center"
        >
          <p class="text-sm text-secondary">No plugins match those filters.</p>
          <button
            type="button"
            @click="resetFilters"
            class="mt-3 text-sm text-accent hover:underline focus:underline focus:outline-none"
          >
            Reset filters
          </button>
        </div>

        <ul v-else class="flex flex-col gap-2.5" role="list">
          <li v-for="plugin in visiblePlugins" :key="plugin.uuid">
            <PluginCard :plugin="plugin">
              <template #actions>
                <!-- Toggle: Installed <-> Disabled. Hidden for
                     Quarantined/Uninstalled (those use the
                     detail view's restore/reinstall flows). -->
                <button
                  v-if="plugin.state === 'installed' || plugin.state === 'disabled'"
                  type="button"
                  @click="handleToggle(plugin as Plugin)"
                  :aria-label="
                    plugin.state === 'installed'
                      ? `Disable ${plugin.display_name}`
                      : `Enable ${plugin.display_name}`
                  "
                  class="relative inline-flex h-5 w-9 cursor-pointer rounded-full border-2 border-transparent transition-colors focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-surface focus:outline-none"
                  :class="plugin.state === 'installed' ? 'bg-accent' : 'bg-border'"
                  :title="plugin.state === 'installed' ? 'Disable plugin' : 'Enable plugin'"
                >
                  <span
                    class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition"
                    :class="plugin.state === 'installed' ? 'translate-x-4' : 'translate-x-0'"
                  />
                </button>

                <RouterLink
                  :to="`/admin/plugins/${plugin.uuid}`"
                  class="rounded-lg p-2 text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                  :aria-label="`Open ${plugin.display_name} details`"
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
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
                    />
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
                    />
                  </svg>
                </RouterLink>

                <a
                  v-if="plugin.manifest.repository"
                  :href="plugin.manifest.repository"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="rounded-lg p-2 text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
                  :aria-label="`View source for ${plugin.display_name}`"
                  @click.stop
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
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
                    />
                  </svg>
                </a>

                <button
                  type="button"
                  @click="uninstallTarget = plugin as Plugin"
                  class="rounded-lg p-2 text-secondary transition-colors hover:bg-status-error/10 hover:text-status-error"
                  :aria-label="`Uninstall ${plugin.display_name}`"
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
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                    />
                  </svg>
                </button>
              </template>
            </PluginCard>
          </li>
        </ul>
      </section>
    </div>

    <!-- Uninstall confirmation -->
    <Modal
      :show="uninstallTarget !== null"
      :title="t('admin-plugins-list-uninstall-title')"
      size="sm"
      @close="uninstallTarget = null"
    >
      <div v-if="uninstallTarget" class="flex flex-col gap-4">
        <p class="text-secondary">
          Uninstall <strong class="text-primary">{{ uninstallTarget.display_name }}</strong
          >? The plugin's
          <code class="rounded bg-surface-alt px-1 text-xs">on_uninstall</code>
          policy decides whether its data is preserved or removed.
        </p>
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="uninstallTarget = null"
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
    </Modal>
  </div>
</template>
