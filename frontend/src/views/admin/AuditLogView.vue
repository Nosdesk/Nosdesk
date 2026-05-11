<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import {
  auditLogService,
  type AuditLogQuery,
  type AuditLogRow,
} from '@/services/auditLogService';

// Tier-1 audited tables (matches the migration
// 2026-05-11-210000_attach_audit_tier1). Hardcoded rather than
// fetched: the list changes only when a migration adds a trigger,
// so a constant keeps the dropdown decisions out of the network
// path. If the list grows enough to be unwieldy, swap to
// `GET /admin/audit-log/tables`.
const AUDITED_TABLES = [
  'tickets',
  'users',
  'groups',
  'ticket_categories',
  'workflow_states',
  'assignment_rules',
  'sla_policies',
  'webhooks',
  'site_settings',
  'plugin_data',
  'plugin_collection_rows',
  'webhook_deliveries',
  'user_ticket_views',
] as const;

type FilterTable = '' | (typeof AUDITED_TABLES)[number];

const tableFilter = ref<FilterTable>('');
const pkFilter = ref('');
const actorFilter = ref('');

const rows = ref<AuditLogRow[]>([]);
const nextCursor = ref<string | null>(null);
// Direct property mutation on a reactive Record is intercepted by Vue's
// proxy and triggers reactivity automatically — simpler than the
// `expanded.value = new Set(expanded.value)` reassignment dance a Set
// would require.
const expanded = ref<Record<number, boolean>>({});

const isLoading = ref(false);
const isLoadingMore = ref(false);
const errorMessage = ref('');

function buildQuery(cursor?: string): AuditLogQuery {
  const q: AuditLogQuery = { limit: 50 };
  if (tableFilter.value) q.table_name = tableFilter.value;
  if (pkFilter.value.trim()) q.pk_text = pkFilter.value.trim();
  if (actorFilter.value.trim()) q.actor_uuid = actorFilter.value.trim();
  if (cursor) q.cursor = cursor;
  return q;
}

async function loadFirstPage() {
  isLoading.value = true;
  errorMessage.value = '';
  expanded.value = {};
  try {
    const page = await auditLogService.list(buildQuery());
    rows.value = page.rows;
    nextCursor.value = page.next_cursor;
  } catch (err) {
    const e = err as { response?: { data?: { message?: string } }; message?: string };
    errorMessage.value =
      e.response?.data?.message || e.message || 'Failed to load audit log';
    rows.value = [];
    nextCursor.value = null;
  } finally {
    isLoading.value = false;
  }
}

async function loadMore() {
  if (!nextCursor.value || isLoadingMore.value) return;
  isLoadingMore.value = true;
  try {
    const page = await auditLogService.list(buildQuery(nextCursor.value));
    rows.value.push(...page.rows);
    nextCursor.value = page.next_cursor;
  } catch (err) {
    const e = err as { response?: { data?: { message?: string } }; message?: string };
    errorMessage.value =
      e.response?.data?.message || e.message || 'Failed to load more audit log entries';
  } finally {
    isLoadingMore.value = false;
  }
}

function toggleExpanded(id: number) {
  expanded.value[id] = !expanded.value[id];
}

function opLabel(op: string): string {
  switch (op) {
    case 'I':
      return 'Created';
    case 'U':
      return 'Updated';
    case 'D':
      return 'Deleted';
    default:
      return op;
  }
}

function opTone(op: string): string {
  switch (op) {
    case 'I':
      return 'bg-green-500/10 text-green-700 dark:text-green-400';
    case 'U':
      return 'bg-blue-500/10 text-blue-700 dark:text-blue-400';
    case 'D':
      return 'bg-red-500/10 text-red-700 dark:text-red-400';
    default:
      return 'bg-default text-secondary';
  }
}

function shortUuid(uuid: string | null): string {
  if (!uuid) return 'system';
  return uuid.slice(0, 8);
}

function formatDateTime(iso: string): string {
  return new Date(iso).toLocaleString();
}

function previewValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    const json = JSON.stringify(value);
    return json.length > 80 ? json.slice(0, 80) + '…' : json;
  } catch {
    return '[unserialisable]';
  }
}

const hasFilters = computed(
  () => tableFilter.value !== '' || pkFilter.value.trim() !== '' || actorFilter.value.trim() !== '',
);

// Reset cursor + reload whenever a filter changes. Debounced via Vue's
// flush:'post' so consecutive setState calls collapse into one fetch.
watch(
  [tableFilter, pkFilter, actorFilter],
  () => {
    void loadFirstPage();
  },
  { flush: 'post' },
);

onMounted(loadFirstPage);
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header class="flex flex-col gap-2">
      <h1 class="text-2xl font-semibold">Audit log</h1>
      <p class="text-sm text-secondary">
        Forensic record of who changed what across audited entities. Defaults to the last 7 days
        and the most recent 50 entries; refine with the filters below.
      </p>
    </header>

    <section class="flex flex-wrap gap-3 items-end">
      <label class="flex flex-col gap-1 text-xs text-secondary">
        <span>Entity</span>
        <select
          v-model="tableFilter"
          class="h-9 px-2 rounded border border-default bg-input text-primary text-sm"
        >
          <option value="">Any</option>
          <option v-for="t in AUDITED_TABLES" :key="t" :value="t">{{ t }}</option>
        </select>
      </label>
      <label class="flex flex-col gap-1 text-xs text-secondary">
        <span>Entity ID</span>
        <input
          v-model="pkFilter"
          type="text"
          placeholder="e.g. 42"
          class="h-9 px-2 rounded border border-default bg-input text-primary text-sm w-32"
        />
      </label>
      <label class="flex flex-col gap-1 text-xs text-secondary">
        <span>Actor UUID</span>
        <input
          v-model="actorFilter"
          type="text"
          placeholder="e.g. 0192…"
          class="h-9 px-2 rounded border border-default bg-input text-primary text-sm w-72 font-mono"
        />
      </label>
      <button
        v-if="hasFilters"
        type="button"
        class="h-9 px-3 rounded border border-default text-sm hover:bg-hover"
        @click="(tableFilter = ''), (pkFilter = ''), (actorFilter = '')"
      >
        Clear filters
      </button>
    </section>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <div v-if="isLoading" class="py-12 flex justify-center">
      <LoadingSpinner />
    </div>

    <EmptyState
      v-else-if="rows.length === 0"
      icon="inbox"
      title="No audit entries"
      description="Either no audited entities have changed in the selected window, or the filters exclude every row."
    />

    <ul v-else class="flex flex-col gap-1">
      <li
        v-for="row in rows"
        :key="row.id"
        class="rounded border border-default bg-surface"
      >
        <button
          type="button"
          class="w-full flex items-center gap-3 px-3 py-2 text-left hover:bg-hover"
          @click="toggleExpanded(row.id)"
        >
          <span
            class="text-xs font-medium px-2 py-0.5 rounded"
            :class="opTone(row.op)"
          >
            {{ opLabel(row.op) }}
          </span>
          <span class="text-sm font-mono text-primary">
            {{ row.table_name }}.{{ row.pk_text }}
          </span>
          <span class="text-sm text-secondary flex-1 truncate">
            by <span class="font-mono">{{ shortUuid(row.actor_uuid) }}</span>
            <template v-if="row.correlation_id">
              · corr <span class="font-mono">{{ shortUuid(row.correlation_id) }}</span>
            </template>
          </span>
          <span class="text-xs text-secondary whitespace-nowrap">
            {{ formatDateTime(row.occurred_at) }}
          </span>
          <Icon
            :name="expanded[row.id] ? 'chevronUp' : 'chevronDown'"
            size="sm"
            class="text-secondary"
          />
        </button>
        <div v-if="expanded[row.id]" class="px-3 pb-3">
          <table v-if="row.diff.length" class="w-full text-sm">
            <thead>
              <tr class="text-xs text-secondary">
                <th class="text-left font-medium pb-1 pr-4">Field</th>
                <th class="text-left font-medium pb-1 pr-4">Old</th>
                <th class="text-left font-medium pb-1">New</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="entry in row.diff" :key="entry.field" class="border-t border-default">
                <td class="py-1 pr-4 font-mono text-primary">{{ entry.field }}</td>
                <td class="py-1 pr-4 text-secondary truncate max-w-xs" :title="previewValue(entry.old)">
                  {{ previewValue(entry.old) }}
                </td>
                <td class="py-1 text-primary truncate max-w-xs" :title="previewValue(entry.new)">
                  {{ previewValue(entry.new) }}
                </td>
              </tr>
            </tbody>
          </table>
          <p v-else class="text-xs text-secondary">No field-level diff for this entry.</p>
        </div>
      </li>
    </ul>

    <div v-if="nextCursor" class="flex justify-center pt-2">
      <button
        type="button"
        class="h-9 px-4 rounded border border-default text-sm hover:bg-hover disabled:opacity-50"
        :disabled="isLoadingMore"
        @click="loadMore"
      >
        {{ isLoadingMore ? 'Loading…' : 'Load more' }}
      </button>
    </div>
  </div>
</template>
