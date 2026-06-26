<script setup lang="ts">
/**
 * Admin rule editor. Creates a new rule when route name is
 * `admin-rules-new`, edits an existing rule by id when route name
 * is `admin-rules-edit`. Manual rules (the Phase 1 trigger kind)
 * have `conditions = []` enforced by the backend; the editor
 * hides the conditions section for manual rules.
 *
 * The action list is the main interaction surface. Each action
 * carries a typed `kind` plus a kind-specific config object. The
 * supported kinds for Phase 1 are reply / set_status / assign /
 * unassign / add_tags / remove_tags / set_priority /
 * stop_processing; notify / apply_macro_template / webhook are
 * deferred and rejected by the backend with RULE_ACTION_UNSUPPORTED.
 */
import { computed, onMounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Icon from '@/components/common/Icon.vue';
import rulesService from '@/services/rulesService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';
import type {
  CreateRuleRequest,
  Rule,
  RuleAction,
  RuleTriggerKind,
  UpdateRuleRequest,
} from '@nosdesk/core/types/rule';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const route = useRoute();
const router = useRouter();
const toast = useToastStore();
const queryCache = useQueryCache();

const isNew = computed(() => route.name === 'admin-rules-new');
const ruleId = computed<number | null>(() =>
  isNew.value ? null : Number(route.params.id) || null,
);

const loading = ref(false);
const saving = ref(false);
const errorMessage = ref('');
const original = ref<Rule | null>(null);

const name = ref('');
const description = ref('');
const triggerKind = ref<RuleTriggerKind>('manual');
const priority = ref<number>(100);
const actions = ref<RuleAction[]>([{ kind: 'reply', config: { visibility: 'public', body: '' } }]);
const overrideSelfRef = ref(false);

onMounted(async () => {
  if (isNew.value) return;
  if (ruleId.value == null) return;
  loading.value = true;
  try {
    const rule = await rulesService.get(ruleId.value);
    original.value = rule;
    name.value = rule.name;
    description.value = rule.description ?? '';
    triggerKind.value = rule.trigger_kind;
    priority.value = rule.priority;
    actions.value = Array.isArray(rule.actions) ? [...rule.actions] : [];
  } catch (err) {
    errorMessage.value = extractErrorMessage(err, t('admin-rule-editor-error-save'));
  } finally {
    loading.value = false;
  }
});

const headerTitle = computed(() =>
  isNew.value
    ? t('admin-rule-editor-title-new')
    : t('admin-rule-editor-title-edit', { name: original.value?.name ?? '' }),
);

function addAction(): void {
  actions.value.push({ kind: 'reply', config: { visibility: 'public', body: '' } });
}

function removeAction(index: number): void {
  actions.value.splice(index, 1);
}

function setActionKind(index: number, kind: RuleAction['kind']): void {
  // Reset config when the kind changes so stale fields from the
  // previous shape don't leak through to the save request. Each
  // kind gets a minimal default; the backend's per-kind validator
  // catches anything stricter.
  const defaultConfig = (k: RuleAction['kind']): Record<string, unknown> => {
    switch (k) {
      case 'reply':
        return { visibility: 'public', body: '' };
      case 'set_status':
        return { workflow_state_id: 0 };
      case 'assign':
        return { method: 'direct', user_uuid: '' };
      case 'unassign':
        return {};
      case 'add_tags':
      case 'remove_tags':
        return { tag_ids: [] };
      case 'set_priority':
        return { priority: 'normal' };
      case 'stop_processing':
        return {};
      default:
        return {};
    }
  };
  actions.value[index] = { kind, config: defaultConfig(kind) };
}

function updateConfigField(index: number, field: string, value: unknown): void {
  const next = { ...(actions.value[index].config ?? {}) } as Record<string, unknown>;
  next[field] = value;
  actions.value[index] = { ...actions.value[index], config: next };
}

const canSave = computed(
  () => name.value.trim().length > 0 && actions.value.length > 0 && !saving.value,
);

async function save(): Promise<void> {
  errorMessage.value = '';
  if (!canSave.value) return;
  saving.value = true;
  try {
    const payload: CreateRuleRequest = {
      name: name.value.trim(),
      description: description.value.trim() || null,
      trigger_kind: triggerKind.value,
      conditions: [],
      actions: actions.value,
      priority: priority.value,
      override_self_reference: overrideSelfRef.value,
    };
    if (isNew.value) {
      const created = await rulesService.create(payload);
      await queryCache.invalidateQueries({ key: ['rules'] });
      toast.success(t('admin-rules-toast-archived', { name: created.name }));
      router.push({ name: 'admin-rules-edit', params: { id: created.id } });
    } else if (ruleId.value != null) {
      const update: UpdateRuleRequest = {
        name: payload.name,
        description: payload.description,
        trigger_kind: payload.trigger_kind,
        actions: payload.actions,
        priority: payload.priority,
        override_self_reference: overrideSelfRef.value,
      };
      await rulesService.update(ruleId.value, update);
      await queryCache.invalidateQueries({ key: ['rules'] });
      toast.success(t('admin-rules-toast-state-changed', { state: name.value }));
    }
  } catch (err) {
    errorMessage.value = extractErrorMessage(err, t('admin-rule-editor-error-save'));
  } finally {
    saving.value = false;
  }
}

function back(): void {
  router.push({ name: 'admin-rules' });
}

const triggerKinds: RuleTriggerKind[] = [
  'manual',
  'ticket_created',
  'ticket_updated',
  'ticket_replied',
  'time_elapsed',
];
const actionKinds: RuleAction['kind'][] = [
  'reply',
  'set_status',
  'assign',
  'unassign',
  'add_tags',
  'remove_tags',
  'set_priority',
  'stop_processing',
];

function triggerLabel(kind: RuleTriggerKind): string {
  return t(`admin-rules-trigger-${kind.replace(/_/g, '-')}`);
}

function actionLabel(kind: RuleAction['kind']): string {
  const map: Record<RuleAction['kind'], string> = {
    reply: 'Reply',
    set_status: 'Set status',
    assign: 'Assign',
    unassign: 'Unassign',
    add_tags: 'Add tags',
    remove_tags: 'Remove tags',
    set_priority: 'Set priority',
    notify: 'Notify',
    apply_macro_template: 'Apply template',
    webhook: 'Webhook',
    stop_processing: 'Stop processing',
  };
  return map[kind] ?? kind;
}

// BaseDropdown option lists (value/label) for the enum selects.
const triggerOptions = computed(() =>
  triggerKinds.map((k) => ({ value: k, label: triggerLabel(k) })),
);
const actionOptions = computed(() =>
  actionKinds.map((k) => ({ value: k, label: actionLabel(k) })),
);
const replyVisibilityOptions = computed(() => [
  { value: 'public', label: t('admin-rules-action-chip-reply-public') },
  { value: 'internal', label: t('admin-rules-action-chip-reply-internal') },
]);
const priorityOptions = [
  { value: 'low', label: 'Low' },
  { value: 'normal', label: 'Normal' },
  { value: 'high', label: 'High' },
  { value: 'urgent', label: 'Urgent' },
];
</script>

<template>
  <div class="flex flex-col gap-6 max-w-3xl">
    <div class="flex items-center gap-3">
      <Button variant="secondary" size="sm" @click="back">
        <Icon name="chevronLeft" class="w-4 h-4" />
        <span>{{ t('admin-rule-editor-back') }}</span>
      </Button>
      <h1 class="text-2xl font-semibold flex-1 min-w-0 truncate">
        {{ headerTitle }}
      </h1>
      <Button variant="primary" :disabled="!canSave" @click="save">
        {{ saving ? t('admin-rule-editor-saving') : t('admin-rule-editor-save') }}
      </Button>
    </div>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <section class="flex flex-col gap-3">
      <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">
        {{ t('admin-rule-editor-section-name') }}
      </h2>
      <FormInput
        v-model="name"
        :label="t('admin-rule-editor-name-label')"
        :placeholder="t('admin-rule-editor-name-placeholder')"
        required
      />
      <FormTextarea
        v-model="description"
        :label="t('admin-rule-editor-description-label')"
        :placeholder="t('admin-rule-editor-description-placeholder')"
        :rows="2"
      />
    </section>

    <section class="flex flex-col gap-3">
      <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">
        {{ t('admin-rule-editor-section-trigger') }}
      </h2>
      <label class="block text-sm font-medium">{{ t('admin-rule-editor-trigger-label') }}</label>
      <BaseDropdown
        :model-value="triggerKind"
        :options="triggerOptions"
        size="sm"
        @update:model-value="triggerKind = String($event) as RuleTriggerKind"
      />
      <p v-if="triggerKind === 'manual'" class="text-xs text-secondary">
        {{ t('admin-rule-editor-trigger-manual-note') }}
      </p>
      <p v-else class="text-xs text-warning">
        {{ t('admin-rule-editor-trigger-other-phase') }}
      </p>
    </section>

    <section class="flex flex-col gap-3">
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">
          {{ t('admin-rule-editor-section-actions') }}
        </h2>
        <Button variant="ghost" size="sm" @click="addAction">
          <Icon name="add" class="w-4 h-4" />
          <span>{{ t('admin-rule-editor-actions-add') }}</span>
        </Button>
      </div>

      <p v-if="actions.length === 0" class="text-sm text-warning">
        {{ t('admin-rule-editor-actions-empty') }}
      </p>

      <ol class="flex flex-col gap-2">
        <li
          v-for="(action, i) in actions"
          :key="i"
          class="border rounded-md p-3 flex flex-col gap-2 bg-surface"
        >
          <div class="flex items-center gap-2">
            <span class="text-xs text-secondary font-mono">#{{ i + 1 }}</span>
            <BaseDropdown
              :model-value="action.kind"
              :options="actionOptions"
              size="sm"
              class="flex-1"
              @update:model-value="setActionKind(i, String($event) as RuleAction['kind'])"
            />
            <Button variant="ghost" size="sm" @click="removeAction(i)">
              <Icon name="trash" class="w-3.5 h-3.5" />
              <span class="sr-only">{{ t('admin-rule-editor-action-remove') }}</span>
            </Button>
          </div>

          <!-- Per-kind config form. Kept inline so the editor stays
               a single component for the Phase 1 surface; if it
               grows past a screen each kind gets its own card. -->
          <template v-if="action.kind === 'reply'">
            <BaseDropdown
              :model-value="(action.config as any)?.visibility ?? 'public'"
              :options="replyVisibilityOptions"
              size="sm"
              @update:model-value="updateConfigField(i, 'visibility', String($event))"
            />
            <FormTextarea
              :model-value="(action.config as any)?.body ?? ''"
              @update:model-value="updateConfigField(i, 'body', $event)"
              :rows="3"
              placeholder="Hi {{customer_name}}, ..."
            />
          </template>

          <template v-else-if="action.kind === 'set_status'">
            <FormInput
              :model-value="String((action.config as any)?.workflow_state_id ?? '')"
              @update:model-value="updateConfigField(i, 'workflow_state_id', Number($event))"
              :label="t('admin-rules-action-chip-set-status', { state_id: '' })"
              type="number"
            />
          </template>

          <template v-else-if="action.kind === 'assign'">
            <FormInput
              :model-value="(action.config as any)?.user_uuid ?? ''"
              @update:model-value="updateConfigField(i, 'user_uuid', $event)"
              label="Assign to user UUID"
              placeholder="00000000-0000-0000-0000-000000000000"
            />
          </template>

          <template v-else-if="action.kind === 'set_priority'">
            <BaseDropdown
              :model-value="(action.config as any)?.priority ?? 'normal'"
              :options="priorityOptions"
              size="sm"
              @update:model-value="updateConfigField(i, 'priority', String($event))"
            />
          </template>

          <template v-else-if="action.kind === 'add_tags' || action.kind === 'remove_tags'">
            <FormInput
              :model-value="((action.config as any)?.tag_ids ?? []).join(',')"
              @update:model-value="updateConfigField(i, 'tag_ids',
                $event.split(',').map((x: string) => Number(x.trim())).filter((n: number) => Number.isFinite(n)))"
              label="Tag IDs (comma-separated)"
              placeholder="1, 2, 3"
            />
          </template>
        </li>
      </ol>
    </section>

    <section class="flex flex-col gap-3">
      <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">
        {{ t('admin-rule-editor-section-state') }}
      </h2>
      <FormInput
        :model-value="String(priority)"
        @update:model-value="priority = Number($event) || 100"
        :label="t('admin-rule-editor-priority-label')"
        type="number"
      />
      <Checkbox
        v-model="overrideSelfRef"
        :label="t('admin-rule-editor-override-self-ref')"
      />
    </section>
  </div>
</template>
