<script setup lang="ts">
/**
 * Operator-facing inventory of plugin trust state. Renders a short
 * summary card above the installed list so an admin can see at a
 * glance how the verifier classified everything it currently
 * serves: tier counts, dev-mode rows (red flag in prod), legacy
 * unsigned rows (migration straggler), and the top publishers by
 * install count (blast radius if a key is revoked).
 *
 * Data comes from GET /admin/plugins/signing-overview. The view is
 * read-only; remediation flows live on PluginDetailView.
 */
import { computed, onMounted, ref } from 'vue';
import pluginService from '@/services/pluginService';
import type {
  PluginTrustLevel,
  SigningOverview,
  TrustLevelCount,
} from '@nosdesk/core/types/plugin';
import PluginTrustBadge from './PluginTrustBadge.vue';

const overview = ref<SigningOverview | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);

const KNOWN_TIERS: PluginTrustLevel[] = ['official', 'verified', 'community', 'local'];

// Densify the tier list so every known tier renders, even at zero.
// The backend only returns rows that have plugins; sparse pills
// would make a zero count for "official" look like missing data
// rather than a deliberate fact about the instance.
const denseTiers = computed<TrustLevelCount[]>(() => {
  if (!overview.value) return [];
  const found = new Map(
    overview.value.by_trust_level.map((t) => [t.trust_level, t.count]),
  );
  return KNOWN_TIERS.map((tier) => ({
    trust_level: tier,
    count: found.get(tier) ?? 0,
  }));
});

const hasWarnings = computed(
  () =>
    !!overview.value &&
    (overview.value.dev_mode_count > 0 ||
      overview.value.legacy_unsigned_count > 0 ||
      overview.value.revoked_signer_count > 0),
);

function fingerprint(pubkey: string): string {
  return pubkey.length > 12 ? `${pubkey.slice(0, 12)}…` : pubkey;
}

async function load() {
  loading.value = true;
  error.value = null;
  try {
    overview.value = await pluginService.getSigningOverview();
  } catch {
    error.value = 'Failed to load signing overview';
  } finally {
    loading.value = false;
  }
}

onMounted(load);

defineExpose({ refresh: load });
</script>

<template>
  <section
    class="rounded-xl border border-default bg-surface p-4"
    aria-labelledby="plugin-signing-overview-heading"
  >
    <header class="mb-3 flex items-center justify-between gap-2">
      <h2
        id="plugin-signing-overview-heading"
        class="text-sm font-semibold text-primary"
      >
        Signing inventory
      </h2>
      <span v-if="overview" class="text-xs tabular-nums text-tertiary">
        {{ overview.total }} installed
      </span>
    </header>

    <p v-if="loading && !overview" class="text-xs text-tertiary">Loading...</p>
    <p v-else-if="error" class="text-xs text-status-error">{{ error }}</p>

    <template v-else-if="overview">
      <!-- Tier distribution: one pill per known tier, with count. -->
      <ul class="flex flex-wrap gap-2" role="list">
        <li
          v-for="tier in denseTiers"
          :key="tier.trust_level"
          class="flex items-center gap-1.5 rounded-lg border border-default bg-surface-alt px-2 py-1"
        >
          <PluginTrustBadge :level="(tier.trust_level as PluginTrustLevel)" />
          <span class="text-xs tabular-nums text-secondary">{{ tier.count }}</span>
        </li>
      </ul>

      <!-- Warnings: dev-mode + legacy unsigned. Both should be
           zero on a clean production instance. -->
      <div v-if="hasWarnings" class="mt-3 flex flex-col gap-1.5">
        <p
          v-if="overview.dev_mode_count > 0"
          class="rounded-md bg-status-warning/10 px-2 py-1 text-xs text-status-warning"
        >
          {{ overview.dev_mode_count }} plugin{{ overview.dev_mode_count === 1 ? '' : 's' }}
          installed via NOSDESK_DEV_MODE. Reinstall through the signed path before deploying to production.
        </p>
        <p
          v-if="overview.legacy_unsigned_count > 0"
          class="rounded-md bg-status-warning/10 px-2 py-1 text-xs text-status-warning"
        >
          {{ overview.legacy_unsigned_count }} plugin{{ overview.legacy_unsigned_count === 1 ? '' : 's' }}
          predate signing rollout and have no signer metadata. Reinstall to re-anchor.
        </p>
        <p
          v-if="overview.revoked_signer_count > 0"
          class="rounded-md bg-status-error/10 px-2 py-1 text-xs text-status-error"
        >
          {{ overview.revoked_signer_count }} plugin{{ overview.revoked_signer_count === 1 ? '' : 's' }}
          signed by a publisher whose key is now revoked. Review the affected rows below and uninstall if you no longer trust the publisher.
        </p>
      </div>

      <!-- Top publishers by install count. Shows blast radius if
           the registry revokes a key. Hidden when no row qualifies
           (all installs are official + local, no third-party
           publishers in play). -->
      <div v-if="overview.top_publishers.length > 0" class="mt-3">
        <h3 class="text-xs font-semibold tracking-wide text-tertiary uppercase">
          Top signers
        </h3>
        <ul class="mt-1.5 flex flex-col gap-1" role="list">
          <li
            v-for="pub in overview.top_publishers"
            :key="pub.pubkey"
            class="flex items-center justify-between gap-2 text-xs"
          >
            <span class="min-w-0 flex-1 truncate text-secondary">
              <template v-if="pub.display_name">{{ pub.display_name }}</template>
              <template v-else>Unattributed</template>
              <code class="ml-1.5 rounded bg-surface-alt px-1 font-mono text-tertiary">
                {{ fingerprint(pub.pubkey) }}
              </code>
            </span>
            <span class="tabular-nums text-tertiary">{{ pub.count }}</span>
          </li>
        </ul>
      </div>
    </template>
  </section>
</template>
