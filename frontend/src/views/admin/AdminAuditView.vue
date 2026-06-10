<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useInfiniteQuery } from '@pinia/colada';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import type { IconName } from '@/components/common/icons';
import { formatDate, formatDateTime, formatRelativeTime } from '@/utils/dateUtils';
import { auditService, type AuditEntry, type AuditPage, type AuditQuery } from '@/services/auditService';
import { auditKeys } from '@/queries/audit';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Polite live-region message: announces the result count after a
// filter/first-load and how many rows were appended on "Load more", so
// non-sighted users learn the outcome of an auto-applied filter.
const liveMessage = ref('');

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

const expanded = ref<Record<string, boolean>>({});
const isExporting = ref(false);
const errorMessage = ref('');

const route = useRoute();
const router = useRouter();

// Hydrate filters from the URL so a shared/bookmarked audit view restores
// its filter set, then mirror changes back (replace, not push, so typing a
// filter doesn't stack history entries).
tierFilter.value = route.query.tier ? Number(route.query.tier) : undefined;
eventPrefix.value = typeof route.query.event === 'string' ? route.query.event : '';
actorFilter.value = typeof route.query.actor === 'string' ? route.query.actor : '';
severityFilter.value = typeof route.query.severity === 'string' ? route.query.severity : '';

watch(
  [tierFilter, eventPrefix, actorFilter, severityFilter],
  () => {
    const query: Record<string, string> = {};
    if (tierFilter.value !== undefined) query.tier = String(tierFilter.value);
    if (eventPrefix.value.trim()) query.event = eventPrefix.value.trim();
    if (actorFilter.value.trim()) query.actor = actorFilter.value.trim();
    if (severityFilter.value) query.severity = severityFilter.value;
    void router.replace({ query });
  },
  { flush: 'post' },
);

function buildQuery(cursor?: string): AuditQuery {
  const q: AuditQuery = { limit: 50 };
  if (tierFilter.value !== undefined) q.tier = tierFilter.value;
  if (eventPrefix.value.trim()) q.event_prefix = eventPrefix.value.trim();
  if (actorFilter.value.trim()) q.actor_uuid = actorFilter.value.trim();
  if (severityFilter.value) q.severity = severityFilter.value;
  if (cursor) q.cursor = cursor;
  return q;
}

// The filter set IS the cache key, so a change swaps to a cached page
// (instant) or fetches fresh — no manual reload wiring. Cursor pagination
// rides `pageParam`; the cursor is excluded from the key.
const cacheKey = computed(() =>
  JSON.stringify({
    tier: tierFilter.value,
    event: eventPrefix.value.trim(),
    actor: actorFilter.value.trim(),
    severity: severityFilter.value,
  }),
);

const auditList = useInfiniteQuery(() => ({
  key: auditKeys.list('infinite', cacheKey.value),
  initialPageParam: null as string | null,
  query: ({ pageParam }): Promise<AuditPage> =>
    auditService.list(buildQuery((pageParam as string | null) ?? undefined)),
  getNextPageParam: (lastPage: AuditPage) => lastPage.next_cursor,
  enabled: true,
}));

const entries = computed<AuditEntry[]>(
  () => (auditList.data.value?.pages as AuditPage[] | undefined)?.flatMap((p) => p.entries) ?? [],
);
const hasMore = computed(() => auditList.hasNextPage.value);
const isFetching = computed(() => auditList.asyncStatus.value === 'loading');
const isFirstLoad = computed(() => isFetching.value && entries.value.length === 0);
const isLoadingMore = computed(() => isFetching.value && entries.value.length > 0);
const displayError = computed(() => {
  if (errorMessage.value) return errorMessage.value;
  return auditList.error.value
    ? extractErrorMessage(auditList.error.value, t('admin-audit-error-load'))
    : '';
});

// Announce the result count once a fetch settles (first load, filter
// change, or load-more); clear stale expanded rows when the filter changes.
watch(isFetching, (now, was) => {
  if (was && !now) liveMessage.value = t('admin-audit-live-count', { count: entries.value.length });
});
watch(cacheKey, () => {
  expanded.value = {};
});

async function loadMore() {
  if (!hasMore.value || isLoadingMore.value) return;
  const prevLen = entries.value.length;
  await auditList.loadNextPage();
  // Move focus to the first newly-appended row so a keyboard user lands
  // on the new content instead of a button that just shifted down.
  await nextTick();
  const firstNew = entries.value[prevLen];
  if (firstNew) document.getElementById(`audit-toggle-${firstNew.id}`)?.focus();
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

async function copyId(value: string | null | undefined) {
  if (!value) return;
  try {
    await navigator.clipboard.writeText(value);
  } catch {
    // Clipboard can be unavailable (insecure context); fail quietly.
  }
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

// Severity uses semantic status tokens (themed + dark-mode aware) and is
// always paired with an icon, so the signal never rests on colour alone.
function severityTone(severity: string): string {
  switch (severity) {
    case 'error':
      return 'bg-status-error-muted text-status-error';
    case 'warning':
      return 'bg-status-warning-muted text-status-warning';
    default:
      return 'bg-surface-alt text-secondary';
  }
}
function severityIcon(severity: string): IconName {
  return severity === 'error' ? 'xCircle' : 'warning';
}

// Humanise the machine event token (`ticket.workflow_state_changed`)
// into a readable label. The raw token is still rendered beneath it as a
// monospace badge so it stays the stable filter / debug key.
function humanizeEvent(eventType: string): string {
  const dot = eventType.indexOf('.');
  const resource = (dot === -1 ? eventType : eventType.slice(0, dot)).replace(/_/g, ' ');
  const action = dot === -1 ? '' : eventType.slice(dot + 1).replace(/[._]/g, ' ');
  const phrase = action ? `${resource} ${action}` : resource;
  return phrase.charAt(0).toUpperCase() + phrase.slice(1);
}

// One icon per event shape, derived from the verb, so a reviewer can
// pre-sort the stream by glyph before reading.
function eventIcon(eventType: string): IconName {
  const e = eventType.toLowerCase();
  if (/(delete|removed|uninstall|revoke)/.test(e)) return 'trash';
  if (/(create|added|install|invite)/.test(e)) return 'add';
  if (/(login|logout|auth|mfa|password|session|token)/.test(e)) return 'key';
  if (/archive/.test(e)) return 'archive';
  if (/(complete|merged|approve)/.test(e)) return 'check';
  if (/(update|changed|renamed|edit|moved)/.test(e)) return 'documentEdit';
  return 'info';
}

function actorKindLabel(kind: string): string {
  switch (kind) {
    case 'token':
    case 'api_token':
    case 'api':
      return t('admin-audit-actor-token');
    case 'anonymous':
    case 'guest':
      return t('admin-audit-actor-anonymous');
    case 'system':
      return t('admin-audit-actor-system');
    default:
      return kind || t('admin-audit-actor-system');
  }
}
function actorKindIcon(kind: string): IconName {
  switch (kind) {
    case 'token':
    case 'api_token':
    case 'api':
      return 'key';
    case 'system':
      return 'settings';
    default:
      return 'user';
  }
}

function targetLabel(entry: AuditEntry): string {
  if (!entry.target) return '';
  return `${entry.target.kind}${entry.target.id ? '.' + entry.target.id : ''}`;
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

// Day label for the group header: Today / Yesterday / absolute date.
function dayLabel(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diffDays = Math.round((startOf(now) - startOf(d)) / 86_400_000);
  if (diffDays === 0) return t('admin-audit-day-today');
  if (diffDays === 1) return t('admin-audit-day-yesterday');
  return formatDate(iso);
}

// Group the (already newest-first) entries under day headers, preserving
// order. Consecutive entries on the same calendar day share a group.
const groupedEntries = computed(() => {
  const groups: { key: string; label: string; items: AuditEntry[] }[] = [];
  let current: { key: string; label: string; items: AuditEntry[] } | null = null;
  for (const entry of entries.value) {
    const key = entry.occurred_at.slice(0, 10);
    if (!current || current.key !== key) {
      current = { key, label: dayLabel(entry.occurred_at), items: [] };
      groups.push(current);
    }
    current.items.push(entry);
  }
  return groups;
});

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
</script>

<template>
  <div class="flex flex-col gap-6 p-6" :aria-busy="isFetching">
    <p role="status" aria-live="polite" class="sr-only">{{ liveMessage }}</p>
    <header class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex flex-col gap-2">
        <h1 class="text-2xl font-semibold">{{ $t('admin-audit-title') }}</h1>
        <p class="text-sm text-secondary">{{ $t('admin-audit-description') }}</p>
      </div>
      <button
        type="button"
        class="h-9 px-4 rounded border border-default text-sm hover:bg-surface-hover disabled:opacity-50 inline-flex items-center gap-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
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
          class="h-8 px-3 rounded-full text-sm border transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          :class="
            tierFilter === tier.value
              ? 'bg-accent/10 border-accent text-accent font-medium'
              : 'border-default text-secondary hover:bg-surface-hover'
          "
          :aria-pressed="tierFilter === tier.value"
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
            class="h-9 px-2 rounded border border-default bg-surface-alt text-primary text-sm w-full sm:w-44 font-mono focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
          />
        </label>
        <label class="flex flex-col gap-1 text-xs text-secondary">
          <span>{{ $t('admin-audit-filter-severity') }}</span>
          <select
            v-model="severityFilter"
            class="h-9 px-2 rounded border border-default bg-surface-alt text-primary text-sm focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
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
            class="h-9 px-2 rounded border border-default bg-surface-alt text-primary text-sm w-full sm:w-72 font-mono focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
          />
        </label>
        <button
          v-if="hasFilters"
          type="button"
          class="h-9 px-3 rounded border border-default text-sm hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          @click="clearFilters"
        >
          {{ $t('admin-audit-clear-filters') }}
        </button>
      </div>
    </section>

    <AlertMessage v-if="displayError" type="error" :message="displayError" />

    <div v-if="isFirstLoad" class="py-12 flex justify-center">
      <LoadingSpinner />
    </div>

    <EmptyState
      v-else-if="entries.length === 0"
      icon="inbox"
      :title="hasFilters ? $t('admin-audit-empty-filtered-title') : $t('admin-audit-empty-title')"
      :description="hasFilters ? $t('admin-audit-empty-filtered-description') : $t('admin-audit-empty-description')"
    />

    <table v-else class="w-full text-sm border-separate border-spacing-0">
      <thead>
        <tr class="text-left text-xs text-tertiary">
          <th scope="col" class="w-8 pb-2 pl-2 font-medium"><span class="sr-only">{{ $t('admin-audit-col-details') }}</span></th>
          <th scope="col" class="pb-2 font-medium">{{ $t('admin-audit-col-event') }}</th>
          <th scope="col" class="pb-2 font-medium">{{ $t('admin-audit-col-actor') }}</th>
          <th scope="col" class="hidden pb-2 font-medium md:table-cell">{{ $t('admin-audit-col-target') }}</th>
          <th scope="col" class="pb-2 pr-2 text-right font-medium">{{ $t('admin-audit-col-time') }}</th>
        </tr>
      </thead>

      <tbody v-for="group in groupedEntries" :key="group.key">
        <tr>
          <th
            scope="colgroup"
            colspan="5"
            class="pt-4 pb-1 pl-2 text-left text-[10px] font-semibold uppercase tracking-wide text-tertiary"
          >
            {{ group.label }}
          </th>
        </tr>

        <template v-for="entry in group.items" :key="entry.id">
          <tr
            class="border-t border-subtle hover:bg-surface-hover cursor-pointer"
            @click="toggleExpanded(entry.id)"
          >
            <td class="py-2 pl-2 align-top">
              <button
                :id="`audit-toggle-${entry.id}`"
                type="button"
                class="flex h-6 w-6 items-center justify-center rounded text-tertiary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                :aria-expanded="!!expanded[entry.id]"
                :aria-controls="`audit-detail-${entry.id}`"
                :aria-label="`${humanizeEvent(entry.event_type)}, ${formatRelativeTime(entry.occurred_at)}`"
                @click.stop="toggleExpanded(entry.id)"
              >
                <Icon :name="eventIcon(entry.event_type)" size="sm" />
              </button>
            </td>

            <td class="py-2 align-top">
              <div class="flex flex-wrap items-center gap-2">
                <span class="text-primary">{{ humanizeEvent(entry.event_type) }}</span>
                <span
                  v-if="entry.severity && entry.severity !== 'info'"
                  class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs"
                  :class="severityTone(entry.severity)"
                >
                  <Icon :name="severityIcon(entry.severity)" size="xs" aria-hidden="true" />
                  {{ entry.severity }}
                </span>
              </div>
              <code class="font-mono text-[11px] text-tertiary">{{ entry.event_type }}</code>
            </td>

            <td class="py-2 align-top">
              <UserAvatar
                v-if="entry.actor_uuid"
                :uuid="entry.actor_uuid"
                :fallback-name="entry.actor_name ?? undefined"
                size="xxs"
              />
              <span v-else class="inline-flex items-center gap-1 text-xs text-secondary">
                <Icon :name="actorKindIcon(entry.actor_kind)" size="xs" aria-hidden="true" />
                {{ actorKindLabel(entry.actor_kind) }}
              </span>
            </td>

            <td class="hidden py-2 align-top text-secondary md:table-cell">
              <span v-if="entry.target" class="font-mono text-xs">{{ targetLabel(entry) }}</span>
              <span v-else class="text-tertiary">-</span>
            </td>

            <td
              class="whitespace-nowrap py-2 pr-2 text-right align-top text-xs text-secondary"
              :title="formatDateTime(entry.occurred_at)"
            >
              {{ formatRelativeTime(entry.occurred_at) }}
            </td>
          </tr>

          <tr :id="`audit-detail-${entry.id}`" :hidden="!expanded[entry.id]">
            <td colspan="5" class="bg-surface-alt/40 px-3 pb-3 pt-1">
              <template v-if="expanded[entry.id]">
              <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
                <dt class="text-secondary">{{ $t('admin-audit-source-field') }}</dt>
                <dd class="text-primary">{{ sourceLabel(entry.source) }}</dd>

                <template v-if="entry.actor_uuid">
                  <dt class="text-secondary">{{ $t('admin-audit-col-actor') }}</dt>
                  <dd class="flex items-center gap-1">
                    <span class="font-mono text-primary">{{ entry.actor_uuid }}</span>
                    <button
                      type="button"
                      class="text-tertiary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded"
                      :aria-label="$t('admin-audit-copy')"
                      @click="copyId(entry.actor_uuid)"
                    >
                      <Icon name="copy" size="xs" />
                    </button>
                  </dd>
                </template>

                <template v-if="entry.target">
                  <dt class="text-secondary">{{ $t('admin-audit-target') }}</dt>
                  <dd class="font-mono text-primary">{{ targetLabel(entry) }}</dd>
                </template>

                <template v-if="entry.source_ip">
                  <dt class="text-secondary">{{ $t('admin-audit-source-ip') }}</dt>
                  <dd class="font-mono text-primary">{{ entry.source_ip }}</dd>
                </template>

                <template v-if="entry.correlation_id">
                  <dt class="text-secondary">{{ $t('admin-audit-corr') }}</dt>
                  <dd class="flex items-center gap-1">
                    <span class="font-mono text-primary">{{ entry.correlation_id }}</span>
                    <button
                      type="button"
                      class="text-tertiary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded"
                      :aria-label="$t('admin-audit-copy')"
                      @click="copyId(entry.correlation_id)"
                    >
                      <Icon name="copy" size="xs" />
                    </button>
                  </dd>
                </template>
              </dl>

              <!-- Tier-3 row diff -->
              <table v-if="entry.diff.length" class="mt-2 w-full text-sm">
                <thead>
                  <tr class="text-xs text-secondary">
                    <th class="pb-1 pr-4 text-left font-medium">{{ $t('admin-audit-diff-field') }}</th>
                    <th class="pb-1 pr-4 text-left font-medium">{{ $t('admin-audit-diff-old') }}</th>
                    <th class="pb-1 text-left font-medium">{{ $t('admin-audit-diff-new') }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="d in entry.diff" :key="d.field" class="border-t border-default">
                    <td class="py-1 pr-4 font-mono text-primary">{{ d.field }}</td>
                    <td class="max-w-xs truncate py-1 pr-4 text-secondary" :title="previewValue(d.old)">
                      {{ previewValue(d.old) }}
                    </td>
                    <td class="max-w-xs truncate py-1 text-primary" :title="previewValue(d.new)">
                      {{ previewValue(d.new) }}
                    </td>
                  </tr>
                </tbody>
              </table>

              <!-- Tier-1 / tier-2 payload -->
              <div v-else-if="entry.payload" class="mt-2">
                <p class="mb-1 text-xs text-secondary">{{ $t('admin-audit-payload') }}</p>
                <pre class="overflow-auto rounded bg-surface-alt p-2 font-mono text-xs text-primary">{{ prettyJson(entry.payload) }}</pre>
              </div>

              <p v-else class="mt-2 text-xs text-secondary">{{ $t('admin-audit-no-diff') }}</p>
              </template>
            </td>
          </tr>
        </template>
      </tbody>
    </table>

    <div v-if="hasMore" class="flex justify-center pt-2">
      <button
        type="button"
        class="h-9 px-4 rounded border border-default text-sm hover:bg-surface-hover disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :disabled="isLoadingMore"
        @click="loadMore"
      >
        {{ isLoadingMore ? $t('admin-audit-loading-more') : $t('admin-audit-load-more') }}
      </button>
    </div>
  </div>
</template>
