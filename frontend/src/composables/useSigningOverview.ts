/**
 * Plugin signing overview: the instance's trust posture (tier counts and
 * warnings) from GET /admin/plugins/signing-overview. Extracted from the old
 * PluginSigningOverview card so the list view can drive both a page-level
 * warning banner and a slim tier strip from one fetch. Read-only; remediation
 * lives on the plugin detail view.
 */
import { computed, onMounted, ref } from 'vue';
import pluginService from '@nosdesk/core/services/pluginService';
import type { PluginTrustLevel, SigningOverview } from '@nosdesk/core/types/plugin';

const KNOWN_TIERS: PluginTrustLevel[] = ['official', 'verified', 'community', 'local'];

export interface SigningWarning {
  tone: 'caution' | 'critical';
  message: string;
}

/** Tier + count with the narrowed trust-level type the badge expects. */
export interface DenseTier {
  trust_level: PluginTrustLevel;
  count: number;
}

function plural(n: number): string {
  return n === 1 ? '' : 's';
}

export function useSigningOverview() {
  const overview = ref<SigningOverview | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Every known tier renders, even at zero: a sparse list would make a real
  // zero (no official plugins) look like missing data.
  const denseTiers = computed<DenseTier[]>(() => {
    if (!overview.value) return [];
    const found = new Map(overview.value.by_trust_level.map((t) => [t.trust_level, t.count]));
    return KNOWN_TIERS.map((tier) => ({ trust_level: tier, count: found.get(tier) ?? 0 }));
  });

  const warnings = computed<SigningWarning[]>(() => {
    const o = overview.value;
    if (!o) return [];
    const out: SigningWarning[] = [];
    if (o.dev_mode_count > 0) {
      out.push({
        tone: 'caution',
        message: `${o.dev_mode_count} plugin${plural(o.dev_mode_count)} installed via NOSDESK_DEV_MODE. Reinstall through the signed path before deploying to production.`,
      });
    }
    if (o.legacy_unsigned_count > 0) {
      out.push({
        tone: 'caution',
        message: `${o.legacy_unsigned_count} plugin${plural(o.legacy_unsigned_count)} predate signing and have no signer metadata. Reinstall to re-anchor.`,
      });
    }
    if (o.revoked_signer_count > 0) {
      out.push({
        tone: 'critical',
        message: `${o.revoked_signer_count} plugin${plural(o.revoked_signer_count)} signed by a publisher whose key is now revoked. Review the affected rows and uninstall if you no longer trust the publisher.`,
      });
    }
    return out;
  });

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

  return { overview, loading, error, denseTiers, warnings, load };
}
