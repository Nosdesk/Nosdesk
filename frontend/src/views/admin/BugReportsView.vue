<script setup lang="ts">
/**
 * Platform-operator view of user-submitted bug reports (the in-app
 * "Report a problem" modal). Cross-tenant: newest reports across every
 * workspace, so a misbehaving build or a spike is visible to the operator.
 * Reports are also pushed to NOSDESK_OPS_EMAIL on submit; this is the durable,
 * browseable channel. Scaffold: latest 50, no filters/pagination yet.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';

import { bugReportsService, type BugReportRow } from '@nosdesk/core/services/bugReportsService';
import { formatDateTime } from '@nosdesk/core/utils/dateUtils';
import { useAuthStore } from '@/stores/auth';
import EmptyState from '@/components/common/EmptyState.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const authStore = useAuthStore();

// Backend gates with require_platform_admin; skip the call for anyone else and
// show the forbidden notice instead of a 403 error flash.
const query = useQuery({
  key: ['admin-bug-reports'],
  query: () => bugReportsService.list({ limit: 50 }),
  enabled: computed(() => authStore.isPlatformAdmin),
});
const rows = computed<BugReportRow[]>(() => query.data.value ?? []);
const loadError = computed(() => {
  const e = query.error.value;
  if (!e) return '';
  return e instanceof Error ? e.message : t('admin-bug-reports-error-load');
});

function shortId(uuid: string | null): string {
  return uuid ? uuid.slice(0, 8) : t('admin-bug-reports-anonymous');
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-bug-reports-title') }}</h1>
        <p class="text-secondary">{{ t('admin-bug-reports-description') }}</p>
      </div>

      <AlertMessage
        v-if="!authStore.isPlatformAdmin"
        type="error"
        :message="t('admin-bug-reports-forbidden')"
      />

      <template v-else>
        <AlertMessage v-if="loadError && rows.length === 0" type="error" :message="loadError" />

        <EmptyState
          v-if="rows.length === 0"
          icon="document"
          :title="t('admin-bug-reports-empty-title')"
          :description="t('admin-bug-reports-empty-description')"
        />

        <table v-else class="w-full text-sm border-collapse">
          <thead>
            <tr class="text-left text-secondary border-b border-default">
              <th class="py-2 pr-3 font-medium whitespace-nowrap">{{ t('admin-bug-reports-col-received') }}</th>
              <th class="py-2 pr-3 font-medium">{{ t('admin-bug-reports-col-workspace') }}</th>
              <th class="py-2 pr-3 font-medium">{{ t('admin-bug-reports-col-reporter') }}</th>
              <th class="py-2 pr-3 font-medium">{{ t('admin-bug-reports-col-description') }}</th>
              <th class="py-2 pr-3 font-medium">{{ t('admin-bug-reports-col-url') }}</th>
              <th class="py-2 font-medium whitespace-nowrap">{{ t('admin-bug-reports-col-build') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="row.id" class="border-b border-default/60 align-top">
              <td class="py-2 pr-3 text-secondary whitespace-nowrap">{{ formatDateTime(row.received_at) }}</td>
              <td class="py-2 pr-3 text-secondary whitespace-nowrap">{{ row.workspace_id }}</td>
              <td class="py-2 pr-3 font-mono text-xs text-secondary" :title="row.user_uuid ?? ''">
                {{ shortId(row.user_uuid) }}
              </td>
              <td class="py-2 pr-3 text-primary max-w-md">
                <span class="line-clamp-2" :title="row.description">{{ row.description }}</span>
              </td>
              <td class="py-2 pr-3 text-secondary truncate max-w-xs" :title="row.url">{{ row.url }}</td>
              <td class="py-2 font-mono text-xs text-secondary whitespace-nowrap">{{ row.build_sha.slice(0, 12) }}</td>
            </tr>
          </tbody>
        </table>
      </template>
    </div>
  </div>
</template>
