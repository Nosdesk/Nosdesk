<script setup lang="ts">
/**
 * Usage history panel for the asset detail view.
 *
 * Two roles in one component because they share the same
 * data source and reload trigger:
 *
 * - **Record ad-hoc consumption** (top of panel): an inline
 *   form for restock audits, write-offs, workshop usage,
 *   anything that isn't tied to a ticket. POSTs with
 *   `ticket_id: null`. Emits `recorded` so the parent can
 *   refresh its copy of the asset (the backend decremented
 *   `assets.quantity` in the same transaction).
 *
 * - **History list** (below): paginated ledger newest first,
 *   25 rows per page with a load-more button.
 *
 * The panel only mounts for stock-tracked assets (caller
 * gates on `asset.quantity != null`); non-tracked assets
 * don't generate usage rows so the panel would always be
 * empty.
 */
import { onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { assetUsageService, type AssetUsage } from '@/services/assetUsageService';
import { RouterLink } from 'vue-router';

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
const rows = ref<AssetUsage[]>([]);
const loading = ref(false);
const errorMessage = ref('');
const hasMore = ref(false);

const recordQuantity = ref('');
const recordNotes = ref('');
const recording = ref(false);
const recordError = ref('');

async function loadInitial() {
  loading.value = true;
  errorMessage.value = '';
  try {
    const page = await assetUsageService.listForAsset(props.assetId, {
      limit: PAGE_SIZE,
      offset: 0,
    });
    rows.value = page;
    hasMore.value = page.length === PAGE_SIZE;
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-usage-history-load-failed');
  } finally {
    loading.value = false;
  }
}

async function loadMore() {
  if (loading.value || !hasMore.value) return;
  loading.value = true;
  try {
    const next = await assetUsageService.listForAsset(props.assetId, {
      limit: PAGE_SIZE,
      offset: rows.value.length,
    });
    rows.value = [...rows.value, ...next];
    hasMore.value = next.length === PAGE_SIZE;
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-usage-history-load-failed');
  } finally {
    loading.value = false;
  }
}

async function submitRecord() {
  const trimmed = recordQuantity.value.trim();
  if (!trimmed) return;

  recording.value = true;
  recordError.value = '';
  try {
    await assetUsageService.record(props.assetId, {
      quantity_used: trimmed,
      ticket_id: null,
      notes: recordNotes.value.trim() || null,
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

onMounted(loadInitial);

function formatDate(iso: string): string {
  return new Date(iso).toLocaleString();
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- Ad-hoc record form -->
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
          @keyup.enter="submitRecord"
        />
        <button
          :disabled="!recordQuantity.trim() || recording"
          class="px-3 py-1.5 text-sm rounded-lg bg-accent text-on-accent hover:bg-accent-strong disabled:opacity-50 disabled:cursor-not-allowed"
          @click="submitRecord"
        >
          {{ $t('asset-usage-record-submit') }}
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

    <!-- History list -->
    <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>

    <p v-if="!loading && rows.length === 0" class="text-sm text-tertiary italic">
      {{ $t('asset-usage-history-empty') }}
    </p>

    <div v-if="rows.length > 0" class="divide-y divide-default">
      <div v-for="row in rows" :key="row.id" class="py-2.5 flex flex-col gap-1">
        <div class="flex items-baseline justify-between gap-3">
          <span class="text-sm text-primary font-medium">
            {{ row.quantity_used }} {{ row.unit }}
          </span>
          <span class="text-xs text-tertiary whitespace-nowrap">
            {{ formatDate(row.recorded_at) }}
          </span>
        </div>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs">
          <RouterLink
            v-if="row.ticket_id"
            :to="`/tickets/${row.ticket_id}`"
            class="text-accent hover:underline"
          >
            {{ $t('asset-usage-history-ticket-link', { id: row.ticket_id }) }}
          </RouterLink>
          <span v-else class="text-tertiary italic">
            {{ $t('asset-usage-history-ad-hoc') }}
          </span>
          <span v-if="row.notes" class="text-secondary">{{ row.notes }}</span>
        </div>
      </div>
    </div>

    <button
      v-if="hasMore"
      :disabled="loading"
      class="self-start text-sm text-accent hover:underline disabled:opacity-50"
      @click="loadMore"
    >
      {{ loading ? $t('asset-usage-history-loading') : $t('asset-usage-history-load-more') }}
    </button>
  </div>
</template>
