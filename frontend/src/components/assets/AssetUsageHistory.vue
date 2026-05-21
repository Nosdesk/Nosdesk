<script setup lang="ts">
/**
 * Usage history panel for the asset detail view. Renders the
 * asset's `asset_usage_log` rows newest first, with a basic
 * load-more affordance for assets with deep history.
 *
 * The panel is only mounted for stock-tracked assets (caller
 * gates on `asset.quantity != null`); non-tracked assets don't
 * generate usage rows so the panel would always be empty.
 */
import { onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { assetUsageService, type AssetUsage } from '@/services/assetUsageService';
import { RouterLink } from 'vue-router';

const props = defineProps<{
  assetId: number;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const PAGE_SIZE = 25;
const rows = ref<AssetUsage[]>([]);
const loading = ref(false);
const errorMessage = ref('');
const hasMore = ref(false);

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

onMounted(loadInitial);

function formatDate(iso: string): string {
  return new Date(iso).toLocaleString();
}
</script>

<template>
  <div class="flex flex-col gap-3">
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
