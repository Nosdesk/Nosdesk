<script setup lang="ts">
import { numberForTicketId } from '@/composables/useTicketNumberLookup'
/**
 * Admin activity log for rules: each run with the rule's name, the ticket,
 * who applied it and, opened up, what each step did in plain words (with
 * the steps the agent skipped). Filters by rule, by status and how many runs
 * to show. A dry run's row says what the rule would have done; the editor
 * links here filtered to one rule's dry runs (`?rule=<id>&status=dry_run`).
 */
import { computed, ref } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import type { StatusPillTone } from '@/components/common/statusPillTone';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import { useRuleStepText } from '@/composables/useRuleStepText';
import { ticketPathForId } from '@/utils/ticketNumbers';
import rulesService from '@nosdesk/core/services/rulesService';
import BackButton from '@/components/common/BackButton.vue';

// Desktop only: on mobile the leading back-arrow in SiteHeader is the single
// back affordance, so this inline control hides to avoid two per screen. Same
// contract BackButton encodes; kept inline here because this toolbar's
// secondary-button styling is deliberate.
import type { RuleAction, RuleApplication, RuleApplicationStatus } from '@nosdesk/core/types/rule';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const route = useRoute();
const STATUSES: RuleApplicationStatus[] = [
  'succeeded',
  'dry_run',
  'failed',
  'suppressed_recursion_budget',
  'suppressed_loop_guard',
  'skipped_condition_unmet',
  'skipped_preflight',
];
const queryStatus = String(route.query.status ?? '');
const statusFilter = ref<RuleApplicationStatus | 'all'>(
  STATUSES.includes(queryStatus as RuleApplicationStatus) ? (queryStatus as RuleApplicationStatus) : 'all',
);
const queryRule = Number(route.query.rule);
const ruleFilter = ref<number | 'all'>(Number.isInteger(queryRule) && queryRule > 0 ? queryRule : 'all');
const limit = ref<number>(50);

/** Query key includes the filters so changing one triggers a refetch
 *  via Pinia Colada's reactivity. */
const APPLICATIONS_KEY = computed(
  () => ['rule-applications', statusFilter.value, ruleFilter.value, limit.value] as const,
);
const applicationsQuery = useQuery({
  key: APPLICATIONS_KEY,
  query: () =>
    rulesService.listApplications({
      status: statusFilter.value === 'all' ? undefined : statusFilter.value,
      rule_id: ruleFilter.value === 'all' ? undefined : ruleFilter.value,
      limit: limit.value,
    }),
});
// Archived rules too, so older runs still show the rule's name.
const rulesQuery = useQuery({
  key: ['rules', 'with-archived'],
  query: () => rulesService.list({ include_archived: true }),
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

// Names for who applied each run, who its assign steps chose and who its
// notify steps emailed.
const peopleUuids = computed(() => {
  const uuids = new Set<string>();
  for (const app of applications.value) {
    if (app.actor_uuid) uuids.add(app.actor_uuid);
    for (const entry of app.actions_taken ?? []) {
      if (typeof entry.assigned_to_uuid === 'string') uuids.add(entry.assigned_to_uuid);
      if (typeof entry.user_uuid === 'string') uuids.add(entry.user_uuid);
    }
  }
  return [...uuids].sort();
});
const stepText = useRuleStepText({ enabled: () => true, userUuids: () => peopleUuids.value });

const ruleFor = (app: RuleApplication) => (rulesQuery.data.value ?? []).find((r) => r.id === app.rule_id);
const ruleName = (app: RuleApplication) =>
  ruleFor(app)?.name ?? t('admin-rules-activity-rule-unknown', { id: app.rule_id });
/** The rule's step at a run's 1-based position, while the rule still has it. */
const ruleStep = (app: RuleApplication, position: unknown): RuleAction | undefined =>
  ruleFor(app)?.actions[Number(position) - 1];

function summary(app: RuleApplication): string {
  // A string, so Fluent doesn't format the number as "1,042".
  const ticket_id = String(numberForTicketId(app.ticket_id) ?? '');
  if (app.status === 'dry_run') return t('admin-rules-activity-row-dry-run', { ticket_id });
  if (app.actor_kind === 'system') return t('admin-rules-activity-row-automatic', { ticket_id });
  const name = stepText.userName(app.actor_uuid);
  return name
    ? t('admin-rules-activity-row-by-person', { ticket_id, name })
    : t('admin-rules-activity-row-by-agent', { ticket_id });
}

/** What the run did, then the steps the agent skipped or the run held, in
 *  plain words. A rule that runs on its own holds the steps that send email
 *  on a ticket flagged as likely spam, and a time rule's tags after a
 *  warning that wasn't sent. */
function outcomeLines(app: RuleApplication): string[] {
  const lines: string[] = [];
  for (const entry of app.actions_taken ?? []) {
    const line = stepText.done(entry, ruleStep(app, entry.index));
    if (line) lines.push(line);
  }
  for (const entry of app.actions_skipped ?? []) {
    const step = ruleStep(app, entry.index);
    const planned = step ? stepText.planned(step) : null;
    if (!planned) continue;
    lines.push(t(heldKey(entry.reason), { step: planned }));
  }
  return lines;
}

/** The line for a step a run skipped, by why. */
function heldKey(reason: unknown): string {
  if (reason === 'spam_suspected') return 'admin-rules-activity-held-spam';
  if (reason === 'warning_not_sent') return 'admin-rules-activity-held-warning-unsent';
  return 'admin-rules-activity-skipped';
}

function statusLabel(status: RuleApplicationStatus): string {
  return t(`admin-rules-activity-status-${status.replace(/_/g, '-')}`);
}

const statusFilterOptions = computed(() => [
  { value: 'all', label: t('admin-rules-activity-filter-all') },
  ...STATUSES.map((status) => ({ value: status, label: statusLabel(status) })),
]);
const ruleFilterOptions = computed(() => {
  const options: { value: number | 'all'; label: string }[] = [
    { value: 'all', label: t('admin-rules-activity-filter-all-rules') },
    ...(rulesQuery.data.value ?? []).map((r) => ({ value: r.id, label: r.name })),
  ];
  // A rule linked here that the list doesn't have (yet) still shows.
  const chosen = ruleFilter.value;
  if (chosen !== 'all' && !options.some((o) => o.value === chosen)) {
    options.push({ value: chosen, label: t('admin-rules-activity-rule-unknown', { id: chosen }) });
  }
  return options;
});
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

function statusTone(status: RuleApplicationStatus): StatusPillTone {
  if (status === 'succeeded') return 'positive';
  if (status === 'dry_run') return 'info';
  if (status === 'failed') return 'critical';
  return 'caution';
}

const expanded = ref<Set<number>>(new Set());
function toggleExpanded(id: number): void {
  const next = new Set(expanded.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expanded.value = next;
}

</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="flex flex-col gap-1">
        <BackButton fallback-route="/admin/rules" :label="t('admin-rules-activity-back')" compact />
        <h1 class="text-xl sm:text-2xl font-bold text-primary">
          {{ t('admin-rules-activity-title') }}
        </h1>
        <p class="text-secondary text-sm sm:text-base max-w-2xl">
          {{ t('admin-rules-activity-help') }}
        </p>
      </div>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />

      <div class="flex flex-wrap items-center gap-3">
        <BaseDropdown
          :model-value="statusFilter"
          :options="statusFilterOptions"
          size="sm"
          @update:model-value="statusFilter = String($event) as RuleApplicationStatus | 'all'"
        />
        <BaseDropdown
          :model-value="ruleFilter"
          :options="ruleFilterOptions"
          :aria-label="t('admin-rules-activity-filter-rule')"
          size="sm"
          @update:model-value="ruleFilter = $event === 'all' ? 'all' : Number($event)"
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
          class="border border-default rounded-lg bg-surface overflow-hidden"
        >
          <button
            type="button"
            class="w-full flex items-center gap-3 px-4 py-3 text-left hover:bg-surface-hover transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
            :aria-expanded="expanded.has(app.id)"
            @click="toggleExpanded(app.id)"
          >
            <StatusPill :label="statusLabel(app.status)" :tone="statusTone(app.status)" />
            <span class="flex flex-col gap-0.5 flex-1 min-w-0">
              <span class="text-sm font-medium text-primary truncate">{{ ruleName(app) }}</span>
              <span class="text-xs text-secondary truncate">{{ summary(app) }}</span>
            </span>
            <span class="text-xs text-secondary flex-shrink-0">{{ formatRelativeTime(app.applied_at) }}</span>
            <Icon
              :name="expanded.has(app.id) ? 'chevronUp' : 'chevronDown'"
              class="w-3.5 h-3.5 text-secondary flex-shrink-0"
            />
          </button>
          <div
            v-if="expanded.has(app.id)"
            class="flex flex-col gap-3 border-t border-subtle bg-surface-alt px-4 py-3"
          >
            <div class="flex flex-col gap-1">
              <p class="text-xs font-medium text-tertiary">
                {{ app.status === 'dry_run' ? t('admin-rules-activity-what-it-would-do') : t('admin-rules-activity-what-it-did') }}
              </p>
              <ul v-if="outcomeLines(app).length > 0" class="flex flex-col gap-1 pl-4 list-disc text-sm text-primary">
                <li v-for="(line, i) in outcomeLines(app)" :key="i">{{ line }}</li>
              </ul>
              <p v-else class="text-sm text-secondary">{{ t('admin-rules-activity-inspector-empty') }}</p>
              <p v-if="app.failure_reason" class="text-sm text-status-error">{{ app.failure_reason }}</p>
            </div>
            <div class="flex flex-wrap items-center gap-4 text-sm">
              <RouterLink :to="ticketPathForId(app.ticket_id)" class="text-accent hover:underline">
                {{ t('admin-rules-activity-open-ticket') }}
              </RouterLink>
              <RouterLink
                v-if="ruleFor(app)"
                :to="{ name: 'admin-rules-edit', params: { id: app.rule_id } }"
                class="text-accent hover:underline"
              >
                {{ t('admin-rules-activity-open-rule') }}
              </RouterLink>
            </div>
          </div>
        </li>
      </ul>
    </div>
  </div>
</template>
