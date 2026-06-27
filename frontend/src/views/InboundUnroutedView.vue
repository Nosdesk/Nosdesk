<script setup lang="ts">
/**
 * Platform-operator view of unrouted inbound mail (the dead-letter log).
 *
 * Cross-tenant diagnostic: mail forwarded to an unknown `<token>` that passed
 * spam/virus scans but matched no active forwarding address. It exists so a
 * customer's misconfigured forward (a typo'd address, a forward set up before
 * the channel was saved) is visible to the operator instead of vanishing. Not
 * a quarantine: there's nothing to action per-message, just a signal that a
 * forward somewhere is pointed at the wrong address.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';

import { inboundDeadLettersService, type DeadLetterRow } from '@nosdesk/core/services/inboundDeadLettersService';
import { formatDateTime } from '@nosdesk/core/utils/dateUtils';
import EmptyState from '@/components/common/EmptyState.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const query = useQuery({
  key: ['admin-inbound-dead-letters'],
  query: () => inboundDeadLettersService.list(),
});
const rows = computed<DeadLetterRow[]>(() => query.data.value?.rows ?? []);
const count7d = computed(() => query.data.value?.count_7d ?? 0);
const loadError = computed(() => {
  const e = query.error.value;
  if (!e) return '';
  return e instanceof Error ? e.message : t('admin-unrouted-error-load');
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-unrouted-title') }}</h1>
        <p class="text-secondary">{{ t('admin-unrouted-description') }}</p>
      </div>

      <AlertMessage v-if="loadError && rows.length === 0" type="error" :message="loadError" />

      <section class="rounded border border-default bg-surface p-3 inline-flex items-baseline gap-2 self-start">
        <span class="text-2xl font-semibold">{{ count7d }}</span>
        <span class="text-xs text-secondary uppercase tracking-wide">{{ t('admin-unrouted-count-label') }}</span>
      </section>

      <EmptyState
        v-if="rows.length === 0"
        icon="inbox"
        :title="t('admin-unrouted-empty-title')"
        :description="t('admin-unrouted-empty-description')"
      />

      <table v-else class="w-full text-sm border-collapse">
        <thead>
          <tr class="text-left text-secondary border-b border-default">
            <th class="py-2 pr-3 font-medium">{{ t('admin-unrouted-col-recipient') }}</th>
            <th class="py-2 pr-3 font-medium">{{ t('admin-unrouted-col-from') }}</th>
            <th class="py-2 pr-3 font-medium">{{ t('admin-unrouted-col-subject') }}</th>
            <th class="py-2 font-medium whitespace-nowrap">{{ t('admin-unrouted-col-received') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.id" class="border-b border-default/60">
            <td class="py-2 pr-3 font-mono text-xs text-primary truncate max-w-xs" :title="row.envelope_recipient">
              {{ row.envelope_recipient }}
            </td>
            <td class="py-2 pr-3 text-secondary truncate max-w-xs" :title="row.from_address ?? ''">
              {{ row.from_address || t('admin-unrouted-no-sender') }}
            </td>
            <td class="py-2 pr-3 text-secondary truncate max-w-sm" :title="row.subject ?? ''">
              {{ row.subject || t('admin-unrouted-no-subject') }}
            </td>
            <td class="py-2 text-secondary whitespace-nowrap">{{ formatDateTime(row.received_at) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
