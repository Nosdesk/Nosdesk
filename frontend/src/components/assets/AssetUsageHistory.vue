<script setup lang="ts">
/**
 * Stock activity panel for the asset detail view.
 *
 * Three roles in one component because they share the same
 * data source and reload trigger:
 *
 * - **Record consumption / restock** (top): inline form that
 *   takes a delta amount. POSTs to /assets/{id}/usage with the
 *   right `kind`.
 *
 * - **Audit count** (middle): separate form that takes the
 *   physical count as a new total. POSTs to /assets/{id}/audit;
 *   the backend sets assets.quantity to the counted value and
 *   logs the delta. Distinct UX because the semantic is
 *   different (state assertion, not transaction).
 *
 * - **Timeline** (bottom): merged ledger of usage events +
 *   audits, newest first. Each entry renders distinctly so the
 *   admin can tell consumption from corrections at a glance.
 *
 * The panel only mounts for stock-tracked assets (caller gates
 * on `asset.quantity != null`); non-tracked assets don't
 * generate ledger rows so the panel would always be empty.
 */
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { assetUsageService, type AssetUsage } from '@/services/assetUsageService';
import { assetAuditService, type AssetAudit } from '@/services/assetAuditService';
import { useSyncActions } from '@/composables/useSyncActions';
import { RouterLink } from 'vue-router';
import { formatDateTime } from '@nosdesk/core/utils/dateUtils';

const props = defineProps<{
  assetId: number;
  unit?: string | null;
  currentQuantity?: string | null;
}>();

const emit = defineEmits<{
  (e: 'recorded'): void;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const PAGE_SIZE = 25;
const usageRows = ref<AssetUsage[]>([]);
const auditRows = ref<AssetAudit[]>([]);
const loading = ref(false);
const errorMessage = ref('');
const hasMoreUsage = ref(false);

const recordQuantity = ref('');
const recordNotes = ref('');
const recording = ref(false);
const recordError = ref('');

const auditCount = ref('');
const auditNotes = ref('');
const auditing = ref(false);
const auditError = ref('');

/** Merged timeline. Usage and audit rows are tagged so the
 *  template can branch on `kind`. Sorted by recorded_at desc;
 *  ties broken by id (newer first). */
type TimelineEntry =
  | { kind: 'usage'; row: AssetUsage }
  | { kind: 'audit'; row: AssetAudit };

const timeline = computed<TimelineEntry[]>(() => {
  const merged: TimelineEntry[] = [
    ...usageRows.value.map((r): TimelineEntry => ({ kind: 'usage', row: r })),
    ...auditRows.value.map((r): TimelineEntry => ({ kind: 'audit', row: r })),
  ];
  merged.sort((a, b) => {
    const ta = new Date(a.row.recorded_at).getTime();
    const tb = new Date(b.row.recorded_at).getTime();
    if (ta !== tb) return tb - ta;
    return b.row.id - a.row.id;
  });
  return merged;
});

async function loadInitial() {
  loading.value = true;
  errorMessage.value = '';
  try {
    const [usage, audits] = await Promise.all([
      assetUsageService.listForAsset(props.assetId, { limit: PAGE_SIZE, offset: 0 }),
      assetAuditService.listForAsset(props.assetId, { limit: PAGE_SIZE, offset: 0 }),
    ]);
    usageRows.value = usage;
    auditRows.value = audits;
    hasMoreUsage.value = usage.length === PAGE_SIZE;
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-usage-history-load-failed');
  } finally {
    loading.value = false;
  }
}

async function loadMore() {
  if (loading.value || !hasMoreUsage.value) return;
  loading.value = true;
  try {
    const next = await assetUsageService.listForAsset(props.assetId, {
      limit: PAGE_SIZE,
      offset: usageRows.value.length,
    });
    usageRows.value = [...usageRows.value, ...next];
    hasMoreUsage.value = next.length === PAGE_SIZE;
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-usage-history-load-failed');
  } finally {
    loading.value = false;
  }
}

async function submitRecord(kind: 'usage' | 'restock') {
  const trimmed = recordQuantity.value.trim();
  if (!trimmed) return;
  recording.value = true;
  recordError.value = '';
  try {
    await assetUsageService.record(props.assetId, {
      quantity_used: trimmed,
      ticket_id: null,
      notes: recordNotes.value.trim() || null,
      kind,
    });
    recordQuantity.value = '';
    recordNotes.value = '';
    await loadInitial();
    emit('recorded');
  } catch (e) {
    recordError.value = e instanceof Error ? e.message : t('asset-usage-record-failed');
  } finally {
    recording.value = false;
  }
}

async function submitAudit() {
  const trimmed = auditCount.value.trim();
  if (!trimmed) return;
  auditing.value = true;
  auditError.value = '';
  try {
    await assetAuditService.record(props.assetId, {
      counted_quantity: trimmed,
      notes: auditNotes.value.trim() || null,
    });
    auditCount.value = '';
    auditNotes.value = '';
    await loadInitial();
    emit('recorded');
  } catch (e) {
    auditError.value = e instanceof Error ? e.message : t('asset-audit-record-failed');
  } finally {
    auditing.value = false;
  }
}

// ---- SSE live updates ----------------------------------------------

interface AssetUsageRecordedEvent {
  id: number;
  asset_id: number;
  asset_name: string;
  ticket_id: number | null;
  quantity_used: string;
  unit: string;
  event_kind: 'usage' | 'restock';
  notes: string | null;
  recorded_at: string;
}

interface AssetAuditRecordedEvent {
  id: number;
  asset_id: number;
  asset_name: string;
  counted_quantity: string;
  previous_quantity: string;
  delta: string;
  unit: string;
  notes: string | null;
  recorded_at: string;
}

// Live ledger updates via the sync stream (cross-machine). The
// asset_usage / asset_audit aggregates aren't pool-materialised; we
// read the row off each recorded event and prepend it, deduping by id.
useSyncActions(
  (actions) => {
    for (const a of actions) {
      const data = a.data as unknown as AssetUsageRecordedEvent;
      if (data.asset_id !== props.assetId) continue;
      if (usageRows.value.some((r) => r.id === data.id)) continue;
      usageRows.value = [
        {
          id: data.id,
          asset_id: data.asset_id,
          ticket_id: data.ticket_id,
          quantity_used: data.quantity_used,
          unit: data.unit,
          recorded_by: null,
          recorded_at: data.recorded_at,
          notes: data.notes,
          event_kind: data.event_kind,
        },
        ...usageRows.value,
      ];
    }
  },
  { aggregates: ['asset_usage'] },
);

useSyncActions(
  (actions) => {
    for (const a of actions) {
      const data = a.data as unknown as AssetAuditRecordedEvent;
      if (data.asset_id !== props.assetId) continue;
      if (auditRows.value.some((r) => r.id === data.id)) continue;
      auditRows.value = [
        {
          id: data.id,
          asset_id: data.asset_id,
          counted_quantity: data.counted_quantity,
          previous_quantity: data.previous_quantity,
          delta: data.delta,
          notes: data.notes,
          recorded_by: null,
          recorded_at: data.recorded_at,
        },
        ...auditRows.value,
      ];
    }
  },
  { aggregates: ['asset_audit'] },
);

onMounted(loadInitial);

function formatDate(iso: string): string {
  return formatDateTime(iso);
}

/** Render the leading sign for an audit delta. BigDecimal
 *  serialises positives as "12.345" with no leading +, and
 *  negatives as "-12.345"; we want explicit "+" on positives
 *  and a unicode minus for typographic balance with the usage
 *  signs above. Zero renders as "0" with no sign. */
function formatDelta(delta: string): string {
  if (delta.startsWith('-')) return `−${delta.slice(1)}`;
  if (delta === '0' || delta === '0.000') return '0';
  return `+${delta}`;
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- Record / Restock form (delta-based) -->
    <div class="flex flex-col gap-2 pb-3 border-b border-default">
      <label class="text-xs font-medium text-secondary uppercase tracking-wide">
        {{ $t('asset-usage-record-heading') }}
        <span v-if="currentQuantity != null && unit" class="text-tertiary normal-case">
          ({{ currentQuantity }} {{ unit }} {{ $t('asset-usage-record-on-hand') }})
        </span>
      </label>
      <div class="flex items-center gap-2">
        <input
          v-model="recordQuantity"
          type="text"
          inputmode="decimal"
          :placeholder="$t('asset-usage-record-quantity-placeholder', { unit: unit ?? '' })"
          class="flex-1 bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-1.5 text-primary placeholder-secondary text-sm focus:outline-none focus:ring-2 focus:ring-accent/50"
          @keyup.enter="submitRecord('usage')"
        />
        <button
          :disabled="!recordQuantity.trim() || recording"
          class="px-3 py-1.5 text-sm rounded-lg bg-accent text-on-accent hover:bg-accent-strong disabled:opacity-50 disabled:cursor-not-allowed"
          :title="$t('asset-usage-record-submit-usage-title')"
          @click="submitRecord('usage')"
        >
          {{ $t('asset-usage-record-submit') }}
        </button>
        <button
          :disabled="!recordQuantity.trim() || recording"
          class="px-3 py-1.5 text-sm rounded-lg border border-status-success/40 text-status-success hover:bg-status-success/10 disabled:opacity-50 disabled:cursor-not-allowed"
          :title="$t('asset-usage-record-submit-restock-title')"
          @click="submitRecord('restock')"
        >
          {{ $t('asset-usage-record-submit-restock') }}
        </button>
      </div>
      <input
        v-if="recordQuantity.trim()"
        v-model="recordNotes"
        type="text"
        :placeholder="$t('asset-usage-record-notes-placeholder')"
        class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-1.5 text-primary placeholder-secondary text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
      />
      <p v-if="recordError" class="text-xs text-status-error">{{ recordError }}</p>
    </div>

    <!-- Audit count form (state-assertion) -->
    <div class="flex flex-col gap-2 pb-3 border-b border-default">
      <label class="text-xs font-medium text-secondary uppercase tracking-wide">
        {{ $t('asset-audit-record-heading') }}
        <span class="text-tertiary normal-case font-normal">
          · {{ $t('asset-audit-record-hint') }}
        </span>
      </label>
      <div class="flex items-center gap-2">
        <input
          v-model="auditCount"
          type="text"
          inputmode="decimal"
          :placeholder="$t('asset-audit-record-placeholder', { unit: unit ?? '' })"
          class="flex-1 bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-1.5 text-primary placeholder-secondary text-sm focus:outline-none focus:ring-2 focus:ring-accent/50"
          @keyup.enter="submitAudit"
        />
        <button
          :disabled="!auditCount.trim() || auditing"
          class="px-3 py-1.5 text-sm rounded-lg border border-default text-primary hover:border-strong disabled:opacity-50 disabled:cursor-not-allowed"
          @click="submitAudit"
        >
          {{ $t('asset-audit-record-submit') }}
        </button>
      </div>
      <input
        v-if="auditCount.trim()"
        v-model="auditNotes"
        type="text"
        :placeholder="$t('asset-audit-record-notes-placeholder')"
        class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-1.5 text-primary placeholder-secondary text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
      />
      <p v-if="auditError" class="text-xs text-status-error">{{ auditError }}</p>
    </div>

    <!-- Timeline -->
    <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>

    <p v-if="!loading && timeline.length === 0" class="text-sm text-tertiary italic">
      {{ $t('asset-usage-history-empty') }}
    </p>

    <div v-if="timeline.length > 0" class="divide-y divide-default">
      <div v-for="entry in timeline" :key="`${entry.kind}-${entry.row.id}`" class="py-2.5 flex flex-col gap-1">
        <!-- Usage / restock row -->
        <template v-if="entry.kind === 'usage'">
          <div class="flex items-baseline justify-between gap-3">
            <span
              class="text-sm font-medium"
              :class="entry.row.event_kind === 'restock' ? 'text-status-success' : 'text-status-error'"
            >
              {{ entry.row.event_kind === 'restock' ? '+' : '−' }}{{ entry.row.quantity_used }} {{ entry.row.unit }}
            </span>
            <span class="text-xs text-tertiary whitespace-nowrap">
              {{ formatDate(entry.row.recorded_at) }}
            </span>
          </div>
          <div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs">
            <RouterLink
              v-if="entry.row.ticket_id"
              :to="`/tickets/${entry.row.ticket_id}`"
              class="text-accent hover:underline"
            >
              {{ $t('asset-usage-history-ticket-link', { id: entry.row.ticket_id }) }}
            </RouterLink>
            <span v-else class="text-tertiary italic">
              {{ $t('asset-usage-history-ad-hoc') }}
            </span>
            <span v-if="entry.row.notes" class="text-secondary">{{ entry.row.notes }}</span>
          </div>
        </template>

        <!-- Audit row -->
        <template v-else>
          <div class="flex items-baseline justify-between gap-3">
            <span class="text-sm font-medium text-accent">
              {{ $t('asset-audit-history-label') }}: {{ entry.row.counted_quantity }} {{ unit }}
            </span>
            <span class="text-xs text-tertiary whitespace-nowrap">
              {{ formatDate(entry.row.recorded_at) }}
            </span>
          </div>
          <div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs">
            <span class="text-tertiary">
              {{ $t('asset-audit-history-previous', { previous: entry.row.previous_quantity }) }}
              <span
                :class="entry.row.delta.startsWith('-')
                  ? 'text-status-error'
                  : entry.row.delta === '0' || entry.row.delta === '0.000'
                    ? 'text-tertiary'
                    : 'text-status-success'"
              >
                ({{ formatDelta(entry.row.delta) }})
              </span>
            </span>
            <span v-if="entry.row.notes" class="text-secondary">{{ entry.row.notes }}</span>
          </div>
        </template>
      </div>
    </div>

    <button
      v-if="hasMoreUsage"
      :disabled="loading"
      class="self-start text-sm text-accent hover:underline disabled:opacity-50"
      @click="loadMore"
    >
      {{ loading ? $t('asset-usage-history-loading') : $t('asset-usage-history-load-more') }}
    </button>
  </div>
</template>
