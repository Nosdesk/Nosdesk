<script setup lang="ts">
/**
 * Admin rule editor. Creates a new rule when route name is
 * `admin-rules-new`, edits an existing rule by id when route name
 * is `admin-rules-edit`. Agents apply manual rules from a ticket's
 * Actions dialog; manual rules have `conditions = []` enforced by the
 * backend, so the editor has no conditions section.
 *
 * The action list is the main interaction surface. Each action
 * carries a typed `kind` plus a kind-specific config object. The
 * supported kinds are reply / set_status / assign / unassign /
 * add_tags / remove_tags / set_priority / stop_processing; notify /
 * apply_macro_template / webhook are deferred and rejected by the
 * backend with RULE_ACTION_UNSUPPORTED.
 *
 * Only manual rules run today, so a new rule is always manual. An older
 * rule saved with another trigger keeps it, with a note that it won't
 * run and the option to switch it to manual.
 */
import { computed, onMounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import IconButton from '@/components/common/IconButton.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import rulesService from '@nosdesk/core/services/rulesService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { useMobileDetection } from '@/composables/useMobileDetection';

// Desktop only: on mobile the leading back-arrow in SiteHeader is the single
// back affordance, so this inline control hides to avoid two per screen. Same
// contract BackButton encodes; kept inline here because this toolbar's
// secondary-button styling is deliberate.
const { isMobile } = useMobileDetection('sm');
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
const transitioning = ref(false);
const errorMessage = ref('');
const original = ref<Rule | null>(null);

const name = ref('');
const description = ref('');
const triggerKind = ref<RuleTriggerKind>('manual');
const priority = ref<number>(100);
const actions = ref<RuleAction[]>([{ kind: 'reply', config: { visibility: 'public', body: '' } }]);
const overrideSelfRef = ref(false);

function fill(rule: Rule): void {
  original.value = rule;
  name.value = rule.name;
  description.value = rule.description ?? '';
  triggerKind.value = rule.trigger_kind;
  priority.value = rule.priority;
  actions.value = Array.isArray(rule.actions) ? [...rule.actions] : [];
}

onMounted(async () => {
  if (isNew.value) return;
  if (ruleId.value == null) return;
  loading.value = true;
  try {
    fill(await rulesService.get(ruleId.value));
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

const isManual = computed(() => triggerKind.value === 'manual');

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
        return { priority: 'medium' };
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

/** Unsaved edits, compared with the rule as last loaded or saved. */
const isDirty = computed(() => {
  const rule = original.value;
  if (!rule) return true;
  return (
    name.value.trim() !== rule.name ||
    (description.value.trim() || null) !== rule.description ||
    triggerKind.value !== rule.trigger_kind ||
    priority.value !== rule.priority ||
    JSON.stringify(actions.value) !== JSON.stringify(rule.actions)
  );
});

/** Saves the form; `quiet` skips the toast when going live follows. */
async function save(options: { quiet?: boolean } = {}): Promise<boolean> {
  errorMessage.value = '';
  if (!canSave.value) return false;
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
      // The route changes but this component stays mounted, so keep the
      // saved rule here rather than waiting for a reload.
      fill(created);
      await queryCache.invalidateQueries({ key: ['rules'] });
      if (!options.quiet) toast.success(t('admin-rules-toast-created', { name: created.name }));
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
      fill(await rulesService.update(ruleId.value, update));
      await queryCache.invalidateQueries({ key: ['rules'] });
      if (!options.quiet) toast.success(t('admin-rules-toast-saved', { name: payload.name }));
    }
    return true;
  } catch (err) {
    errorMessage.value = extractErrorMessage(err, t('admin-rule-editor-error-save'));
    return false;
  } finally {
    saving.value = false;
  }
}

// Agents can apply a rule only while it's live; pausing (the dry_run
// state) takes it out of their Actions list. Going live saves pending
// edits first so agents get the rule as shown here.
async function setLive(live: boolean): Promise<void> {
  if (isDirty.value && !(await save({ quiet: true }))) return;
  const rule = original.value;
  if (!rule) return;
  transitioning.value = true;
  try {
    fill(await rulesService.transitionState(rule.id, { state: live ? 'live' : 'dry_run' }));
    await queryCache.invalidateQueries({ key: ['rules'] });
    toast.success(t(live ? 'admin-rules-toast-live' : 'admin-rules-toast-paused', { name: rule.name }));
  } catch (err) {
    errorMessage.value = extractErrorMessage(err, t('admin-rules-error-transition'));
  } finally {
    transitioning.value = false;
  }
}

const stateNote = computed(() => {
  switch (original.value?.state) {
    case undefined:
      return t('admin-rule-editor-state-new');
    case 'live':
      return t('admin-rule-editor-state-live');
    case 'dry_run':
      return t('admin-rule-editor-state-paused');
    default:
      return t('admin-rule-editor-state-draft');
  }
});

function back(): void {
  router.push({ name: 'admin-rules' });
}

function triggerLabel(kind: RuleTriggerKind): string {
  return t(`admin-rules-trigger-${kind.replace(/_/g, '-')}`);
}

// A rule saved with a trigger that doesn't run yet can switch to manual.
// New and manual rules have nothing to choose.
const triggerOptions = computed(() => {
  const saved = original.value?.trigger_kind;
  if (!saved || saved === 'manual') return [];
  return [saved, 'manual' as const].map((k) => ({ value: k, label: triggerLabel(k) }));
});

// stop_processing only means something to rules that run on their own.
const actionKinds = computed<RuleAction['kind'][]>(() => [
  'reply',
  'set_status',
  'assign',
  'unassign',
  'add_tags',
  'remove_tags',
  'set_priority',
  ...(isManual.value ? [] : (['stop_processing'] as const)),
]);

const SUPPORTED_KINDS: RuleAction['kind'][] = [
  'reply',
  'set_status',
  'assign',
  'unassign',
  'add_tags',
  'remove_tags',
  'set_priority',
  'stop_processing',
];

function actionLabel(kind: RuleAction['kind']): string {
  return SUPPORTED_KINDS.includes(kind)
    ? t(`admin-rule-editor-action-${kind.replace(/_/g, '-')}`)
    : kind;
}

// BaseDropdown option lists (value/label) for the enum selects.
const actionOptions = computed(() =>
  actionKinds.value.map((k) => ({ value: k, label: actionLabel(k) })),
);
const replyVisibilityOptions = computed(() => [
  { value: 'public', label: t('admin-rule-editor-reply-public') },
  { value: 'internal', label: t('admin-rule-editor-reply-internal') },
]);
const priorityOptions = computed(() =>
  ['low', 'medium', 'high', 'urgent'].map((value) => ({ value, label: t(`priority-${value}`) })),
);
// The backend reads `normal` as medium; older rules may carry it.
const priorityValue = (config: Record<string, unknown> | undefined) => {
  const value = String(config?.priority ?? 'medium');
  return value === 'normal' ? 'medium' : value;
};
</script>

<template>
  <div class="flex flex-col gap-6 max-w-3xl">
    <div class="flex items-center gap-3">
      <Button v-if="!isMobile" variant="secondary" size="sm" @click="back" icon="chevronLeft">
        <span>{{ t('admin-rule-editor-back') }}</span>
      </Button>
      <h1 class="text-2xl font-semibold flex-1 min-w-0 truncate">
        {{ headerTitle }}
      </h1>
      <Button variant="primary" :disabled="!canSave" :loading="saving" @click="save()">
        {{ t('admin-rule-editor-save') }}
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
      <BaseDropdown
        v-if="triggerOptions.length > 0"
        :model-value="triggerKind"
        :options="triggerOptions"
        :label="t('admin-rule-editor-trigger-label')"
        size="sm"
        @update:model-value="triggerKind = String($event) as RuleTriggerKind"
      />
      <p v-if="isManual" class="text-sm text-secondary">
        {{ t('admin-rule-editor-trigger-manual-summary') }}
      </p>
      <p v-else class="text-sm text-status-warning">
        {{ t('admin-rule-editor-trigger-other-phase') }}
      </p>
    </section>

    <section class="flex flex-col gap-3">
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold text-secondary uppercase tracking-wide">
          {{ t('admin-rule-editor-section-actions') }}
        </h2>
        <Button variant="ghost" size="sm" @click="addAction" icon="add">
          <span>{{ t('admin-rule-editor-actions-add') }}</span>
        </Button>
      </div>

      <p v-if="actions.length === 0" class="text-sm text-status-warning">
        {{ t('admin-rule-editor-actions-empty') }}
      </p>

      <ol class="flex flex-col gap-2">
        <li
          v-for="(action, i) in actions"
          :key="i"
          class="border border-default rounded-lg p-3 flex flex-col gap-2 bg-surface"
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
            <IconButton size="sm" icon="trash" :label="t('admin-rule-editor-action-remove')" @click="removeAction(i)" />
          </div>

          <!-- Per-kind config form. Kept inline so the editor stays
               a single component; if it grows past a screen each
               kind gets its own card. -->
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
              :placeholder="t('admin-rule-editor-reply-placeholder')"
            />
          </template>

          <template v-else-if="action.kind === 'set_status'">
            <FormNumber
              :model-value="Number((action.config as any)?.workflow_state_id) || null"
              @update:model-value="updateConfigField(i, 'workflow_state_id', $event ?? undefined)"
              :label="t('admin-rule-editor-status-id-label')"
              :min="1"
              integer
            />
          </template>

          <template v-else-if="action.kind === 'assign'">
            <FormInput
              :model-value="(action.config as any)?.user_uuid ?? ''"
              @update:model-value="updateConfigField(i, 'user_uuid', $event)"
              :label="t('admin-rule-editor-user-id-label')"
              placeholder="00000000-0000-0000-0000-000000000000"
            />
          </template>

          <template v-else-if="action.kind === 'set_priority'">
            <BaseDropdown
              :model-value="priorityValue(action.config)"
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
              :label="t('admin-rule-editor-tag-ids-label')"
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
      <div class="flex flex-col gap-3 rounded-lg border border-default bg-surface px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <p class="text-sm text-secondary">{{ stateNote }}</p>
        <Button
          v-if="original?.state === 'live'"
          variant="secondary"
          size="sm"
          icon="pause"
          :loading="transitioning"
          @click="setLive(false)"
        >
          {{ t('admin-rules-pause') }}
        </Button>
        <Button
          v-else-if="original && isManual"
          variant="secondary"
          size="sm"
          icon="play"
          :loading="transitioning"
          :disabled="!canSave"
          @click="setLive(true)"
        >
          {{ t('admin-rules-go-live') }}
        </Button>
      </div>
      <!-- Run order and the loop override only matter to rules that run
           on their own. -->
      <template v-if="!isManual">
        <FormNumber
          :model-value="priority"
          @update:model-value="priority = $event ?? 100"
          :label="t('admin-rule-editor-priority-label')"
          integer
        />
        <Checkbox
          v-model="overrideSelfRef"
          :label="t('admin-rule-editor-override-self-ref')"
        />
      </template>
    </section>
  </div>
</template>
