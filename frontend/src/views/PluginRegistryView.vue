<script setup lang="ts">
/**
 * Browse plugins published to the Nosdesk registry. Layout mirrors
 * the installed-plugins list (sticky sidebar with search + tier
 * filters, scrolling card list on the right) so the two admin
 * views feel like the same surface seen from different angles.
 *
 * Each registry row shows whether the plugin is already installed.
 * Installed rows route to the detail view via Manage; un-installed
 * rows kick off the tier-aware install confirmation flow:
 *   - official: one click, no modal
 *   - verified: single confirmation modal showing publisher + fingerprint
 *   - community: two-step modal, warning + type-to-confirm
 *   - local: not surfaced in the registry (CLI-only path)
 */
import { computed, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import PluginIcon from '@/components/plugins/PluginIcon.vue';
import PluginTrustBadge from '@/components/plugins/PluginTrustBadge.vue';
import { usePluginAdminConfig } from '@/composables/usePluginAdminConfig';
import pluginService from '@/services/pluginService';
import { logger } from '@/utils/logger';
import type {
  Plugin,
  RegistryState,
  RegistrySnapshot,
  RegistryPlugin,
  RegistryPublisher,
  PluginTrustLevel,
} from '@/types/plugin';

const router = useRouter();

const state = ref<RegistryState | null>(null);
const installedPlugins = ref<Plugin[]>([]);
const isLoading = ref(true);
const isRefreshing = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

const searchQuery = ref('');
const activeTiers = ref<Set<PluginTrustLevel>>(new Set());

const pendingInstall = ref<RegistryPlugin | null>(null);
const communityConfirmText = ref('');
const installing = ref<string | null>(null);
const pendingFingerprint = ref('');

const { config: adminConfig, load: loadAdminConfig } = usePluginAdminConfig();

onMounted(async () => {
  await Promise.all([loadRegistry(), loadInstalled(), loadAdminConfig()]);
});

async function loadRegistry() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    state.value = await pluginService.getRegistry();
  } catch (err: unknown) {
    errorMessage.value = 'Failed to load the registry.';
    logger.error('Failed to load registry', { error: err });
  } finally {
    isLoading.value = false;
  }
}

async function loadInstalled() {
  try {
    installedPlugins.value = await pluginService.listPlugins();
  } catch (err: unknown) {
    logger.error('Failed to load installed plugins', { error: err });
  }
}

async function retryRegistrySync() {
  isRefreshing.value = true;
  errorMessage.value = '';
  try {
    state.value = await pluginService.refreshRegistry();
  } catch (err: unknown) {
    errorMessage.value = 'Failed to retry the registry sync.';
    logger.error('Failed to refresh registry', { error: err });
  } finally {
    isRefreshing.value = false;
  }
}

const snapshot = computed<RegistrySnapshot | null>(() =>
  state.value?.status === 'available' ? state.value.snapshot : null,
);

const publishersByKey = computed<Map<string, RegistryPublisher>>(() => {
  const map = new Map<string, RegistryPublisher>();
  for (const p of snapshot.value?.publishers.publishers ?? []) {
    map.set(p.pubkey, p);
  }
  return map;
});

const installedByName = computed<Map<string, Plugin>>(() => {
  const map = new Map<string, Plugin>();
  for (const p of installedPlugins.value) map.set(p.name, p);
  return map;
});

const tierCounts = computed<Record<PluginTrustLevel, number>>(() => {
  const counts: Record<PluginTrustLevel, number> = {
    official: 0,
    verified: 0,
    community: 0,
    local: 0,
  };
  for (const p of snapshot.value?.index.plugins ?? []) counts[p.tier]++;
  return counts;
});

const filteredPlugins = computed<RegistryPlugin[]>(() => {
  if (!snapshot.value) return [];
  const q = searchQuery.value.trim().toLowerCase();
  return snapshot.value.index.plugins.filter((p) => {
    if (activeTiers.value.size > 0 && !activeTiers.value.has(p.tier)) return false;
    if (!q) return true;
    return (
      p.name.toLowerCase().includes(q) ||
      p.display_name.toLowerCase().includes(q) ||
      (p.description?.toLowerCase().includes(q) ?? false)
    );
  });
});

const filtersActive = computed(
  () => searchQuery.value.trim().length > 0 || activeTiers.value.size > 0,
);

function publisherFor(plugin: RegistryPlugin): RegistryPublisher | null {
  if (plugin.tier === 'official') return null;
  return publishersByKey.value.get(plugin.publisher_pubkey) ?? null;
}

function publisherName(plugin: RegistryPlugin): string {
  if (plugin.tier === 'official') return 'Nosdesk';
  return publisherFor(plugin)?.display_name ?? 'Unknown publisher';
}

function toggleTier(tier: PluginTrustLevel) {
  const next = new Set(activeTiers.value);
  if (next.has(tier)) next.delete(tier);
  else next.add(tier);
  activeTiers.value = next;
}

function resetFilters() {
  searchQuery.value = '';
  activeTiers.value = new Set();
}

/** Bounded FIFO so a long-running session against a large registry
 * doesn't grow this map without limit. The cap is generous: the
 * Nosdesk public registry will never approach 500 publishers in
 * one snapshot, so eviction effectively never fires in practice. */
const FINGERPRINT_CACHE_LIMIT = 500;
const fingerprintCache = new Map<string, string>();

async function pubkeyFingerprint(pubkeyB64: string): Promise<string> {
  const cached = fingerprintCache.get(pubkeyB64);
  if (cached) return cached;
  const raw = Uint8Array.from(atob(pubkeyB64), (c) => c.charCodeAt(0));
  const digest = await crypto.subtle.digest('SHA-256', raw);
  const fp = Array.from(new Uint8Array(digest).slice(0, 8))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
  if (fingerprintCache.size >= FINGERPRINT_CACHE_LIMIT) {
    // FIFO eviction: oldest insertion wins. Map preserves insertion
    // order so the first key is the oldest.
    const oldest = fingerprintCache.keys().next().value;
    if (oldest !== undefined) fingerprintCache.delete(oldest);
  }
  fingerprintCache.set(pubkeyB64, fp);
  return fp;
}

async function startInstall(plugin: RegistryPlugin) {
  pendingInstall.value = plugin;
  communityConfirmText.value = '';
  pendingFingerprint.value = await pubkeyFingerprint(plugin.publisher_pubkey);

  if (plugin.tier === 'official') {
    await confirmInstall();
  }
}

async function confirmInstall() {
  if (!pendingInstall.value) return;
  const plugin = pendingInstall.value;

  if (
    plugin.tier === 'community' &&
    communityConfirmText.value.trim() !== plugin.name
  ) {
    errorMessage.value = 'Type the plugin name exactly to confirm installation.';
    return;
  }

  installing.value = plugin.name;
  errorMessage.value = '';
  try {
    const installed = await pluginService.installFromRegistry({
      plugin_name: plugin.name,
    });
    installedPlugins.value = [...installedPlugins.value, installed];
    successMessage.value = `Installed ${installed.display_name} v${installed.version}`;
    setTimeout(() => (successMessage.value = ''), 4000);
    pendingInstall.value = null;
  } catch (err: unknown) {
    const message =
      (err as { response?: { data?: string } })?.response?.data ?? 'Install failed.';
    errorMessage.value = typeof message === 'string' ? message : 'Install failed.';
    logger.error('Registry install failed', { error: err, plugin: plugin.name });
  } finally {
    installing.value = null;
  }
}

function cancelInstall() {
  pendingInstall.value = null;
  communityConfirmText.value = '';
}

const TIER_FILTER_ORDER: PluginTrustLevel[] = ['official', 'verified', 'community'];

const TIER_LABELS: Record<PluginTrustLevel, string> = {
  official: 'Official',
  verified: 'Verified',
  community: 'Community',
  local: 'Local',
};

function formatRelative(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const minutes = Math.round(diff / 60_000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} hr ago`;
  const days = Math.round(hours / 24);
  return `${days} day${days === 1 ? '' : 's'} ago`;
}
</script>

<template>
  <div class="mx-auto flex w-full max-w-8xl flex-1 flex-col gap-4 px-4 py-4 sm:px-6">
    <!-- Header -->
    <header class="mb-2 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <RouterLink
          to="/admin/plugins"
          class="mb-1.5 inline-flex items-center gap-1.5 text-sm text-secondary transition-colors hover:text-primary"
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
          Installed plugins
        </RouterLink>
        <h1 class="text-xl font-bold text-primary sm:text-2xl">Plugin registry</h1>
        <p class="mt-1 text-sm text-secondary sm:text-base">
          Browse and install plugins published to <code
            class="rounded bg-surface-alt px-1.5 py-0.5 font-mono text-xs"
          >nosdesk.com/registry</code>. Signatures are verified against the Nosdesk root key before
          any bundle executes.
        </p>
      </div>
      <button
        v-if="snapshot"
        type="button"
        @click="retryRegistrySync"
        :disabled="isRefreshing"
        class="flex items-center gap-1.5 self-start rounded-lg border border-default bg-surface px-3 py-1.5 text-sm font-medium text-primary transition-colors hover:bg-surface-hover disabled:opacity-50 sm:self-auto"
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="h-4 w-4"
          :class="{ 'animate-spin': isRefreshing }"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="2"
          aria-hidden="true"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
          />
        </svg>
        {{ isRefreshing ? 'Refreshing' : 'Refresh' }}
      </button>
    </header>

    <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <LoadingSpinner v-if="isLoading" text="Loading registry..." />

    <!-- Sync disabled: operator opted out via NOSDESK_REGISTRY_URL=. -->
    <EmptyState
      v-else-if="state?.status === 'disabled'"
      icon="plugin"
      title="Registry sync is disabled"
      :description="
        adminConfig?.web_sideload_enabled
          ? 'This instance has NOSDESK_REGISTRY_URL set to empty, so it isn\'t fetching the published plugin catalog. You can still sideload a signed zip.'
          : 'This instance has NOSDESK_REGISTRY_URL set to empty, so it isn\'t fetching the published plugin catalog. Use the CLI to install local-signed plugins.'
      "
      :action-label="adminConfig?.web_sideload_enabled ? 'Sideload signed zip' : undefined"
      variant="card"
      @action="router.push('/admin/plugins/install')"
    />

    <!-- First sync hasn't completed yet. -->
    <EmptyState
      v-else-if="state?.status === 'pending'"
      icon="plugin"
      title="Registry is syncing"
      description="The instance is fetching the published plugin catalog. This usually completes within a few seconds of boot."
      action-label="Retry now"
      variant="card"
      @action="retryRegistrySync"
    />

    <!-- Sync attempted and errored. -->
    <EmptyState
      v-else-if="state?.status === 'failed'"
      icon="plugin"
      title="Registry sync failed"
      :description="`${state.reason}. Retry now to fetch again, or wait for the next scheduled attempt.`"
      action-label="Retry now"
      variant="card"
      @action="retryRegistrySync"
    />

    <div v-else-if="snapshot" class="lg:grid lg:grid-cols-[16rem_1fr] lg:gap-6">
      <!-- Sidebar: search + tier filters -->
      <aside class="mb-6 lg:sticky lg:top-4 lg:mb-0 lg:self-start" aria-label="Filter registry">
        <div class="flex flex-col gap-4 rounded-xl border border-default bg-surface p-4">
          <div class="relative">
            <label for="registry-search" class="sr-only">Search plugins</label>
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
              id="registry-search"
              v-model="searchQuery"
              type="search"
              placeholder="Search plugins"
              class="w-full rounded-lg border border-default bg-surface-alt py-2 pr-3 pl-9 text-sm text-primary placeholder:text-tertiary focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
            />
          </div>

          <fieldset>
            <legend class="text-xs font-semibold tracking-wide text-tertiary uppercase">
              Trust tier
            </legend>
            <ul class="mt-2 flex flex-col gap-1" role="list">
              <li
                v-for="tier in TIER_FILTER_ORDER"
                :key="tier"
                class="flex items-center gap-2.5 rounded px-2 py-1.5 text-sm hover:bg-surface-alt focus-within:bg-surface-alt"
              >
                <Checkbox
                  size="sm"
                  :model-value="activeTiers.has(tier)"
                  :label="TIER_LABELS[tier]"
                  class="flex-1"
                  @update:model-value="toggleTier(tier)"
                />
                <span class="text-xs tabular-nums text-tertiary">{{ tierCounts[tier] }}</span>
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

          <p class="border-t border-default pt-3 text-xs text-tertiary">
            Snapshot fetched {{ formatRelative(snapshot.fetched_at) }}
          </p>
        </div>
      </aside>

      <!-- Card list -->
      <section :aria-busy="isRefreshing">
        <p class="mb-3 text-sm text-tertiary" aria-live="polite">
          {{ filteredPlugins.length }} of {{ snapshot.index.plugins.length }}
          plugin{{ snapshot.index.plugins.length === 1 ? '' : 's' }}
        </p>

        <div
          v-if="filteredPlugins.length === 0"
          role="status"
          class="rounded-xl border border-default bg-surface p-10 text-center"
        >
          <p class="text-sm text-secondary">No plugins match those filters.</p>
          <button
            v-if="filtersActive"
            type="button"
            @click="resetFilters"
            class="mt-3 text-sm text-accent hover:underline focus:underline focus:outline-none"
          >
            Reset filters
          </button>
        </div>

        <ul v-else class="flex flex-col gap-2.5" role="list">
          <li v-for="plugin in filteredPlugins" :key="plugin.name">
            <article class="overflow-hidden rounded-xl border border-default bg-surface">
              <div class="p-4">
                <div class="flex items-start gap-3">
                  <PluginIcon :src="plugin.icon_url ?? undefined" :alt="plugin.display_name" />

                  <div class="min-w-0 flex-1">
                    <header class="flex flex-wrap items-center gap-1.5 sm:gap-2">
                      <h3 class="font-semibold text-primary">{{ plugin.display_name }}</h3>
                      <code
                        v-if="plugin.versions[0]"
                        class="rounded bg-surface-alt px-1.5 py-0.5 font-mono text-xs text-secondary"
                      >
                        v{{ plugin.versions[0].version }}
                      </code>
                      <PluginTrustBadge :level="plugin.tier" />
                      <span
                        v-if="installedByName.has(plugin.name)"
                        class="inline-flex items-center gap-1 rounded bg-status-success/10 px-1.5 py-0.5 text-xs font-medium text-status-success"
                      >
                        <svg
                          xmlns="http://www.w3.org/2000/svg"
                          class="h-3 w-3"
                          fill="none"
                          viewBox="0 0 24 24"
                          stroke="currentColor"
                          stroke-width="2.5"
                          aria-hidden="true"
                        >
                          <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                        </svg>
                        Installed
                      </span>
                    </header>

                    <p
                      v-if="plugin.description"
                      class="mt-1.5 line-clamp-2 text-sm text-secondary"
                    >
                      {{ plugin.description }}
                    </p>

                    <dl
                      class="mt-2 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-xs text-tertiary"
                    >
                      <dt class="sr-only">Plugin name</dt>
                      <dd>
                        <code class="rounded bg-surface-alt px-1.5 py-0.5 font-mono">
                          {{ plugin.name }}
                        </code>
                      </dd>
                      <span aria-hidden="true" class="text-border">·</span>
                      <dt class="sr-only">Publisher</dt>
                      <dd>by {{ publisherName(plugin) }}</dd>
                      <template v-if="plugin.homepage">
                        <span aria-hidden="true" class="text-border">·</span>
                        <dt class="sr-only">Homepage</dt>
                        <dd>
                          <a
                            :href="plugin.homepage"
                            target="_blank"
                            rel="noopener noreferrer"
                            class="inline-flex items-center gap-1 hover:text-secondary"
                            @click.stop
                          >
                            Homepage
                            <svg
                              xmlns="http://www.w3.org/2000/svg"
                              class="h-3 w-3"
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
                        </dd>
                      </template>
                    </dl>
                  </div>

                  <div class="flex flex-shrink-0 items-center gap-1.5">
                    <RouterLink
                      v-if="installedByName.get(plugin.name)"
                      :to="`/admin/plugins/${installedByName.get(plugin.name)!.uuid}`"
                      class="rounded-lg border border-default bg-surface px-3 py-1.5 text-sm font-medium text-primary transition-colors hover:bg-surface-hover"
                    >
                      Manage
                    </RouterLink>
                    <button
                      v-else
                      type="button"
                      :disabled="installing === plugin.name"
                      @click="startInstall(plugin)"
                      class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
                    >
                      {{ installing === plugin.name ? 'Installing...' : 'Install' }}
                    </button>
                  </div>
                </div>
              </div>
            </article>
          </li>
        </ul>
      </section>
    </div>

    <Modal
      v-if="pendingInstall && pendingInstall.tier !== 'official'"
      :show="true"
      :title="`Install ${pendingInstall.display_name}?`"
      @close="cancelInstall"
    >
      <div class="flex flex-col gap-3 text-sm">
        <div
          v-if="pendingInstall.tier === 'community'"
          class="rounded-lg border border-status-warning/30 bg-status-warning/10 p-3 text-status-warning"
        >
          <strong>Community plugin.</strong>
          Nosdesk does not vouch for the safety of community plugins beyond verifying
          the publisher's signature. Review the source before trusting it with your data.
        </div>
        <dl class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2">
          <dt class="text-xs text-tertiary">Publisher</dt>
          <dd class="text-primary">{{ publisherName(pendingInstall) }}</dd>
          <dt class="text-xs text-tertiary">Fingerprint</dt>
          <dd class="font-mono text-xs text-secondary">{{ pendingFingerprint }}</dd>
          <dt class="text-xs text-tertiary">Version</dt>
          <dd class="text-primary">v{{ pendingInstall.versions[0]?.version }}</dd>
        </dl>
        <label v-if="pendingInstall.tier === 'community'" class="flex flex-col gap-1">
          <span class="text-xs text-tertiary">
            Type
            <code class="rounded bg-surface-alt px-1 font-mono">{{ pendingInstall.name }}</code>
            to confirm
          </span>
          <input
            v-model="communityConfirmText"
            type="text"
            class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-sm text-primary placeholder:text-tertiary focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
            :placeholder="pendingInstall.name"
          />
        </label>
      </div>
      <template #footer>
        <button
          type="button"
          @click="cancelInstall"
          class="px-4 py-2 text-sm text-secondary transition-colors hover:text-primary"
        >
          Cancel
        </button>
        <button
          type="button"
          :disabled="
            installing === pendingInstall.name ||
            (pendingInstall.tier === 'community' &&
              communityConfirmText.trim() !== pendingInstall.name)
          "
          @click="confirmInstall"
          class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
        >
          {{ installing === pendingInstall.name ? 'Installing...' : 'Install' }}
        </button>
      </template>
    </Modal>
  </div>
</template>
