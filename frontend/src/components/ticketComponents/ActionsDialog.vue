<script setup lang="ts">
/**
 * Agent toolbar dialog for manual rule application ("Actions"
 * surface, decision 26 in the rules-and-actions plan). Open from
 * the ticket view's toolbar; lists the live manual rules in the
 * caller's workspace, preview the actions a rule will take, and
 * apply it via POST /api/rules/{id}/apply.
 *
 * Phase 1 is unconditional: manual rules have empty conditions, so
 * the picker shows every live manual rule. When Path A relaxation
 * lands (manual rules with visibility-filter conditions), this
 * dialog reads from the same `applicable-actions` endpoint —
 * server-side filtering will then narrow the list automatically.
 */
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';

import Modal from '@/components/Modal.vue';
import Button from '@/components/common/Button.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import rulesService from '@/services/rulesService';
import { useToastStore } from '@/stores/toast';
import { extractErrorMessage } from '@/utils/errors';
import type { Rule, RuleAction } from '@/types/rule';

const props = defineProps<{
  show: boolean;
  ticketId: number;
}>();
const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'applied', ruleId: number): void;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

const PICK_KEY = computed(
  () => ['applicable-actions', props.ticketId] as const,
);
const picksQuery = useQuery({
  key: PICK_KEY,
  query: () => rulesService.pickableActions(props.ticketId),
  enabled: () => props.show,
});

const search = ref('');
const selected = ref<Rule | null>(null);
const overrideBody = ref<string | null>(null);
const showOverrideEditor = ref(false);
const applying = ref(false);

const allRules = computed<Rule[]>(() =>
  Array.isArray(picksQuery.data.value) ? picksQuery.data.value : [],
);
const filtered = computed<Rule[]>(() => {
  const q = search.value.trim().toLowerCase();
  if (!q) return allRules.value;
  return allRules.value.filter((r) => r.name.toLowerCase().includes(q));
});
const isFirstLoad = computed(
  () => picksQuery.status.value === 'pending' && picksQuery.data.value === undefined,
);

watch(
  () => props.show,
  (val) => {
    if (!val) {
      selected.value = null;
      overrideBody.value = null;
      showOverrideEditor.value = false;
      search.value = '';
    }
  },
);

function actionSummaryChips(rule: Rule): string[] {
  return rule.actions.map((a) => describeAction(a));
}

function describeAction(action: RuleAction): string {
  const config = (action.config ?? {}) as Record<string, unknown>;
  switch (action.kind) {
    case 'reply':
      return config.visibility === 'internal'
        ? t('admin-rules-action-chip-reply-internal')
        : t('admin-rules-action-chip-reply-public');
    case 'set_status':
      return t('admin-rules-action-chip-set-status', {
        state_id: Number(config.workflow_state_id ?? 0),
      });
    case 'assign':
      return t('admin-rules-action-chip-assign');
    case 'unassign':
      return t('admin-rules-action-chip-unassign');
    case 'add_tags':
      return t('admin-rules-action-chip-add-tags', {
        count: Array.isArray(config.tag_ids) ? config.tag_ids.length : 0,
      });
    case 'remove_tags':
      return t('admin-rules-action-chip-remove-tags', {
        count: Array.isArray(config.tag_ids) ? config.tag_ids.length : 0,
      });
    case 'set_priority':
      return t('admin-rules-action-chip-set-priority', {
        priority: String(config.priority ?? 'normal'),
      });
    case 'stop_processing':
      return t('admin-rules-action-chip-stop-processing');
    default:
      return action.kind;
  }
}

const replyAction = computed<RuleAction | null>(() => {
  const r = selected.value;
  if (!r) return null;
  return r.actions.find((a) => a.kind === 'reply') ?? null;
});

const replyBodyForPreview = computed<string>(() => {
  if (overrideBody.value !== null) return overrideBody.value;
  const action = replyAction.value;
  if (!action) return '';
  return String((action.config as Record<string, unknown> | undefined)?.body ?? '');
});

async function apply(): Promise<void> {
  const rule = selected.value;
  if (!rule) return;
  applying.value = true;
  try {
    await rulesService.apply(rule.id, {
      ticket_id: props.ticketId,
      overrides: {
        body: overrideBody.value ?? undefined,
      },
    });
    toast.success(t('ticket-actions-success-toast', { rule: rule.name }));
    emit('applied', rule.id);
    emit('close');
  } catch (err) {
    toast.error(extractErrorMessage(err, t('ticket-actions-error-toast')));
  } finally {
    applying.value = false;
  }
}
</script>

<template>
  <Modal :show="show" :title="t('ticket-actions-dialog-title')" @close="emit('close')">
    <div class="flex flex-col gap-4 max-w-xl w-full">
      <input
        v-model="search"
        type="search"
        :placeholder="t('ticket-actions-dialog-picker-placeholder')"
        class="w-full border rounded-md px-3 py-2 text-sm bg-surface"
        autofocus
      />

      <Skeleton v-if="isFirstLoad" class="flex flex-col gap-2">
        <SkeletonBar v-for="i in 3" :key="i" class="h-12 w-full" />
      </Skeleton>

      <p v-else-if="filtered.length === 0" class="text-sm text-secondary">
        {{ t('ticket-actions-dialog-empty') }}
      </p>

      <ul v-else class="flex flex-col gap-1 max-h-64 overflow-y-auto">
        <li
          v-for="rule in filtered"
          :key="rule.id"
          :class="[
            'border rounded-md p-2 cursor-pointer',
            selected?.id === rule.id ? 'border-primary bg-primary/5' : 'hover:bg-surface-hover',
          ]"
          @click="selected = rule; overrideBody = null"
        >
          <div class="font-medium text-sm">{{ rule.name }}</div>
          <div v-if="rule.description" class="text-xs text-secondary truncate">
            {{ rule.description }}
          </div>
          <div class="mt-1 flex flex-wrap gap-1">
            <span
              v-for="(chip, i) in actionSummaryChips(rule)"
              :key="i"
              class="inline-block rounded-full bg-surface-hover px-2 py-0.5 text-xs"
            >
              {{ chip }}
            </span>
          </div>
        </li>
      </ul>

      <div v-if="selected" class="border-t pt-3 flex flex-col gap-2">
        <p class="text-sm font-medium">{{ t('ticket-actions-dialog-action-list-label') }}</p>
        <ul class="flex flex-col gap-1 text-sm">
          <li v-for="(chip, i) in actionSummaryChips(selected)" :key="i" class="text-secondary">
            • {{ chip }}
          </li>
        </ul>

        <template v-if="replyAction">
          <Button
            v-if="!showOverrideEditor"
            variant="ghost"
            size="sm"
            @click="overrideBody = replyBodyForPreview; showOverrideEditor = true"
          >
            Edit reply
          </Button>
          <FormTextarea
            v-if="showOverrideEditor"
            :model-value="overrideBody ?? ''"
            @update:model-value="overrideBody = $event"
            :rows="4"
          />
        </template>
      </div>

      <div class="flex justify-end gap-2">
        <Button variant="secondary" @click="emit('close')">
          {{ t('ticket-actions-dialog-cancel') }}
        </Button>
        <Button
          variant="primary"
          :disabled="!selected || applying"
          :loading="applying"
          @click="apply"
        >
          {{ applying ? t('ticket-actions-dialog-applying') : t('ticket-actions-dialog-apply') }}
        </Button>
      </div>
    </div>
  </Modal>
</template>
