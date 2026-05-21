<script setup lang="ts">
/**
 * Asset usage panel on the ticket detail view.
 *
 * Two columns of behaviour:
 *
 * - **Recorded usage**: a read-only list of `asset_usage_log`
 *   rows tied to this ticket, newest first. Sourced from
 *   GET /tickets/{id}/asset-usage. Loaded once on mount and
 *   after every successful record.
 *
 * - **Record new usage**: for each linked asset whose
 *   `quantity` is set (stock-tracked), surface an inline
 *   "Used [N] [unit]" form. Non-stock-tracked assets are
 *   simply not eligible — they don't appear in the entry form.
 *
 * The component owns the network round-trip; the parent only
 * needs to supply the linked assets and ticket id, and listen
 * for `asset-updated` to refresh its own copy of the asset
 * (because `assets.quantity` decremented in the same
 * transaction).
 */
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { assetUsageService, type AssetUsage } from '@/services/assetUsageService';
import type { Asset } from '@/types/asset';
import Icon from '@/components/common/Icon.vue';

const props = defineProps<{
  ticketId: number;
  assets: Asset[];
}>();

const emit = defineEmits<{
  (e: 'asset-updated', assetId: number): void;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const history = ref<AssetUsage[]>([]);
const loading = ref(false);
const errorMessage = ref('');

/** Map assetId -> draft input state for the inline entry forms.
 *  Keyed on asset id so multiple rows can be edited at once. */
const drafts = ref<Record<number, { quantity: string; notes: string; submitting: boolean }>>({});

const stockTrackedAssets = computed(() =>
  props.assets.filter((a) => a.quantity != null && a.unit != null && a.unit !== ''),
);

/** Index assets by id for the history-row enrichment so each
 *  entry can render `name (unit)` without a second fetch. */
const assetsById = computed(() => {
  const out = new Map<number, Asset>();
  for (const a of props.assets) out.set(a.id, a);
  return out;
});

async function reload() {
  loading.value = true;
  errorMessage.value = '';
  try {
    history.value = await assetUsageService.listForTicket(props.ticketId);
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('ticket-asset-usage-load-failed');
  } finally {
    loading.value = false;
  }
}

function draftFor(assetId: number) {
  if (!drafts.value[assetId]) {
    drafts.value[assetId] = { quantity: '', notes: '', submitting: false };
  }
  return drafts.value[assetId];
}

async function submit(asset: Asset) {
  const draft = draftFor(asset.id);
  const trimmed = draft.quantity.trim();
  if (!trimmed) return;

  draft.submitting = true;
  try {
    await assetUsageService.record(asset.id, {
      quantity_used: trimmed,
      ticket_id: props.ticketId,
      notes: draft.notes.trim() || null,
    });
    drafts.value[asset.id] = { quantity: '', notes: '', submitting: false };
    await reload();
    // Backend decremented assets.quantity; let the parent
    // refresh its copy of the asset row.
    emit('asset-updated', asset.id);
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('ticket-asset-usage-record-failed');
    draft.submitting = false;
  }
}

onMounted(reload);
</script>

<template>
  <div class="flex flex-col gap-3">
    <!-- Header row mirrors the other sidebar field labels -->
    <div class="flex items-center gap-2">
      <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">
        {{ $t('ticket-asset-usage-heading') }}
      </h3>
      <span v-if="history.length > 0" class="text-xs text-tertiary">
        {{ history.length }}
      </span>
    </div>

    <p v-if="errorMessage" class="text-xs text-status-error">{{ errorMessage }}</p>

    <!-- Recorded history -->
    <div v-if="history.length > 0" class="flex flex-col gap-1.5">
      <div
        v-for="row in history"
        :key="row.id"
        class="flex items-baseline justify-between gap-2 text-sm"
      >
        <div class="flex flex-col gap-0.5 min-w-0">
          <span class="truncate font-medium" :class="row.event_kind === 'restock' ? 'text-status-success' : 'text-status-error'">
            {{ row.event_kind === 'restock' ? '+' : '−' }}{{ row.quantity_used }} {{ row.unit }}
            <span class="text-tertiary font-normal">
              · {{ assetsById.get(row.asset_id)?.name ?? `#${row.asset_id}` }}
            </span>
          </span>
          <span v-if="row.notes" class="text-xs text-tertiary truncate">{{ row.notes }}</span>
        </div>
        <span class="text-xs text-tertiary whitespace-nowrap">
          {{ new Date(row.recorded_at).toLocaleDateString() }}
        </span>
      </div>
    </div>
    <p v-else-if="!loading && stockTrackedAssets.length === 0" class="text-xs text-tertiary italic">
      {{ $t('ticket-asset-usage-empty-no-stock') }}
    </p>
    <p v-else-if="!loading" class="text-xs text-tertiary italic">
      {{ $t('ticket-asset-usage-empty-no-history') }}
    </p>

    <!-- Inline entry: one row per stock-tracked linked asset -->
    <div v-if="stockTrackedAssets.length > 0" class="flex flex-col gap-3 pt-2 border-t border-default">
      <div v-for="asset in stockTrackedAssets" :key="asset.id" class="flex flex-col gap-1.5">
        <label class="text-xs font-medium text-secondary">
          {{ asset.name }}
          <span class="text-tertiary">({{ asset.quantity }} {{ asset.unit }} on hand)</span>
        </label>
        <div class="flex items-center gap-2">
          <input
            v-model="draftFor(asset.id).quantity"
            type="text"
            inputmode="decimal"
            :placeholder="$t('ticket-asset-usage-quantity-placeholder', { unit: asset.unit ?? '' })"
            class="flex-1 bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-1.5 text-primary placeholder-secondary text-sm focus:outline-none focus:ring-2 focus:ring-accent/50"
          />
          <button
            :disabled="!draftFor(asset.id).quantity.trim() || draftFor(asset.id).submitting"
            class="px-3 py-1.5 text-sm rounded-lg bg-accent text-on-accent hover:bg-accent-strong disabled:opacity-50 disabled:cursor-not-allowed"
            @click="submit(asset)"
          >
            <Icon name="add" />
          </button>
        </div>
        <input
          v-if="draftFor(asset.id).quantity"
          v-model="draftFor(asset.id).notes"
          type="text"
          :placeholder="$t('ticket-asset-usage-notes-placeholder')"
          class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-1.5 text-primary placeholder-secondary text-xs focus:outline-none focus:ring-2 focus:ring-accent/50"
        />
      </div>
    </div>
  </div>
</template>
