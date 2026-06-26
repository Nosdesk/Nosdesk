<script setup lang="ts">
/**
 * Admin list view for the unified rules engine (Phase 1 surface).
 * Lists every rule in the workspace, filtered by trigger kind /
 * state, with edit + archive + state-toggle affordances. Reads use
 * Pinia Colada so navigating away and back renders instantly from
 * cache and revalidates in the background.
 *
 * Cross-link to the activity tab via the `/admin/rules/activity`
 * route; the editor lives at `/admin/rules/:id` (Wave 7).
 */
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import SearchInput from '@/components/common/SearchInput.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import rulesService from '@/services/rulesService';
import { useToastStore } from '@/stores/toast';
import { extractErrorMessage } from '@/utils/errors';
import type { Rule, RuleState, RuleTriggerKind } from '@nosdesk/core/types/rule';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();
const toast = useToastStore();

/** Pinia Colada key shared by the editor (Wave 7) so saves
 *  invalidate the list automatically. */
const RULES_KEY = ['rules'] as const;
const queryCache = useQueryCache();
const rulesQuery = useQuery({
  key: RULES_KEY,
  query: () => rulesService.list({ include_archived: false }),
});

const rules = computed<Rule[]>(() =>
  Array.isArray(rulesQuery.data.value) ? rulesQuery.data.value : [],
);
const isFirstLoad = computed(
  () => rulesQuery.status.value === 'pending' && rulesQuery.data.value === undefined,
);
const loadError = computed(() =>
  rulesQuery.error.value ? t('admin-rules-error-load') : '',
);

const search = ref('');
const triggerFilter = ref<RuleTriggerKind | 'all'>('all');
const stateFilter = ref<RuleState | 'all'>('all');

const filtered = computed<Rule[]>(() => {
  const term = search.value.trim().toLowerCase();
  return rules.value.filter((r) => {
    if (triggerFilter.value !== 'all' && r.trigger_kind !== triggerFilter.value) {
      return false;
    }
    if (stateFilter.value !== 'all' && r.state !== stateFilter.value) {
      return false;
    }
    if (term && !r.name.toLowerCase().includes(term)) {
      return false;
    }
    return true;
  });
});

function formatLastFired(value: string | null): string {
  if (!value) return t('admin-rules-last-fired-never');
  try {
    return formatDistanceToNow(new Date(value), { addSuffix: true });
  } catch {
    return value;
  }
}

function triggerLabel(kind: RuleTriggerKind): string {
  return t(`admin-rules-trigger-${kind.replace(/_/g, '-')}`);
}

function stateLabel(state: RuleState): string {
  return t(`admin-rules-state-${state.replace(/_/g, '-')}`);
}

const triggerFilterOptions = computed(() => [
  { value: 'all', label: t('admin-rules-filter-trigger-all') },
  { value: 'manual', label: triggerLabel('manual') },
  { value: 'ticket_created', label: triggerLabel('ticket_created') },
  { value: 'ticket_updated', label: triggerLabel('ticket_updated') },
  { value: 'ticket_replied', label: triggerLabel('ticket_replied') },
  { value: 'time_elapsed', label: triggerLabel('time_elapsed') },
]);
const stateFilterOptions = computed(() => [
  { value: 'all', label: t('admin-rules-filter-state-all') },
  { value: 'draft', label: stateLabel('draft') },
  { value: 'dry_run', label: stateLabel('dry_run') },
  { value: 'live', label: stateLabel('live') },
]);

const errorMessage = ref('');
const archiveTarget = ref<Rule | null>(null);

async function openEdit(rule: Rule): Promise<void> {
  await router.push({ name: 'admin-rules-edit', params: { id: rule.id } });
}

function openCreate(): void {
  router.push({ name: 'admin-rules-new' });
}

function openActivity(): void {
  router.push({ name: 'admin-rules-activity' });
}

async function archive(rule: Rule): Promise<void> {
  errorMessage.value = '';
  try {
    await rulesService.archive(rule.id);
    await queryCache.invalidateQueries({ key: RULES_KEY });
    toast.success(t('admin-rules-toast-archived', { name: rule.name }));
  } catch (err) {
    errorMessage.value = extractErrorMessage(err, t('admin-rules-error-archive'));
  }
  archiveTarget.value = null;
}

async function toggleLive(rule: Rule): Promise<void> {
  errorMessage.value = '';
  const target: RuleState = rule.state === 'live' ? 'dry_run' : 'live';
  try {
    await rulesService.transitionState(rule.id, { state: target });
    await queryCache.invalidateQueries({ key: RULES_KEY });
    toast.success(t('admin-rules-toast-state-changed', { state: stateLabel(target) }));
  } catch (err) {
    errorMessage.value = extractErrorMessage(err, t('admin-rules-error-transition'));
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex flex-wrap items-center gap-3">
      <h1 class="text-2xl font-semibold flex-1 min-w-0">
        {{ t('admin-rules-title') }}
      </h1>
      <Button variant="secondary" @click="openActivity">
        <Icon name="history" class="w-4 h-4" />
        <span>{{ t('admin-rules-activity-cta') }}</span>
      </Button>
      <Button variant="primary" @click="openCreate">
        <Icon name="add" class="w-4 h-4" />
        <span>{{ t('admin-rules-new-cta') }}</span>
      </Button>
    </div>

    <p class="text-sm text-secondary max-w-2xl">
      {{ t('admin-rules-help-intro') }}
    </p>

    <AlertMessage v-if="loadError" type="error" :message="loadError" />
    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <div class="flex flex-wrap items-center gap-3">
      <SearchInput
        v-model="search"
        :placeholder="t('admin-rules-search-placeholder')"
        class="flex-1 min-w-[12rem]"
      />
      <BaseDropdown
        :model-value="triggerFilter"
        :options="triggerFilterOptions"
        size="sm"
        @update:model-value="triggerFilter = String($event) as RuleTriggerKind | 'all'"
      />
      <BaseDropdown
        :model-value="stateFilter"
        :options="stateFilterOptions"
        size="sm"
        @update:model-value="stateFilter = String($event) as RuleState | 'all'"
      />
    </div>

    <Skeleton v-if="isFirstLoad" class="flex flex-col gap-2">
      <SkeletonBar v-for="i in 6" :key="i" class="h-10 w-full" />
    </Skeleton>

    <EmptyState
      v-else-if="filtered.length === 0"
      :title="t('admin-rules-empty-title')"
      :hint="t('admin-rules-empty-hint')"
    >
      <Button variant="primary" @click="openCreate">
        {{ t('admin-rules-new-cta') }}
      </Button>
    </EmptyState>

    <table v-else class="w-full text-sm">
      <thead>
        <tr class="border-b text-left text-secondary">
          <th class="py-2 font-medium">{{ t('admin-rules-col-name') }}</th>
          <th class="py-2 font-medium">{{ t('admin-rules-col-trigger') }}</th>
          <th class="py-2 font-medium">{{ t('admin-rules-col-state') }}</th>
          <th class="py-2 font-medium">{{ t('admin-rules-col-last-fired') }}</th>
          <th class="py-2 font-medium text-right">{{ t('admin-rules-col-fire-count') }}</th>
          <th class="py-2 font-medium"></th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="rule in filtered"
          :key="rule.id"
          class="border-b hover:bg-surface-hover cursor-pointer"
          @click="openEdit(rule)"
        >
          <td class="py-2">
            <div class="font-medium">{{ rule.name }}</div>
            <div v-if="rule.description" class="text-xs text-secondary truncate max-w-md">
              {{ rule.description }}
            </div>
          </td>
          <td class="py-2 text-secondary">{{ triggerLabel(rule.trigger_kind) }}</td>
          <td class="py-2">
            <span
              :class="[
                'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium',
                rule.state === 'live' ? 'bg-success/10 text-success' : '',
                rule.state === 'dry_run' ? 'bg-warning/10 text-warning' : '',
                rule.state === 'draft' ? 'bg-info/10 text-info' : '',
              ]"
            >
              {{ stateLabel(rule.state) }}
            </span>
          </td>
          <td class="py-2 text-secondary">{{ formatLastFired(rule.last_fired_at) }}</td>
          <td class="py-2 text-right tabular-nums">{{ rule.fire_count }}</td>
          <td class="py-2 text-right" @click.stop>
            <div class="flex items-center justify-end gap-2">
              <Button
                variant="secondary"
                size="sm"
                @click="toggleLive(rule)"
                :title="rule.state === 'live'
                  ? t('admin-rules-action-pause-tooltip')
                  : t('admin-rules-action-resume-tooltip')"
              >
                <Icon
                  :name="rule.state === 'live' ? 'eyeOff' : 'eye'"
                  class="w-3.5 h-3.5"
                />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                @click="archiveTarget = rule"
                :title="t('admin-rules-action-archive-tooltip')"
              >
                <Icon name="trash" class="w-3.5 h-3.5" />
              </Button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>

    <ConfirmModal
      :show="archiveTarget !== null"
      :title="t('admin-rules-archive-confirm-title')"
      :message="archiveTarget ? t('admin-rules-archive-confirm-body', { name: archiveTarget.name }) : ''"
      :confirm-label="t('admin-rules-archive-confirm-button')"
      variant="warning"
      @confirm="archiveTarget && archive(archiveTarget)"
      @cancel="archiveTarget = null"
    />
  </div>
</template>
