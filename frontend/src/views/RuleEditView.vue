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
} from '@/types/rule';

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
</script>

<template>
  <div class="space-y-6 max-w-3xl">
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

    <section class="space-y-3">
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

    <section class="space-y-3">
      <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">
        {{ t('admin-rule-editor-section-trigger') }}
      </h2>
      <label class="block text-sm font-medium">{{ t('admin-rule-editor-trigger-label') }}</label>
      <select
        v-model="triggerKind"
        class="w-full border rounded-md px-3 py-2 text-sm bg-surface"
      >
        <option v-for="k in triggerKinds" :key="k" :value="k">
          {{ triggerLabel(k) }}
        </option>
      </select>
      <p v-if="triggerKind === 'manual'" class="text-xs text-secondary">
        {{ t('admin-rule-editor-trigger-manual-note') }}
      </p>
      <p v-else class="text-xs text-warning">
        {{ t('admin-rule-editor-trigger-other-phase') }}
      </p>
    </section>

    <section class="space-y-3">
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

      <ol class="space-y-2">
        <li
          v-for="(action, i) in actions"
          :key="i"
          class="border rounded-md p-3 space-y-2 bg-surface"
        >
          <div class="flex items-center gap-2">
            <span class="text-xs text-secondary font-mono">#{{ i + 1 }}</span>
            <select
              :value="action.kind"
              @change="setActionKind(i, ($event.target as HTMLSelectElement).value as RuleAction['kind'])"
              class="flex-1 border rounded-md px-2 py-1 text-sm bg-surface"
            >
              <option v-for="k in actionKinds" :key="k" :value="k">
                {{ actionLabel(k) }}
              </option>
            </select>
            <Button variant="ghost" size="sm" @click="removeAction(i)">
              <Icon name="trash" class="w-3.5 h-3.5" />
              <span class="sr-only">{{ t('admin-rule-editor-action-remove') }}</span>
            </Button>
          </div>

          <!-- Per-kind config form. Kept inline so the editor stays
               a single component for the Phase 1 surface; if it
               grows past a screen each kind gets its own card. -->
          <template v-if="action.kind === 'reply'">
            <select
              :value="(action.config as any)?.visibility ?? 'public'"
              @change="updateConfigField(i, 'visibility', ($event.target as HTMLSelectElement).value)"
              class="border rounded-md px-2 py-1 text-sm bg-surface"
            >
              <option value="public">{{ t('admin-rules-action-chip-reply-public') }}</option>
              <option value="internal">{{ t('admin-rules-action-chip-reply-internal') }}</option>
            </select>
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
            <select
              :value="(action.config as any)?.priority ?? 'normal'"
              @change="updateConfigField(i, 'priority', ($event.target as HTMLSelectElement).value)"
              class="border rounded-md px-2 py-1 text-sm bg-surface"
            >
              <option value="low">Low</option>
              <option value="normal">Normal</option>
              <option value="high">High</option>
              <option value="urgent">Urgent</option>
            </select>
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

    <section class="space-y-3">
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
