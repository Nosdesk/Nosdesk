<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import { formatDateTime } from '@/utils/dateUtils';
import {
  emailQueueService,
  type OutboundEmailQuery,
  type OutboundEmailRow,
  type OutboundEmailStats,
  type OutboundEmailStatus,
} from '@/services/emailQueueService';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Status filter — multi-select via comma-joined string. Aligns with the
// backend's parsing in handlers/email_queue.rs.
const STATUS_OPTIONS: OutboundEmailStatus[] = [
  'pending',
  'sending',
  'sent',
  'failed',
  'dead',
  'suppressed',
];

const statusFilter = ref<Set<OutboundEmailStatus>>(new Set());
const ticketFilter = ref('');
const domainFilter = ref('');

const rows = ref<OutboundEmailRow[]>([]);
const nextCursor = ref<string | null>(null);
const stats = ref<OutboundEmailStats | null>(null);
const expanded = ref<Record<number, boolean>>({});

const isLoading = ref(false);
const isLoadingMore = ref(false);
const errorMessage = ref('');

const showCancelConfirm = ref(false);
const pendingCancelId = ref<number | null>(null);

function buildQuery(cursor?: string): OutboundEmailQuery {
  const q: OutboundEmailQuery = { limit: 50 };
  if (statusFilter.value.size > 0) {
    q.status = [...statusFilter.value].join(',');
  }
  const ticketId = parseInt(ticketFilter.value.trim(), 10);
  if (!Number.isNaN(ticketId)) q.ticket_id = ticketId;
  if (domainFilter.value.trim()) q.recipient_domain = domainFilter.value.trim();
  if (cursor) q.cursor = cursor;
  return q;
}

async function loadStats() {
  try {
    stats.value = await emailQueueService.stats();
  } catch (err) {
    // Non-fatal: stats are decorative, the list is the load-bearing data.
    const e = err as { message?: string };
    errorMessage.value = e.message || t('admin-email-queue-error-stats');
  }
}

async function loadFirstPage() {
  isLoading.value = true;
  errorMessage.value = '';
  expanded.value = {};
  try {
    const page = await emailQueueService.list(buildQuery());
    rows.value = page.rows;
    nextCursor.value = page.next_cursor;
  } catch (err) {
    const e = err as { response?: { data?: { message?: string } }; message?: string };
    errorMessage.value =
      e.response?.data?.message || e.message || t('admin-email-queue-error-load');
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
    const page = await emailQueueService.list(buildQuery(nextCursor.value));
    rows.value.push(...page.rows);
    nextCursor.value = page.next_cursor;
  } catch (err) {
    const e = err as { response?: { data?: { message?: string } }; message?: string };
    errorMessage.value =
      e.response?.data?.message || e.message || t('admin-email-queue-error-load-more');
  } finally {
    isLoadingMore.value = false;
  }
}

function toggleExpanded(id: number) {
  expanded.value[id] = !expanded.value[id];
}

function toggleStatus(s: OutboundEmailStatus) {
  const next = new Set(statusFilter.value);
  if (next.has(s)) next.delete(s);
  else next.add(s);
  statusFilter.value = next;
}

async function retryNow(id: number) {
  try {
    await emailQueueService.retryNow(id);
    await Promise.all([loadFirstPage(), loadStats()]);
  } catch (err) {
    const e = err as { message?: string };
    errorMessage.value = e.message || t('admin-email-queue-error-retry');
  }
}

function cancelRow(id: number) {
  pendingCancelId.value = id;
  showCancelConfirm.value = true;
}

async function confirmCancel() {
  const id = pendingCancelId.value;
  showCancelConfirm.value = false;
  pendingCancelId.value = null;
  if (id === null) return;
  try {
    await emailQueueService.cancel(id);
    await Promise.all([loadFirstPage(), loadStats()]);
  } catch (err) {
    const e = err as { message?: string };
    errorMessage.value = e.message || t('admin-email-queue-error-cancel');
  }
}

function statusLabel(s: string): string {
  return t(`admin-email-queue-status-${s}`);
}

function statusTone(s: string): string {
  switch (s) {
    case 'sent':
      return 'bg-green-500/10 text-green-700 dark:text-green-400';
    case 'pending':
    case 'sending':
      return 'bg-blue-500/10 text-blue-700 dark:text-blue-400';
    case 'failed':
      return 'bg-amber-500/10 text-amber-700 dark:text-amber-400';
    case 'dead':
      return 'bg-red-500/10 text-red-700 dark:text-red-400';
    case 'suppressed':
      return 'bg-default text-secondary';
    default:
      return 'bg-default text-secondary';
  }
}

function formatAge(seconds: number | null): string {
  if (seconds === null || seconds < 0) return '—';
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

function recipientDomain(addr: string): string {
  const at = addr.indexOf('@');
  return at >= 0 ? addr.slice(at + 1) : addr;
}

const hasFilters = computed(
  () =>
    statusFilter.value.size > 0 ||
    ticketFilter.value.trim() !== '' ||
    domainFilter.value.trim() !== '',
);

const pendingTotal = computed(() => stats.value?.pending_total ?? 0);
const oldestPendingAge = computed(() =>
  formatAge(stats.value?.oldest_pending_age_seconds ?? null),
);
const sentTotal = computed(
  () => stats.value?.by_status.find((s) => s.status === 'sent')?.count ?? 0,
);
const failedTotal = computed(
  () => stats.value?.by_status.find((s) => s.status === 'failed')?.count ?? 0,
);
const deadTotal = computed(
  () => stats.value?.by_status.find((s) => s.status === 'dead')?.count ?? 0,
);

watch(
  [statusFilter, ticketFilter, domainFilter],
  () => {
    void loadFirstPage();
  },
  { flush: 'post' },
);

onMounted(async () => {
  await Promise.all([loadFirstPage(), loadStats()]);
});
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header class="flex flex-col gap-2">
      <h1 class="text-2xl font-semibold">{{ $t('admin-email-queue-title') }}</h1>
      <p class="text-sm text-secondary">
        {{ $t('admin-email-queue-description') }}
      </p>
    </header>

    <section class="grid grid-cols-2 md:grid-cols-4 gap-3">
      <div class="rounded border border-default bg-surface p-3">
        <div class="text-xs text-secondary uppercase tracking-wide">{{ $t('admin-email-queue-stat-pending') }}</div>
        <div class="text-2xl font-semibold mt-1">{{ pendingTotal }}</div>
        <div class="text-xs text-secondary mt-1">{{ t('admin-email-queue-stat-oldest', { age: oldestPendingAge }) }}</div>
      </div>
      <div class="rounded border border-default bg-surface p-3">
        <div class="text-xs text-secondary uppercase tracking-wide">{{ $t('admin-email-queue-stat-sent') }}</div>
        <div class="text-2xl font-semibold mt-1">{{ sentTotal }}</div>
      </div>
      <div class="rounded border border-default bg-surface p-3">
        <div class="text-xs text-secondary uppercase tracking-wide">{{ $t('admin-email-queue-stat-failed') }}</div>
        <div class="text-2xl font-semibold mt-1">{{ failedTotal }}</div>
      </div>
      <div class="rounded border border-default bg-surface p-3">
        <div class="text-xs text-secondary uppercase tracking-wide">{{ $t('admin-email-queue-stat-dead') }}</div>
        <div class="text-2xl font-semibold mt-1">{{ deadTotal }}</div>
      </div>
    </section>

    <section class="flex flex-wrap gap-3 items-end">
      <div class="flex flex-col gap-1 text-xs text-secondary">
        <span>{{ $t('admin-email-queue-filter-status') }}</span>
        <div class="flex flex-wrap gap-1">
          <button
            v-for="s in STATUS_OPTIONS"
            :key="s"
            type="button"
            class="px-2 py-1 text-xs rounded border"
            :class="
              statusFilter.has(s)
                ? 'border-accent bg-accent/10 text-accent'
                : 'border-default text-primary hover:bg-hover'
            "
            @click="toggleStatus(s)"
          >
            {{ statusLabel(s) }}
          </button>
        </div>
      </div>
      <label class="flex flex-col gap-1 text-xs text-secondary">
        <span>{{ $t('admin-email-queue-filter-ticket') }}</span>
        <input
          v-model="ticketFilter"
          type="text"
          :placeholder="$t('admin-email-queue-filter-ticket-placeholder')"
          class="h-9 px-2 rounded border border-default bg-input text-primary text-sm w-24"
        />
      </label>
      <label class="flex flex-col gap-1 text-xs text-secondary">
        <span>{{ $t('admin-email-queue-filter-domain') }}</span>
        <input
          v-model="domainFilter"
          type="text"
          :placeholder="$t('admin-email-queue-filter-domain-placeholder')"
          class="h-9 px-2 rounded border border-default bg-input text-primary text-sm w-56"
        />
      </label>
      <button
        v-if="hasFilters"
        type="button"
        class="h-9 px-3 rounded border border-default text-sm hover:bg-hover"
        @click="
          statusFilter = new Set();
          ticketFilter = '';
          domainFilter = '';
        "
      >
        {{ $t('admin-email-queue-clear-filters') }}
      </button>
    </section>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <div v-if="isLoading" class="py-12 flex justify-center">
      <LoadingSpinner />
    </div>

    <EmptyState
      v-else-if="rows.length === 0"
      icon="inbox"
      :title="$t('admin-email-queue-empty-title')"
      :description="$t('admin-email-queue-empty-description')"
    />

    <ul v-else class="flex flex-col gap-1">
      <li
        v-for="row in rows"
        :key="row.id"
        class="rounded border border-default bg-surface"
      >
        <div class="flex items-center gap-3 px-3 py-2">
          <span
            class="text-xs font-medium px-2 py-0.5 rounded"
            :class="statusTone(row.status)"
          >
            {{ statusLabel(row.status) }}
          </span>
          <!-- Bounce badge sits beside the status badge. Bounced
               rows are usually status=sent (SMTP relay accepted,
               remote MTA rejected later via DSN); the status pill
               alone would lie about delivery success. Title carries
               the upstream diagnostic so a hover reveals "why". -->
          <span
            v-if="row.bounced_at"
            class="text-[10px] font-semibold uppercase tracking-wide px-2 py-0.5 rounded bg-red-500/10 text-red-700 dark:text-red-400"
            :title="row.bounce_diagnostic
              ? t('admin-email-queue-bounced-with-diagnostic', { diagnostic: row.bounce_diagnostic })
              : t('admin-email-queue-bounced-no-diagnostic')"
          >
            {{ $t('admin-email-queue-bounced') }}
          </span>
          <span class="text-sm text-primary truncate" :title="row.subject">
            {{ row.subject }}
          </span>
          <span class="text-sm text-secondary truncate flex-1" :title="row.recipient">
            → {{ recipientDomain(row.recipient) }}
          </span>
          <span class="text-xs text-secondary whitespace-nowrap">
            {{ formatDateTime(row.created_at) }}
          </span>
          <span
            v-if="row.attempts > 0"
            class="text-xs text-secondary whitespace-nowrap"
            :title="t('admin-email-queue-attempts-title', { count: row.attempts })"
          >
            {{ row.attempts }}×
          </span>
          <button
            v-if="['failed', 'dead', 'suppressed'].includes(row.status)"
            type="button"
            class="text-xs px-2 py-1 rounded border border-default hover:bg-hover"
            @click="retryNow(row.id)"
          >
            {{ $t('admin-email-queue-retry-now') }}
          </button>
          <button
            v-if="['pending', 'failed'].includes(row.status)"
            type="button"
            class="text-xs px-2 py-1 rounded border border-default hover:bg-hover"
            @click="cancelRow(row.id)"
          >
            {{ $t('admin-email-queue-cancel') }}
          </button>
          <button
            type="button"
            class="text-xs px-2 py-1 rounded text-secondary hover:bg-hover"
            @click="toggleExpanded(row.id)"
          >
            {{ expanded[row.id] ? $t('admin-email-queue-hide') : $t('admin-email-queue-details') }}
          </button>
        </div>
        <div v-if="expanded[row.id]" class="px-3 pb-3 text-sm">
          <dl class="grid grid-cols-[120px_1fr] gap-y-1 gap-x-3 text-xs">
            <dt class="text-secondary">{{ $t('admin-email-queue-field-recipient') }}</dt>
            <dd class="font-mono">{{ row.recipient }}</dd>
            <dt class="text-secondary">{{ $t('admin-email-queue-field-channel') }}</dt>
            <dd class="font-mono">{{ row.channel_id }}</dd>
            <dt v-if="row.ticket_id" class="text-secondary">{{ $t('admin-email-queue-field-ticket') }}</dt>
            <dd v-if="row.ticket_id" class="font-mono">#{{ row.ticket_id }}</dd>
            <dt v-if="row.comment_id" class="text-secondary">{{ $t('admin-email-queue-field-comment') }}</dt>
            <dd v-if="row.comment_id" class="font-mono">#{{ row.comment_id }}</dd>
            <dt class="text-secondary">{{ $t('admin-email-queue-field-next-attempt') }}</dt>
            <dd>{{ formatDateTime(row.next_attempt_at) }}</dd>
            <dt v-if="row.sent_at" class="text-secondary">{{ $t('admin-email-queue-field-sent-at') }}</dt>
            <dd v-if="row.sent_at">{{ formatDateTime(row.sent_at) }}</dd>
            <dt v-if="row.failed_at" class="text-secondary">{{ $t('admin-email-queue-field-failed-at') }}</dt>
            <dd v-if="row.failed_at">{{ formatDateTime(row.failed_at) }}</dd>
            <dt v-if="row.last_smtp_code" class="text-secondary">{{ $t('admin-email-queue-field-smtp-code') }}</dt>
            <dd v-if="row.last_smtp_code" class="font-mono">{{ row.last_smtp_code }}</dd>
            <dt v-if="row.last_error" class="text-secondary">{{ $t('admin-email-queue-field-last-error') }}</dt>
            <dd v-if="row.last_error" class="font-mono text-amber-700 dark:text-amber-400">
              {{ row.last_error }}
            </dd>
            <dt v-if="row.bounced_at" class="text-secondary">{{ $t('admin-email-queue-field-bounced-at') }}</dt>
            <dd v-if="row.bounced_at">{{ formatDateTime(row.bounced_at) }}</dd>
            <dt v-if="row.bounce_recipient" class="text-secondary">{{ $t('admin-email-queue-field-bounce-recipient') }}</dt>
            <dd v-if="row.bounce_recipient" class="font-mono">{{ row.bounce_recipient }}</dd>
            <dt v-if="row.bounce_diagnostic" class="text-secondary">{{ $t('admin-email-queue-field-bounce-reason') }}</dt>
            <dd v-if="row.bounce_diagnostic" class="font-mono text-red-700 dark:text-red-400">
              {{ row.bounce_diagnostic }}
            </dd>
          </dl>
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
        {{ isLoadingMore ? $t('admin-email-queue-loading-more') : $t('admin-email-queue-load-more') }}
      </button>
    </div>

    <ConfirmModal
      :show="showCancelConfirm"
      variant="danger"
      :title="$t('admin-email-queue-confirm-title')"
      :message="$t('admin-email-queue-confirm-message')"
      :confirm-label="$t('admin-email-queue-confirm-yes')"
      :cancel-label="$t('admin-email-queue-confirm-no')"
      @confirm="confirmCancel"
      @close="showCancelConfirm = false"
    />
  </div>
</template>
