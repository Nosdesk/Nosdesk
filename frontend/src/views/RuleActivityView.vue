<script setup lang="ts">
/**
 * Admin activity-log view for the rules engine. Lists recent
 * rule_applications across the workspace with filters by rule,
 * status, and date range. The inspector tab on each row reveals
 * the condition_evaluation / actions_taken / actions_skipped /
 * failure_reason payloads the backend captures for non-succeeded
 * fires (succeeded rows stay tight per plan §4.3 to keep the
 * happy-path retention case cheap).
 *
 * Phase 1 is read-only; clicking a row opens the per-application
 * inspector inline. Wave 2's conflicts tab adds heuristic
 * warnings on top of this same dataset.
 */
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import rulesService from '@/services/rulesService';
import type { RuleApplication, RuleApplicationStatus } from '@nosdesk/core/types/rule';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();

const statusFilter = ref<RuleApplicationStatus | 'all'>('all');
const limit = ref<number>(50);

/** Query key includes the filter so changing it triggers a refetch
 *  via Pinia Colada's reactivity. */
const APPLICATIONS_KEY = computed(
  () => ['rule-applications', statusFilter.value, limit.value] as const,
);
const applicationsQuery = useQuery({
  key: APPLICATIONS_KEY,
  query: () =>
    rulesService.listApplications({
      status: statusFilter.value === 'all' ? undefined : statusFilter.value,
      limit: limit.value,
    }),
});

const applications = computed<RuleApplication[]>(() =>
  Array.isArray(applicationsQuery.data.value) ? applicationsQuery.data.value : [],
);
const isFirstLoad = computed(
  () =>
    applicationsQuery.status.value === 'pending' &&
    applicationsQuery.data.value === undefined,
);
const loadError = computed(() =>
  applicationsQuery.error.value ? t('admin-rules-activity-error-load') : '',
);

function statusLabel(status: RuleApplicationStatus): string {
  return t(`admin-rules-activity-status-${status.replace(/_/g, '-')}`);
}

const statusFilterOptions = computed(() => [
  { value: 'all', label: t('admin-rules-activity-filter-all') },
  { value: 'succeeded', label: statusLabel('succeeded') },
  { value: 'dry_run', label: statusLabel('dry_run') },
  { value: 'failed', label: statusLabel('failed') },
  { value: 'suppressed_recursion_budget', label: statusLabel('suppressed_recursion_budget') },
  { value: 'suppressed_loop_guard', label: statusLabel('suppressed_loop_guard') },
  { value: 'skipped_condition_unmet', label: statusLabel('skipped_condition_unmet') },
  { value: 'skipped_preflight', label: statusLabel('skipped_preflight') },
]);
const limitOptions = computed(() =>
  [25, 50, 100, 500].map((n) => ({ value: String(n), label: t('admin-rules-activity-limit', { n }) })),
);
// limit is numeric; BaseDropdown is string-valued, so bridge it.
const limitModel = computed<string>({
  get: () => String(limit.value),
  set: (v) => {
    limit.value = Number(v);
  },
});

function statusVariant(status: RuleApplicationStatus): string {
  if (status === 'succeeded') return 'bg-success/10 text-success';
  if (status === 'dry_run') return 'bg-info/10 text-info';
  if (status === 'failed') return 'bg-error/10 text-error';
  return 'bg-warning/10 text-warning';
}

function formatTime(value: string): string {
  try {
    return formatDistanceToNow(new Date(value), { addSuffix: true });
  } catch {
    return value;
  }
}

const expanded = ref<Set<number>>(new Set());
function toggleExpanded(id: number): void {
  if (expanded.value.has(id)) {
    expanded.value.delete(id);
  } else {
    expanded.value.add(id);
  }
}

function inspectorPayload(app: RuleApplication): string {
  /** The four payloads are stored sparsely (only on non-succeeded
   *  rows, see migration / plan). Pretty-print whichever are
   *  present so the inspector reads correctly. */
  const blobs: Record<string, unknown> = {};
  if (app.condition_evaluation) blobs.condition_evaluation = app.condition_evaluation;
  if (app.actions_taken) blobs.actions_taken = app.actions_taken;
  if (app.actions_skipped) blobs.actions_skipped = app.actions_skipped;
  if (app.failure_reason) blobs.failure_reason = app.failure_reason;
  if (Object.keys(blobs).length === 0) return t('admin-rules-activity-inspector-empty');
  return JSON.stringify(blobs, null, 2);
}

function back(): void {
  router.push({ name: 'admin-rules' });
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex flex-wrap items-center gap-3">
      <Button variant="secondary" size="sm" @click="back">
        <Icon name="chevronLeft" class="w-4 h-4" />
        <span>{{ t('admin-rules-activity-back') }}</span>
      </Button>
      <h1 class="text-2xl font-semibold flex-1 min-w-0">
        {{ t('admin-rules-activity-title') }}
      </h1>
    </div>

    <p class="text-sm text-secondary max-w-2xl">
      {{ t('admin-rules-activity-help') }}
    </p>

    <AlertMessage v-if="loadError" type="error" :message="loadError" />

    <div class="flex flex-wrap items-center gap-3">
      <BaseDropdown
        :model-value="statusFilter"
        :options="statusFilterOptions"
        size="sm"
        @update:model-value="statusFilter = String($event) as RuleApplicationStatus | 'all'"
      />
      <BaseDropdown
        :model-value="limitModel"
        :options="limitOptions"
        size="sm"
        @update:model-value="limitModel = String($event)"
      />
    </div>

    <Skeleton v-if="isFirstLoad" class="flex flex-col gap-2">
      <SkeletonBar v-for="i in 6" :key="i" class="h-10 w-full" />
    </Skeleton>

    <EmptyState
      v-else-if="applications.length === 0"
      :title="t('admin-rules-activity-empty-title')"
      :hint="t('admin-rules-activity-empty-hint')"
    />

    <ul v-else class="flex flex-col gap-2">
      <li
        v-for="app in applications"
        :key="app.id"
        class="border rounded-md bg-surface"
      >
        <div
          class="flex items-center gap-3 px-3 py-2 cursor-pointer hover:bg-surface-hover"
          @click="toggleExpanded(app.id)"
        >
          <span
            :class="[
              'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium',
              statusVariant(app.status),
            ]"
          >
            {{ statusLabel(app.status) }}
          </span>
          <span class="font-mono text-xs text-secondary">
            #{{ app.rule_id }} v{{ app.rule_version }}
          </span>
          <span class="text-sm flex-1 min-w-0 truncate">
            {{ t('admin-rules-activity-row-summary', {
              ticket_id: app.ticket_id,
              actor: app.actor_kind === 'system'
                ? t('admin-rules-activity-actor-system')
                : t('admin-rules-activity-actor-user'),
            }) }}
          </span>
          <span class="text-xs text-secondary">{{ formatTime(app.applied_at) }}</span>
          <Icon
            :name="expanded.has(app.id) ? 'chevronUp' : 'chevronDown'"
            class="w-3.5 h-3.5 text-secondary"
          />
        </div>
        <pre
          v-if="expanded.has(app.id)"
          class="text-xs px-3 py-2 border-t bg-surface-hover overflow-x-auto"
        >{{ inspectorPayload(app) }}</pre>
      </li>
    </ul>
  </div>
</template>
