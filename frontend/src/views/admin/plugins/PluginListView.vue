<script setup lang="ts">
/**
 * Installed plugins list. Single-column house layout: header + primary
 * action, signing warnings banner, a top bar (search + lifecycle-state
 * SegmentedControl), a slim trust-tier strip, and a stacked list of
 * plugin rows. Per-row actions are state-aware: an Active or Disabled
 * plugin gets a toggle + settings/source/uninstall; Quarantined /
 * Uninstalled rows are read-only and use the detail view's flows.
 *
 * Composition:
 *   - usePlugins()          data + lifecycle mutations
 *   - useSigningOverview()  instance trust posture (warnings + tier counts)
 *   - PluginCard            renders the row identity; actions via slot
 */
import { computed, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useBrandingStore } from '@/stores/branding';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import SearchInput from '@/components/common/SearchInput.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import PluginCard from '@/components/plugins/PluginCard.vue';
import PluginTrustBadge from '@/components/plugins/PluginTrustBadge.vue';
import { usePlugins } from '@/composables/usePlugins';
import { usePluginAdminConfig } from '@/composables/usePluginAdminConfig';
import { useSigningOverview } from '@/composables/useSigningOverview';
import type { Plugin, PluginState } from '@nosdesk/core/types/plugin';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

const router = useRouter();
const brandingStore = useBrandingStore();
const { plugins, loading, error, load, toggle, uninstall } = usePlugins();
const { config: adminConfig, load: loadAdminConfig } = usePluginAdminConfig();
const signing = useSigningOverview();

const searchQuery = ref('');
type StateFilter = 'all' | PluginState;
const stateFilter = ref<StateFilter>('all');

const successMessage = ref('');
const actionError = ref('');
const uninstallTarget = ref<Plugin | null>(null);
const uninstalling = ref(false);

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
    awaiting_consent: 0,
  };
  for (const p of plugins.value) counts[p.state]++;
  return counts;
});

const filterOptions = computed<{ value: StateFilter; label: string }[]>(() => [
  { value: 'all', label: `All ${plugins.value.length}` },
  { value: 'installed', label: `${t('plugin-state-active')} ${stateCounts.value.installed}` },
  { value: 'awaiting_consent', label: `${t('plugin-state-awaiting-consent')} ${stateCounts.value.awaiting_consent}` },
  { value: 'disabled', label: `${t('plugin-state-disabled')} ${stateCounts.value.disabled}` },
  { value: 'quarantined', label: `${t('plugin-state-quarantined')} ${stateCounts.value.quarantined}` },
  { value: 'uninstalled', label: `${t('plugin-state-uninstalled')} ${stateCounts.value.uninstalled}` },
]);

const visiblePlugins = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  return plugins.value.filter((p) => {
    if (stateFilter.value !== 'all' && p.state !== stateFilter.value) return false;
    if (!q) return true;
    return (
      p.name.toLowerCase().includes(q) ||
      p.display_name.toLowerCase().includes(q) ||
      (p.description ?? '').toLowerCase().includes(q)
    );
  });
});

const filtersActive = computed(
  () => searchQuery.value.trim().length > 0 || stateFilter.value !== 'all',
);

function resetFilters() {
  searchQuery.value = '';
  stateFilter.value = 'all';
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
  uninstalling.value = true;
  try {
    await uninstall(uninstallTarget.value);
    announce('Plugin uninstalled');
    uninstallTarget.value = null;
  } catch {
    actionError.value = 'Failed to uninstall plugin';
  } finally {
    uninstalling.value = false;
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="mx-auto flex w-full max-w-8xl flex-col gap-4 px-4 py-4 sm:px-6">
      <!-- Header -->
      <header class="mb-2 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 class="text-xl font-bold text-primary sm:text-2xl">{{ t('admin-plugins-list-title') }}</h1>
          <p class="mt-1 text-sm text-secondary sm:text-base">
            Manage installed plugins. Browse the
            <RouterLink to="/admin/plugins/registry" class="text-accent hover:underline">registry</RouterLink>
            <template v-if="adminConfig?.web_sideload_enabled">
              for one-click installs, or
              <RouterLink to="/admin/plugins/install" class="text-accent hover:underline">sideload a signed zip</RouterLink>.
            </template>
            <template v-else> for one-click installs.</template>
          </p>
        </div>
        <Button
          v-if="plugins.length > 0"
          icon="search"
          class="self-start sm:self-auto"
          @click="router.push('/admin/plugins/registry')"
        >
          Browse registry
        </Button>
      </header>

      <!-- Signing warnings: dev-mode / legacy-unsigned (caution) + revoked (critical) -->
      <div
        v-for="(w, i) in signing.warnings.value"
        :key="i"
        class="flex items-start gap-2 rounded-lg border px-3 py-2 text-sm"
        :class="w.tone === 'critical'
          ? 'border-status-error/50 bg-status-error/10 text-status-error'
          : 'border-status-warning/50 bg-status-warning/10 text-status-warning'"
      >
        <Icon name="warning" class="mt-0.5 shrink-0" />
        <span>{{ w.message }}</span>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="error || actionError" type="error" :message="error ?? actionError" />

      <!-- Loading skeleton (mirrors a plugin row) -->
      <div v-if="loading && plugins.length === 0" role="status" aria-label="Loading plugins" class="flex flex-col gap-2.5">
        <div v-for="n in 3" :key="n" class="flex items-start gap-3 rounded-xl border border-default bg-surface p-4">
          <SkeletonBar class="h-10 w-10 rounded-lg" />
          <div class="flex flex-1 flex-col gap-2">
            <SkeletonBar class="h-4 w-40" />
            <SkeletonBar class="h-3 w-64" />
          </div>
        </div>
      </div>

      <!-- No plugins installed at all -->
      <EmptyState
        v-else-if="plugins.length === 0"
        icon="plugin"
        :title="t('empty-plugins-installed-title')"
        :description="t('empty-plugins-installed-description', { app: brandingStore.appName })"
        action-label="Browse registry"
        variant="card"
        @action="router.push('/admin/plugins/registry')"
      />

      <template v-else>
        <!-- Top bar: search + lifecycle-state filter -->
        <div class="flex flex-col gap-3 sm:flex-row sm:items-center">
          <SearchInput
            v-model="searchQuery"
            :placeholder="t('admin-plugins-list-search-placeholder')"
            class="sm:max-w-xs"
          />
          <div class="-mx-4 overflow-x-auto px-4 sm:mx-0 sm:px-0">
            <SegmentedControl
              v-model="stateFilter"
              :options="filterOptions"
              size="sm"
              :aria-label="t('admin-plugins-list-aria-filter')"
            />
          </div>
        </div>

        <!-- Meta row: trust-tier strip + result count -->
        <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
          <div v-if="signing.overview.value" class="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span
              v-for="tier in signing.denseTiers.value"
              :key="tier.trust_level"
              class="inline-flex items-center gap-1.5"
            >
              <PluginTrustBadge :level="tier.trust_level" />
              <span class="text-xs tabular-nums text-tertiary">{{ tier.count }}</span>
            </span>
          </div>
          <p class="text-xs text-tertiary" aria-live="polite">
            {{ visiblePlugins.length }} of {{ plugins.length }} plugin{{ plugins.length === 1 ? '' : 's' }}
          </p>
        </div>

        <!-- Filtered to empty -->
        <div
          v-if="visiblePlugins.length === 0"
          role="status"
          class="rounded-xl border border-default bg-surface p-10 text-center"
        >
          <p class="text-sm text-secondary">No plugins match those filters.</p>
          <button
            v-if="filtersActive"
            type="button"
            class="mt-3 text-sm text-accent hover:underline focus:underline focus:outline-none"
            @click="resetFilters"
          >
            Reset filters
          </button>
        </div>

        <!-- Plugin rows -->
        <ul v-else class="flex flex-col gap-2.5" role="list">
          <li v-for="plugin in visiblePlugins" :key="plugin.uuid">
            <PluginCard :plugin="plugin">
              <template #actions>
                <ToggleSwitch
                  v-if="plugin.state === 'installed' || plugin.state === 'disabled'"
                  size="sm"
                  :model-value="plugin.state === 'installed'"
                  :aria-label="plugin.state === 'installed'
                    ? `Disable ${plugin.display_name}`
                    : `Enable ${plugin.display_name}`"
                  @update:model-value="handleToggle(plugin as Plugin)"
                />

                <RouterLink
                  :to="`/admin/plugins/${plugin.uuid}`"
                  class="rounded-md p-1.5 text-secondary transition-colors hover:bg-surface-hover hover:text-primary sm:rounded-lg sm:p-2"
                  :aria-label="`Open ${plugin.display_name} details`"
                >
                  <Icon name="settings" />
                </RouterLink>

                <a
                  v-if="plugin.manifest.repository"
                  :href="plugin.manifest.repository"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="rounded-md p-1.5 text-secondary transition-colors hover:bg-surface-hover hover:text-primary sm:rounded-lg sm:p-2"
                  :aria-label="`View source for ${plugin.display_name}`"
                  @click.stop
                >
                  <Icon name="openExternal" />
                </a>

                <button
                  type="button"
                  class="rounded-md p-1.5 text-secondary transition-colors hover:bg-status-error/10 hover:text-status-error sm:rounded-lg sm:p-2"
                  :aria-label="`Uninstall ${plugin.display_name}`"
                  @click="uninstallTarget = plugin as Plugin"
                >
                  <Icon name="trash" />
                </button>
              </template>
            </PluginCard>
          </li>
        </ul>
      </template>
    </div>

    <!-- Uninstall confirmation -->
    <ConfirmModal
      :show="uninstallTarget !== null"
      variant="danger"
      :title="t('admin-plugins-list-uninstall-title')"
      :message="uninstallTarget
        ? `Uninstall ${uninstallTarget.display_name}? The plugin's on_uninstall policy decides whether its data is preserved or removed.`
        : ''"
      confirm-label="Uninstall"
      :loading="uninstalling"
      @confirm="executeUninstall"
      @close="uninstallTarget = null"
    />
  </div>
</template>
