<script setup lang="ts">
import { computed } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import Icon from '@/components/common/Icon.vue';
import {
  assetLifecycleKeys,
  assetLifecycleService,
} from '@nosdesk/core/services/assetLifecycleService';
import { useSyncActions } from '@/composables/useSyncActions';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { lifecycleTransitionLabel } from '@/utils/assetStatusMeta';
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils';
import { numberForTicketId } from '@/composables/useTicketNumberLookup';
import { ticketPath } from '@/utils/ticketNumbers';
import type { AssetLifecycleEvent } from '@nosdesk/core/types/asset';

/**
 * An asset's status history: the disposal record once disposed, then every
 * transition, newest first. The current status and the actions that change
 * it live in the page's state header.
 */
const props = defineProps<{
  assetId: number;
  currentStatus: string;
  /** Staff see who made each change. */
  canEdit?: boolean;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const queryCache = useQueryCache();
const lifecycleQuery = useQuery({
  key: () => assetLifecycleKeys.forAsset(props.assetId),
  query: () => assetLifecycleService.list(props.assetId),
});
// The disposal record (method, vendor, notes), fetched once the asset is
// disposed so its details are visible in the asset view. Anyone but staff
// gets the method and data-bearing flag, without vendor, notes or
// certificate.
const disposalQuery = useQuery({
  key: () => assetLifecycleKeys.disposal(props.assetId),
  query: () => assetLifecycleService.getDisposal(props.assetId),
  enabled: () => props.currentStatus === 'disposed',
});
const disposal = computed(() => disposalQuery.data.value ?? null);
const events = computed<AssetLifecycleEvent[]>(() =>
  Array.isArray(lifecycleQuery.data.value) ? lifecycleQuery.data.value : [],
);
const isFirstLoad = computed(
  () => lifecycleQuery.status.value === 'pending' && lifecycleQuery.data.value === undefined,
);

function invalidate() {
  return queryCache.invalidateQueries({ key: assetLifecycleKeys.forAsset(props.assetId) });
}

useSyncActions(
  (actions) => {
    if (
      actions.some((a) => {
        const data = a.data as { asset_id?: number };
        return data.asset_id === props.assetId;
      })
    ) {
      void invalidate();
    }
  },
  { aggregates: ['asset_lifecycle_event'], debounceMs: 250 },
);

function actorLabel(uuid: string | null | undefined): string {
  if (!uuid) return t('asset-lifecycle-timeline-unknown-actor');
  const handle = getUserHandle(uuid);
  return handle.user.value?.name ?? t('asset-lifecycle-timeline-unknown-actor');
}

function transitionSummary(event: AssetLifecycleEvent): string {
  return lifecycleTransitionLabel(t, event.from_status, event.to_status);
}

function metadataLines(event: AssetLifecycleEvent): string[] {
  const meta = event.metadata ?? {};
  const lines: string[] = [];
  if (typeof meta.vendor === 'string' && meta.vendor) {
    lines.push(t('asset-lifecycle-timeline-vendor', { vendor: meta.vendor }));
  }
  if (typeof meta.rma_number === 'string' && meta.rma_number) {
    lines.push(t('asset-lifecycle-timeline-rma', { rma: meta.rma_number }));
  }
  if (meta.offsite === true) {
    lines.push(t('asset-lifecycle-timeline-offsite'));
  }
  if (typeof meta.expected_return === 'string' && meta.expected_return) {
    lines.push(t('asset-lifecycle-timeline-expected-return', { date: meta.expected_return }));
  }
  if (typeof meta.loaned_to === 'string' && meta.loaned_to) {
    lines.push(t('asset-lifecycle-timeline-loaned-to', { name: meta.loaned_to }));
  }
  if (typeof meta.due_back === 'string' && meta.due_back) {
    lines.push(t('asset-lifecycle-timeline-due-back', { date: meta.due_back }));
  }
  return lines;
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <!-- Disposal record: the compliance detail, surfaced on the asset once it
         is disposed. Compact + wrapping so it stays readable on a narrow rail;
         the who/when lives in the timeline below rather than being repeated. -->
    <div
      v-if="disposal"
      class="rounded-lg border border-default bg-surface-alt px-3 py-2.5 flex flex-col gap-2"
    >
      <div class="flex items-center gap-1.5 text-tertiary">
        <Icon name="trash" size="sm" class="flex-shrink-0" />
        <h3 class="text-xs font-medium uppercase tracking-wide">
          {{ $t('asset-disposal-record-heading') }}
        </h3>
      </div>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-sm">
        <span class="inline-flex items-center gap-1.5">
          <span class="text-tertiary">{{ $t('asset-disposal-method') }}</span>
          <span class="px-1.5 py-0.5 rounded bg-surface text-primary text-xs font-medium">
            {{ $t(`asset-disposal-method-${disposal.sanitization_method}`) }}
          </span>
        </span>
        <span class="inline-flex items-center gap-1.5">
          <span class="text-tertiary">{{ $t('asset-disposal-data-bearing') }}</span>
          <span
            class="px-1.5 py-0.5 rounded text-xs font-medium"
            :class="
              disposal.data_bearing ? 'bg-accent-muted text-accent' : 'bg-surface text-secondary'
            "
          >
            {{ disposal.data_bearing ? $t('asset-disposal-data-bearing-yes') : $t('asset-disposal-data-bearing-no') }}
          </span>
        </span>
        <span v-if="disposal.itad_vendor" class="inline-flex items-center gap-1.5 min-w-0">
          <span class="text-tertiary flex-shrink-0">{{ $t('asset-disposal-itad-vendor') }}</span>
          <span class="text-primary truncate">{{ disposal.itad_vendor }}</span>
        </span>
      </div>
      <p v-if="disposal.notes" class="text-sm text-secondary whitespace-pre-wrap">
        {{ disposal.notes }}
      </p>
    </div>

    <div
      v-if="events.length === 0 && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-4 flex items-start gap-3"
    >
      <Icon name="history" class="text-tertiary flex-shrink-0 mt-0.5" />
      <div class="min-w-0">
        <p class="text-sm font-medium text-primary">{{ $t('asset-lifecycle-empty-title') }}</p>
        <p class="text-xs text-tertiary mt-1">{{ $t('asset-lifecycle-empty-description') }}</p>
      </div>
    </div>

    <div v-else-if="events.length > 0" class="divide-y divide-default">
      <div
        v-for="event in events"
        :key="event.id"
        class="py-2 flex flex-col gap-0.5"
      >
        <div class="flex items-baseline justify-between gap-3 flex-wrap">
          <span class="text-sm font-medium text-primary">{{ transitionSummary(event) }}</span>
          <span class="text-xs text-tertiary whitespace-nowrap">
            <!-- Who made a change is staff only; a requester's events carry
                 no actor, reason, ticket or metadata. -->
            <template v-if="canEdit">{{ actorLabel(event.actor_uuid) }} · </template>{{ formatRelativeTime(event.occurred_at, { addSuffix: true }) }}
          </span>
        </div>
        <div
          v-if="event.ticket_id || event.reason || metadataLines(event).length"
          class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-secondary"
        >
          <RouterLink
            v-if="event.ticket_id"
            :to="ticketPath({ id: event.ticket_id, number: numberForTicketId(event.ticket_id) })"
            class="inline-flex items-center px-1.5 py-0.5 rounded border border-default bg-surface-alt text-accent hover:underline"
          >
            {{ $t('asset-lifecycle-timeline-ticket', { id: numberForTicketId(event.ticket_id) ?? '' }) }}
          </RouterLink>
          <span v-if="event.reason">{{ event.reason }}</span>
          <span v-for="(line, idx) in metadataLines(event)" :key="idx">{{ line }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
