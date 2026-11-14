<script setup lang="ts">
/**
 * Admin conflicts inspector for rules: pairs of automated rules where one
 * rule's steps change a field another rule's conditions check, and pairs
 * that ran on the same tickets lately. Only live and dry run rules that run
 * on their own are checked; manual, draft and paused rules are left out.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import { numberForTicketId } from '@/composables/useTicketNumberLookup';
import { ticketPathForId } from '@/utils/ticketNumbers';
import rulesService from '@nosdesk/core/services/rulesService';
import type { RuleConflictRule, RuleConflicts } from '@nosdesk/core/types/rule';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const days = ref<number>(7);
const CONFLICTS_KEY = computed(() => ['rule-conflicts', days.value] as const);
const conflictsQuery = useQuery({
  key: CONFLICTS_KEY,
  query: () => rulesService.conflicts(days.value),
  // Keep the last answer on screen while a new window loads.
  placeholderData: (previous) => previous,
});

const conflicts = computed<RuleConflicts | undefined>(() => conflictsQuery.data.value);
const loadError = computed(() =>
  conflictsQuery.error.value ? t('admin-rules-conflicts-error-load') : '',
);

const rulesById = computed(
  () => new Map((conflicts.value?.rules ?? []).map((r) => [r.id, r] as const)),
);
const ruleOf = (id: number): RuleConflictRule | undefined => rulesById.value.get(id);
const ruleName = (id: number) =>
  ruleOf(id)?.name ?? t('admin-rules-activity-rule-unknown', { id });

/** The `ticket.*` keys a rule writes, as the words an admin knows. Status
 *  and its category read as one field. */
const FIELD_KEYS: Record<string, string> = {
  'ticket.workflow_state_id': 'admin-rules-conflicts-field-status',
  'ticket.workflow_state.category': 'admin-rules-conflicts-field-status',
  'ticket.priority': 'admin-rules-conflicts-field-priority',
  'ticket.assignee_uuid': 'admin-rules-conflicts-field-assignee',
  'ticket.tag_ids': 'admin-rules-conflicts-field-tags',
  'ticket.updated_at': 'admin-rules-conflicts-field-updated',
  'ticket.resolved_at': 'admin-rules-conflicts-field-resolved',
  'ticket.first_response_at': 'admin-rules-conflicts-field-first-response',
  'ticket.comments': 'admin-rules-conflicts-field-replies',
};

const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined;
const list = (items: string[]) =>
  typeof Intl.ListFormat === 'function'
    ? new Intl.ListFormat(lang, { type: 'conjunction' }).format(items)
    : items.join(', ');

function fieldsText(fields: string[]): string {
  const labels = new Set(fields.map((f) => (FIELD_KEYS[f] ? t(FIELD_KEYS[f]) : f)));
  return list([...labels]);
}

const ticketLabel = (id: number) => `#${numberForTicketId(id) ?? id}`;

const windowOptions = computed(() =>
  [7, 30, 90].map((n) => ({ value: String(n), label: t('admin-rules-conflicts-window', { days: n }) })),
);
// days is numeric; BaseDropdown is string-valued, so bridge it.
const daysModel = computed<string>({
  get: () => String(days.value),
  set: (v) => {
    days.value = Number(v);
  },
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="flex flex-col gap-1">
        <BackButton fallback-route="/admin/rules" :label="t('admin-rules-conflicts-back')" compact />
        <h1 class="text-xl sm:text-2xl font-bold text-primary">
          {{ t('admin-rules-conflicts-title') }}
        </h1>
        <p class="text-secondary text-sm sm:text-base max-w-2xl">
          {{ t('admin-rules-conflicts-help') }}
        </p>
      </div>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />

      <template v-if="conflicts">
        <EmptyState
          v-if="conflicts.rules.length === 0"
          :title="t('admin-rules-conflicts-none-checked-title')"
          :hint="t('admin-rules-conflicts-none-checked-hint')"
        />

        <template v-else>
          <section class="flex flex-col gap-2" data-testid="rule-overlaps">
            <h2 class="text-base font-semibold text-primary">
              {{ t('admin-rules-conflicts-overlaps-title') }}
            </h2>
            <p v-if="conflicts.overlaps.length === 0" class="text-sm text-secondary">
              {{ t('admin-rules-conflicts-overlaps-empty') }}
            </p>
            <ul v-else class="flex flex-col gap-2">
              <li
                v-for="pair in conflicts.overlaps"
                :key="`${pair.writer_rule_id}-${pair.reader_rule_id}`"
                class="flex flex-col gap-1 border border-default rounded-lg bg-surface px-4 py-3"
              >
                <span class="flex flex-wrap items-center gap-2 text-sm font-medium">
                  <RouterLink
                    :to="{ name: 'admin-rules-edit', params: { id: pair.writer_rule_id } }"
                    class="text-accent hover:underline"
                  >
                    {{ ruleName(pair.writer_rule_id) }}
                  </RouterLink>
                  <StatusPill
                    v-if="ruleOf(pair.writer_rule_id)?.state === 'dry_run'"
                    :label="t('admin-rules-state-dry-run')"
                    tone="info"
                  />
                  <Icon name="arrowRight" class="w-3.5 h-3.5 text-secondary" />
                  <RouterLink
                    :to="{ name: 'admin-rules-edit', params: { id: pair.reader_rule_id } }"
                    class="text-accent hover:underline"
                  >
                    {{ ruleName(pair.reader_rule_id) }}
                  </RouterLink>
                  <StatusPill
                    v-if="ruleOf(pair.reader_rule_id)?.state === 'dry_run'"
                    :label="t('admin-rules-state-dry-run')"
                    tone="info"
                  />
                </span>
                <span class="text-sm text-secondary">
                  {{
                    t('admin-rules-conflicts-overlap-detail', {
                      writer: ruleName(pair.writer_rule_id),
                      reader: ruleName(pair.reader_rule_id),
                      fields: fieldsText(pair.fields),
                    })
                  }}
                </span>
              </li>
            </ul>
          </section>

          <section class="flex flex-col gap-2" data-testid="rule-co-fires">
            <div class="flex flex-wrap items-center justify-between gap-3">
              <h2 class="text-base font-semibold text-primary">
                {{ t('admin-rules-conflicts-co-fires-title') }}
              </h2>
              <BaseDropdown
                :model-value="daysModel"
                :options="windowOptions"
                size="sm"
                @update:model-value="daysModel = String($event)"
              />
            </div>
            <p v-if="conflicts.co_fires.length === 0" class="text-sm text-secondary">
              {{ t('admin-rules-conflicts-co-fires-empty') }}
            </p>
            <ul v-else class="flex flex-col gap-2">
              <li
                v-for="pair in conflicts.co_fires"
                :key="`${pair.first_rule_id}-${pair.second_rule_id}`"
                class="flex flex-col gap-1 border border-default rounded-lg bg-surface px-4 py-3"
              >
                <span class="flex flex-wrap items-center gap-2 text-sm font-medium">
                  <RouterLink
                    :to="{ name: 'admin-rules-edit', params: { id: pair.first_rule_id } }"
                    class="text-accent hover:underline"
                  >
                    {{ ruleName(pair.first_rule_id) }}
                  </RouterLink>
                  <span class="text-secondary font-normal">{{ t('admin-rules-conflicts-and') }}</span>
                  <RouterLink
                    :to="{ name: 'admin-rules-edit', params: { id: pair.second_rule_id } }"
                    class="text-accent hover:underline"
                  >
                    {{ ruleName(pair.second_rule_id) }}
                  </RouterLink>
                </span>
                <span class="text-sm text-secondary">
                  {{
                    t('admin-rules-conflicts-co-fire-detail', {
                      count: pair.ticket_count,
                      when: formatRelativeTime(pair.last_co_fired_at),
                    })
                  }}
                </span>
                <span class="flex flex-wrap items-center gap-3 text-sm">
                  <RouterLink
                    v-for="ticketId in pair.sample_ticket_ids"
                    :key="ticketId"
                    :to="ticketPathForId(ticketId)"
                    class="text-accent hover:underline"
                  >
                    {{ ticketLabel(ticketId) }}
                  </RouterLink>
                </span>
              </li>
            </ul>
          </section>
        </template>
      </template>
    </div>
  </div>
</template>
