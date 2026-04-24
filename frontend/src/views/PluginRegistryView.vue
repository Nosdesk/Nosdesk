<script setup lang="ts">
/**
 * Plugin Registry View
 *
 * Browse plugins published to the Nosdesk registry and install them
 * into this instance. The registry snapshot is fetched by the
 * backend on startup and every 24h; we just render what's cached.
 *
 * Install confirmation depth scales with tier:
 *   - official: one click
 *   - verified: single confirmation dialog showing publisher + fingerprint
 *   - community: two-step dialog, warning + type-to-confirm
 *   - local: not shown in the registry (CLI-only path)
 */

import { computed, onMounted, ref } from 'vue';
import pluginService from '@/services/pluginService';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import { logger } from '@/utils/logger';
import type {
  RegistrySnapshot,
  RegistryPlugin,
  RegistryPublisher,
  PluginTrustLevel,
} from '@/types/plugin';

const snapshot = ref<RegistrySnapshot | null>(null);
const isLoading = ref(true);
const errorMessage = ref('');
const successMessage = ref('');
const searchQuery = ref('');
const tierFilter = ref<PluginTrustLevel | 'all'>('all');

// Install confirmation state.
const pendingInstall = ref<RegistryPlugin | null>(null);
const communityConfirmText = ref('');
const installing = ref<string | null>(null);

onMounted(async () => {
  await loadRegistry();
});

async function loadRegistry() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    snapshot.value = await pluginService.getRegistry();
  } catch (err: unknown) {
    // 503 = snapshot not ready yet. Everything else is a real error.
    const status = (err as { response?: { status?: number } })?.response?.status;
    if (status === 503) {
      errorMessage.value =
        'Registry has not synced yet. The backend fetches on startup and every 24 hours. Retry in a moment.';
    } else {
      errorMessage.value = 'Failed to load the registry.';
    }
    logger.error('Failed to load registry', { error: err });
  } finally {
    isLoading.value = false;
  }
}

const publishersByKey = computed<Map<string, RegistryPublisher>>(() => {
  const map = new Map<string, RegistryPublisher>();
  if (snapshot.value) {
    for (const p of snapshot.value.publishers.publishers) {
      map.set(p.pubkey, p);
    }
  }
  return map;
});

const filteredPlugins = computed<RegistryPlugin[]>(() => {
  if (!snapshot.value) return [];
  const q = searchQuery.value.trim().toLowerCase();
  return snapshot.value.index.plugins.filter((p) => {
    if (tierFilter.value !== 'all' && p.tier !== tierFilter.value) return false;
    if (q) {
      return (
        p.name.toLowerCase().includes(q) ||
        p.display_name.toLowerCase().includes(q) ||
        (p.description?.toLowerCase().includes(q) ?? false)
      );
    }
    return true;
  });
});

function publisherFor(plugin: RegistryPlugin): RegistryPublisher | null {
  // Official-tier plugins are signed by the Nosdesk root key and
  // don't have a publisher row. Render a synthetic entry for the UI.
  if (plugin.tier === 'official') return null;
  return publishersByKey.value.get(plugin.publisher_pubkey) ?? null;
}

/** First 16 hex chars of SHA-256(pubkey) — matches the backend
 * `signing::fingerprint` output. Computed client-side so the
 * registry snapshot doesn't have to carry a redundant field. */
async function pubkeyFingerprint(pubkeyB64: string): Promise<string> {
  const raw = Uint8Array.from(atob(pubkeyB64), (c) => c.charCodeAt(0));
  const digest = await crypto.subtle.digest('SHA-256', raw);
  return Array.from(new Uint8Array(digest).slice(0, 8))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

const fingerprintCache = new Map<string, string>();
async function getFingerprint(pubkeyB64: string): Promise<string> {
  let fp = fingerprintCache.get(pubkeyB64);
  if (!fp) {
    fp = await pubkeyFingerprint(pubkeyB64);
    fingerprintCache.set(pubkeyB64, fp);
  }
  return fp;
}

const pendingFingerprint = ref<string>('');

async function startInstall(plugin: RegistryPlugin) {
  pendingInstall.value = plugin;
  communityConfirmText.value = '';
  pendingFingerprint.value = await getFingerprint(plugin.publisher_pubkey);

  // Official: one click, no modal.
  if (plugin.tier === 'official') {
    await confirmInstall();
  }
  // verified / community: modal stays open for confirmation.
}

async function confirmInstall() {
  if (!pendingInstall.value) return;
  const plugin = pendingInstall.value;

  // Community tier requires typing the plugin name exactly.
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
    successMessage.value = `Installed ${installed.display_name} v${installed.version}`;
    setTimeout(() => (successMessage.value = ''), 4000);
    pendingInstall.value = null;
  } catch (err: unknown) {
    const message =
      (err as { response?: { data?: string } })?.response?.data ??
      'Install failed.';
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

function tierBadgeClass(tier: PluginTrustLevel): string {
  switch (tier) {
    case 'official':
      return 'bg-status-success/10 text-status-success';
    case 'verified':
      return 'bg-accent/10 text-accent';
    case 'community':
      return 'bg-status-warning/10 text-status-warning';
    default:
      return 'bg-surface-alt text-secondary';
  }
}

function tierLabel(tier: PluginTrustLevel): string {
  return tier.charAt(0).toUpperCase() + tier.slice(1);
}
</script>

<template>
  <div class="p-6 space-y-4">
    <header class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold">Plugin Registry</h1>
        <p class="text-sm text-secondary">
          Browse and install plugins published to
          <code>nosdesk.com/registry</code>. Signatures are verified
          against the Nosdesk root key before any bundle executes.
        </p>
      </div>
      <button
        type="button"
        class="btn btn-secondary text-sm"
        @click="loadRegistry"
        :disabled="isLoading"
      >
        Refresh
      </button>
    </header>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
    <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

    <div v-if="isLoading" class="flex justify-center py-12">
      <LoadingSpinner />
    </div>

    <template v-else-if="snapshot">
      <div class="flex flex-wrap gap-2 items-center">
        <input
          v-model="searchQuery"
          type="search"
          placeholder="Search plugins..."
          class="input input-sm flex-1 min-w-[200px]"
        />
        <select v-model="tierFilter" class="input input-sm w-40">
          <option value="all">All tiers</option>
          <option value="official">Official</option>
          <option value="verified">Verified</option>
          <option value="community">Community</option>
        </select>
        <span class="text-xs text-secondary ml-auto">
          Fetched {{ new Date(snapshot.fetched_at).toLocaleString() }}
        </span>
      </div>

      <EmptyState
        v-if="filteredPlugins.length === 0"
        title="No plugins match"
        :description="searchQuery ? 'Try clearing search or filter.' : 'Registry is empty.'"
      />

      <ul v-else class="space-y-2">
        <li
          v-for="plugin in filteredPlugins"
          :key="plugin.name"
          class="border border-default rounded-lg p-4 bg-surface"
        >
          <div class="flex items-start justify-between gap-4">
            <div class="min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <h3 class="font-semibold">{{ plugin.display_name }}</h3>
                <span class="text-xs text-secondary">{{ plugin.name }}</span>
                <span
                  class="text-xs px-2 py-0.5 rounded font-medium"
                  :class="tierBadgeClass(plugin.tier)"
                >
                  {{ tierLabel(plugin.tier) }}
                </span>
              </div>
              <p v-if="plugin.description" class="text-sm text-secondary mt-1">
                {{ plugin.description }}
              </p>
              <div class="text-xs text-secondary mt-2 flex flex-wrap gap-3">
                <span v-if="publisherFor(plugin)">
                  by {{ publisherFor(plugin)?.display_name }}
                </span>
                <span v-else-if="plugin.tier === 'official'">by Nosdesk</span>
                <span v-if="plugin.versions[0]">
                  latest: v{{ plugin.versions[0].version }}
                </span>
              </div>
            </div>
            <button
              type="button"
              class="btn btn-primary text-sm shrink-0"
              :disabled="installing === plugin.name"
              @click="startInstall(plugin)"
            >
              {{ installing === plugin.name ? 'Installing...' : 'Install' }}
            </button>
          </div>
        </li>
      </ul>
    </template>

    <Modal
      v-if="pendingInstall && pendingInstall.tier !== 'official'"
      :is-open="true"
      :title="`Install ${pendingInstall.display_name}?`"
      @close="cancelInstall"
    >
      <div class="space-y-3 text-sm">
        <div
          v-if="pendingInstall.tier === 'community'"
          class="border border-status-warning/30 bg-status-warning/10 rounded p-3 text-status-warning"
        >
          <strong>Community plugin.</strong> Nosdesk does not vouch for
          the safety of community plugins beyond verifying the
          publisher's signature. Review the source before trusting it
          with your data.
        </div>
        <dl class="space-y-2">
          <div>
            <dt class="text-xs text-secondary">Publisher</dt>
            <dd>{{ publisherFor(pendingInstall)?.display_name ?? 'Nosdesk' }}</dd>
          </div>
          <div>
            <dt class="text-xs text-secondary">Fingerprint</dt>
            <dd class="font-mono text-xs">{{ pendingFingerprint }}</dd>
          </div>
          <div>
            <dt class="text-xs text-secondary">Version</dt>
            <dd>v{{ pendingInstall.versions[0]?.version }}</dd>
          </div>
        </dl>
        <label v-if="pendingInstall.tier === 'community'" class="block">
          <span class="text-xs text-secondary">
            Type <code>{{ pendingInstall.name }}</code> to confirm
          </span>
          <input
            v-model="communityConfirmText"
            type="text"
            class="input input-sm w-full mt-1"
            :placeholder="pendingInstall.name"
          />
        </label>
      </div>
      <template #footer>
        <button type="button" class="btn btn-secondary text-sm" @click="cancelInstall">
          Cancel
        </button>
        <button
          type="button"
          class="btn btn-primary text-sm"
          :disabled="
            installing === pendingInstall.name ||
            (pendingInstall.tier === 'community' &&
              communityConfirmText.trim() !== pendingInstall.name)
          "
          @click="confirmInstall"
        >
          {{ installing === pendingInstall.name ? 'Installing...' : 'Install' }}
        </button>
      </template>
    </Modal>
  </div>
</template>
