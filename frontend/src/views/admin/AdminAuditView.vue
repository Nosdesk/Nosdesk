<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import { formatDateTime } from '@/utils/dateUtils';
import { auditService, type AuditEntry, type AuditQuery } from '@/services/auditService';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

// Tier chips. `undefined` = all sources.
const TIERS = [
  { value: undefined as number | undefined, labelKey: 'admin-audit-tier-all' },
  { value: 1, labelKey: 'admin-audit-tier-app' },
  { value: 2, labelKey: 'admin-audit-tier-auth' },
  { value: 3, labelKey: 'admin-audit-tier-change' },
];

const tierFilter = ref<number | undefined>(undefined);
const eventPrefix = ref('');
const actorFilter = ref('');
const severityFilter = ref('');

const entries = ref<AuditEntry[]>([]);
const nextCursor = ref<string | null>(null);
const expanded = ref<Record<string, boolean>>({});

const isLoading = ref(false);
const isLoadingMore = ref(false);
const isExporting = ref(false);
const errorMessage = ref('');

function buildQuery(cursor?: string): AuditQuery {
  const q: AuditQuery = { limit: 50 };
  if (tierFilter.value !== undefined) q.tier = tierFilter.value;
  if (eventPrefix.value.trim()) q.event_prefix = eventPrefix.value.trim();
  if (actorFilter.value.trim()) q.actor_uuid = actorFilter.value.trim();
  if (severityFilter.value) q.severity = severityFilter.value;
  if (cursor) q.cursor = cursor;
  return q;
}

async function loadFirstPage() {
  isLoading.value = true;
  errorMessage.value = '';
  expanded.value = {};
  try {
    const page = await auditService.list(buildQuery());
    entries.value = page.entries;
    nextCursor.value = page.next_cursor;
  } catch (err) {
    errorMessage.value = readError(err, 'admin-audit-error-load');
    entries.value = [];
    nextCursor.value = null;
  } finally {
    isLoading.value = false;
  }
}

async function loadMore() {
  if (!nextCursor.value || isLoadingMore.value) return;
  isLoadingMore.value = true;
  try {
    const page = await auditService.list(buildQuery(nextCursor.value));
    entries.value.push(...page.entries);
    nextCursor.value = page.next_cursor;
  } catch (err) {
    errorMessage.value = readError(err, 'admin-audit-error-load-more');
  } finally {
    isLoadingMore.value = false;
  }
}

async function exportJson() {
  if (isExporting.value) return;
  isExporting.value = true;
  errorMessage.value = '';
  try {
    const data = await auditService.export(buildQuery());
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `audit-export-${new Date().toISOString().slice(0, 10)}.json`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  } catch (err) {
    errorMessage.value = readError(err, 'admin-audit-error-load');
  } finally {
    isExporting.value = false;
  }
}

function readError(err: unknown, fallbackKey: string): string {
  return extractErrorMessage(err, t(fallbackKey));
}

function toggleExpanded(id: string) {
  expanded.value[id] = !expanded.value[id];
}

function sourceLabel(source: string): string {
  switch (source) {
    case 'tier1':
      return t('admin-audit-source-tier1');
    case 'tier2':
      return t('admin-audit-source-tier2');
    default:
      return t('admin-audit-source-tier3');
  }
}

function sourceTone(source: string): string {
  switch (source) {
    case 'tier1':
      return 'bg-blue-500/10 text-blue-700 dark:text-blue-400';
    case 'tier2':
      return 'bg-purple-500/10 text-purple-700 dark:text-purple-400';
    default:
      return 'bg-amber-500/10 text-amber-700 dark:text-amber-400';
  }
}

function severityTone(severity: string): string {
  switch (severity) {
    case 'warning':
      return 'bg-amber-500/10 text-amber-700 dark:text-amber-400';
    case 'error':
      return 'bg-red-500/10 text-red-700 dark:text-red-400';
    default:
      return 'bg-default text-secondary';
  }
}

function actorLabel(entry: AuditEntry): string {
  if (entry.actor_uuid) return entry.actor_uuid.slice(0, 8);
  // No actor uuid: surface the kind (system / anonymous).
  return entry.actor_kind || t('admin-audit-actor-system');
}

function previewValue(value: unknown): string {
  if (value === null || value === undefined) return '-';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    const json = JSON.stringify(value);
    return json.length > 80 ? json.slice(0, 80) + '…' : json;
  } catch {
    return '[unserialisable]';
  }
}

function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

const hasFilters = computed(
  () =>
    tierFilter.value !== undefined ||
    eventPrefix.value.trim() !== '' ||
    actorFilter.value.trim() !== '' ||
    severityFilter.value !== '',
);

function clearFilters() {
  tierFilter.value = undefined;
  eventPrefix.value = '';
  actorFilter.value = '';
  severityFilter.value = '';
}

watch([tierFilter, eventPrefix, actorFilter, severityFilter], () => void loadFirstPage(), {
  flush: 'post',
});

onMounted(loadFirstPage);
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex flex-col gap-2">
        <h1 class="text-2xl font-semibold">{{ $t('admin-audit-title') }}</h1>
        <p class="text-sm text-secondary">{{ $t('admin-audit-description') }}</p>
      </div>
      <button
        type="button"
        class="h-9 px-4 rounded border border-default text-sm hover:bg-hover disabled:opacity-50 inline-flex items-center gap-2"
        :disabled="isExporting || entries.length === 0"
        @click="exportJson"
      >
        <Icon name="download" size="sm" />
        {{ isExporting ? $t('admin-audit-exporting') : $t('admin-audit-export') }}
      </button>
    </header>

    <section class="flex flex-col gap-3">
      <!-- Tier chips -->
      <div class="flex flex-wrap gap-1">
        <button
          v-for="tier in TIERS"
          :key="tier.labelKey"
          type="button"
          class="h-8 px-3 rounded-full text-sm border transition-colors"
          :class="
            tierFilter === tier.value
              ? 'bg-accent/10 border-accent text-accent font-medium'
              : 'border-default text-secondary hover:bg-hover'
          "
          @click="tierFilter = tier.value"
        >
          {{ $t(tier.labelKey) }}
        </button>
      </div>

      <div class="flex flex-wrap gap-3 items-end">
        <label class="flex flex-col gap-1 text-xs text-secondary">
          <span>{{ $t('admin-audit-filter-event') }}</span>
          <input
            v-model="eventPrefix"
            type="text"
            :placeholder="$t('admin-audit-filter-event-placeholder')"
            class="h-9 px-2 rounded border border-default bg-input text-primary text-sm w-full sm:w-44 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 text-xs text-secondary">
          <span>{{ $t('admin-audit-filter-severity') }}</span>
          <select
            v-model="severityFilter"
            class="h-9 px-2 rounded border border-default bg-input text-primary text-sm"
          >
            <option value="">{{ $t('admin-audit-severity-any') }}</option>
            <option value="info">info</option>
            <option value="warning">warning</option>
            <option value="error">error</option>
          </select>
        </label>
        <label class="flex flex-col gap-1 text-xs text-secondary w-full sm:w-auto">
          <span>{{ $t('admin-audit-filter-actor') }}</span>
          <input
            v-model="actorFilter"
            type="text"
            :placeholder="$t('admin-audit-filter-actor-placeholder')"
            class="h-9 px-2 rounded border border-default bg-input text-primary text-sm w-full sm:w-72 font-mono"
          />
        </label>
        <button
          v-if="hasFilters"
          type="button"
          class="h-9 px-3 rounded border border-default text-sm hover:bg-hover"
          @click="clearFilters"
        >
          {{ $t('admin-audit-clear-filters') }}
        </button>
      </div>
    </section>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <div v-if="isLoading" class="py-12 flex justify-center">
      <LoadingSpinner />
    </div>

    <EmptyState
      v-else-if="entries.length === 0"
      icon="inbox"
      :title="$t('admin-audit-empty-title')"
      :description="$t('admin-audit-empty-description')"
    />

    <ul v-else class="flex flex-col gap-1">
      <li v-for="entry in entries" :key="entry.id" class="rounded border border-default bg-surface">
        <button
          type="button"
          class="w-full flex items-center gap-3 px-3 py-2 text-left hover:bg-hover"
          @click="toggleExpanded(entry.id)"
        >
          <span class="text-xs font-medium px-2 py-0.5 rounded" :class="sourceTone(entry.source)">
            {{ sourceLabel(entry.source) }}
          </span>
          <span class="text-sm font-mono text-primary truncate max-w-[16rem]">
            {{ entry.event_type }}
          </span>
          <span
            v-if="entry.severity && entry.severity !== 'info'"
            class="text-xs px-2 py-0.5 rounded"
            :class="severityTone(entry.severity)"
          >
            {{ entry.severity }}
          </span>
          <span class="text-sm text-secondary flex-1 truncate">
            {{ $t('admin-audit-by') }}
            <span class="font-mono">{{ actorLabel(entry) }}</span>
            <template v-if="entry.target">
              · <span class="font-mono">{{ entry.target.kind }}{{ entry.target.id ? '.' + entry.target.id : '' }}</span>
            </template>
          </span>
          <span class="text-xs text-secondary whitespace-nowrap">
            {{ formatDateTime(entry.occurred_at) }}
          </span>
          <Icon :name="expanded[entry.id] ? 'chevronUp' : 'chevronDown'" size="sm" class="text-secondary" />
        </button>

        <div v-if="expanded[entry.id]" class="px-3 pb-3 flex flex-col gap-2">
          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
            <template v-if="entry.target">
              <dt class="text-secondary">{{ $t('admin-audit-target') }}</dt>
              <dd class="font-mono text-primary">
                {{ entry.target.kind }}{{ entry.target.id ? '.' + entry.target.id : '' }}
              </dd>
            </template>
            <template v-if="entry.source_ip">
              <dt class="text-secondary">{{ $t('admin-audit-source-ip') }}</dt>
              <dd class="font-mono text-primary">{{ entry.source_ip }}</dd>
            </template>
            <template v-if="entry.correlation_id">
              <dt class="text-secondary">{{ $t('admin-audit-corr') }}</dt>
              <dd class="font-mono text-primary">{{ entry.correlation_id.slice(0, 8) }}</dd>
            </template>
          </dl>

          <!-- Tier-3 row diff -->
          <table v-if="entry.diff.length" class="w-full text-sm">
            <thead>
              <tr class="text-xs text-secondary">
                <th class="text-left font-medium pb-1 pr-4">{{ $t('admin-audit-diff-field') }}</th>
                <th class="text-left font-medium pb-1 pr-4">{{ $t('admin-audit-diff-old') }}</th>
                <th class="text-left font-medium pb-1">{{ $t('admin-audit-diff-new') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="d in entry.diff" :key="d.field" class="border-t border-default">
                <td class="py-1 pr-4 font-mono text-primary">{{ d.field }}</td>
                <td class="py-1 pr-4 text-secondary truncate max-w-xs" :title="previewValue(d.old)">
                  {{ previewValue(d.old) }}
                </td>
                <td class="py-1 text-primary truncate max-w-xs" :title="previewValue(d.new)">
                  {{ previewValue(d.new) }}
                </td>
              </tr>
            </tbody>
          </table>

          <!-- Tier-1 / tier-2 payload -->
          <div v-else-if="entry.payload">
            <p class="text-xs text-secondary mb-1">{{ $t('admin-audit-payload') }}</p>
            <pre class="font-mono text-xs overflow-auto bg-input p-2 rounded text-primary">{{ prettyJson(entry.payload) }}</pre>
          </div>

          <p v-else class="text-xs text-secondary">{{ $t('admin-audit-no-diff') }}</p>
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
        {{ isLoadingMore ? $t('admin-audit-loading-more') : $t('admin-audit-load-more') }}
      </button>
    </div>
  </div>
</template>
