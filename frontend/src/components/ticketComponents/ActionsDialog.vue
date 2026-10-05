<script setup lang="ts">
/**
 * The ticket's Actions dialog. An agent picks one of the workspace's live
 * manual rules, sees each step it will take on this ticket in plain words
 * (with the reply as it will read), can untick a step or edit the reply,
 * and applies it. The ticket and its comments update through the sync
 * stream, so the dialog only closes and confirms.
 */
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import Modal from '@/components/Modal.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import SearchInput from '@/components/common/SearchInput.vue';
import userService from '@/services/userService';
import { extractErrorMessage } from '@/utils/errors';
import rulesService from '@nosdesk/core/services/rulesService';
import { renderTemplate, type TemplateVars } from '@nosdesk/core/services/cannedResponsesService';
import { useTagsStore } from '@nosdesk/core/stores/tags';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates';
import { escapeHtml } from '@nosdesk/core/utils/escape';
import type { Rule, RuleAction } from '@nosdesk/core/types/rule';

const props = defineProps<{
  show: boolean;
  ticketId: number;
  /** The live manual rules agents can apply here. */
  rules: Rule[];
  /** This ticket's values for reply variables. */
  vars: TemplateVars;
}>();
const emit = defineEmits<{ close: [] }>();

const { $t: t } = useFluent();
const toast = useToastStore();
const queryCache = useQueryCache();
const states = useWorkflowStatesStore();
const tags = useTagsStore();

// Matches the backend's reply_template_html: a template with tags is HTML,
// one without is plain text whose line breaks are kept.
const HTML_TAG_RE = /<\/?[A-Za-z][A-Za-z0-9]*(\s[^<>]*)?\/?>/;
// Searching only helps once the list is longer than a glance.
const SEARCH_FROM = 6;

const search = ref('');
const selectedId = ref<number | null>(null);
/** 1-based positions in the rule's action list the agent unticked. */
const skipped = ref<Set<number>>(new Set());
/** The edited reply template, or null while it's the rule's own. */
const replyDraft = ref<string | null>(null);
const editingReply = ref(false);
const applying = ref(false);
const error = ref('');

// Most used first, so the routine ones sit at the top.
const sortedRules = computed(() =>
  [...props.rules].sort((a, b) => b.fire_count - a.fire_count || a.name.localeCompare(b.name)),
);
const visibleRules = computed(() => {
  const term = search.value.trim().toLowerCase();
  if (!term) return sortedRules.value;
  return sortedRules.value.filter(
    (r) => r.name.toLowerCase().includes(term) || (r.description ?? '').toLowerCase().includes(term),
  );
});
const selected = computed(() => props.rules.find((r) => r.id === selectedId.value) ?? null);

watch(
  () => props.show,
  (open) => {
    if (!open) return;
    search.value = '';
    error.value = '';
    selectedId.value = sortedRules.value[0]?.id ?? null;
    void states.load();
  },
  { immediate: true },
);
watch(selectedId, () => {
  skipped.value = new Set();
  replyDraft.value = null;
  editingReply.value = false;
  error.value = '';
});

const config = (action: RuleAction) => (action.config ?? {}) as Record<string, unknown>;

// Names for the people the selected rule assigns to.
const assigneeUuids = computed(() =>
  (selected.value?.actions ?? [])
    .filter((a) => a.kind === 'assign' && typeof config(a).user_uuid === 'string')
    .map((a) => String(config(a).user_uuid)),
);
const assigneesQuery = useQuery({
  key: () => ['users', 'batch', ...assigneeUuids.value],
  query: () => userService.getUsersBatch(assigneeUuids.value),
  enabled: () => props.show && assigneeUuids.value.length > 0,
  staleTime: 5 * 60 * 1000,
});
const assigneeName = (uuid: unknown): string | null =>
  (assigneesQuery.data.value ?? []).find((u) => u.uuid === uuid)?.name ?? null;

const tagNames = (ids: unknown): string[] | null => {
  if (!Array.isArray(ids)) return null;
  const names = ids.map((id) => tags.findById(Number(id))?.name);
  return names.every((n): n is string => !!n) ? names : null;
};

// The backend reads `normal` as medium, so the label does too.
const priorityLabel = (value: unknown) => {
  const v = String(value ?? 'medium');
  return t(`priority-${v === 'normal' ? 'medium' : v}`);
};

interface Step {
  /** 1-based position in the rule's action list. */
  position: number;
  action: RuleAction;
  label: string;
}

function describe(action: RuleAction): string | null {
  const c = config(action);
  switch (action.kind) {
    case 'reply':
      return c.visibility === 'internal' ? t('ticket-actions-step-note') : t('ticket-actions-step-reply');
    case 'set_status': {
      const state = states.findById(Number(c.workflow_state_id));
      return state ? t('ticket-actions-step-status', { status: state.name }) : t('ticket-actions-step-status-unknown');
    }
    case 'assign': {
      const name = assigneeName(c.user_uuid);
      return name ? t('ticket-actions-step-assign', { name }) : t('ticket-actions-step-assign-unknown');
    }
    case 'unassign':
      return t('ticket-actions-step-unassign');
    case 'add_tags':
    case 'remove_tags': {
      const names = tagNames(c.tag_ids);
      const count = Array.isArray(c.tag_ids) ? c.tag_ids.length : 0;
      const kind = action.kind === 'add_tags' ? 'add' : 'remove';
      return names
        ? t(`ticket-actions-step-${kind}-tags`, { tags: names.join(', '), count })
        : t(`ticket-actions-step-${kind}-tags-unknown`, { count });
    }
    case 'set_priority':
      return t('ticket-actions-step-priority', { priority: priorityLabel(c.priority) });
    default:
      // stop_processing does nothing when an agent applies a rule.
      return null;
  }
}

const steps = computed<Step[]>(() =>
  (selected.value?.actions ?? []).flatMap((action, i) => {
    const label = describe(action);
    return label ? [{ position: i + 1, action, label }] : [];
  }),
);

/** The first reply step: the one an edited reply replaces. */
const firstReplyPosition = computed(
  () => steps.value.find((s) => s.action.kind === 'reply')?.position ?? null,
);

function replyTemplate(step: Step): string {
  if (step.position === firstReplyPosition.value && replyDraft.value !== null) return replyDraft.value;
  return String(config(step.action).body ?? '');
}

/** A reply as it will read on this ticket. HTML templates get escaped
 *  values, as the backend does; the preview renders through v-safe-html. */
function replyPreview(step: Step): { html: boolean; text: string } {
  const template = replyTemplate(step);
  if (!HTML_TAG_RE.test(template)) return { html: false, text: renderTemplate(template, props.vars) };
  const escaped = Object.fromEntries(
    Object.entries(props.vars).map(([k, v]) => [k, typeof v === 'string' ? escapeHtml(v) : v]),
  ) as TemplateVars;
  return { html: true, text: renderTemplate(template, escaped) };
}

function toggleStep(position: number, run: boolean) {
  const next = new Set(skipped.value);
  if (run) next.delete(position);
  else next.add(position);
  skipped.value = next;
}

function editReply(step: Step) {
  replyDraft.value = replyTemplate(step);
  editingReply.value = true;
}

const runningSteps = computed(() => steps.value.filter((s) => !skipped.value.has(s.position)));
const replyIsEmpty = computed(() => {
  const position = firstReplyPosition.value;
  if (position === null || skipped.value.has(position)) return false;
  return replyDraft.value !== null && replyDraft.value.trim() === '';
});
const canApply = computed(
  () => !!selected.value && runningSteps.value.length > 0 && !replyIsEmpty.value && !applying.value,
);

async function apply() {
  const rule = selected.value;
  if (!rule || !canApply.value) return;
  applying.value = true;
  error.value = '';
  try {
    const firstReply = steps.value.find((s) => s.position === firstReplyPosition.value);
    const edited =
      replyDraft.value !== null && firstReply && replyDraft.value !== String(config(firstReply.action).body ?? '');
    await rulesService.apply(rule.id, {
      ticket_id: props.ticketId,
      overrides: {
        body: edited ? replyDraft.value ?? undefined : undefined,
        suppress_actions: [...skipped.value].sort((a, b) => a - b),
      },
    });
    toast.success(t('ticket-actions-success-toast', { rule: rule.name }));
    // The run count orders the list; refresh it for next time.
    void queryCache.invalidateQueries({ key: ['rules', 'applicable'] });
    emit('close');
  } catch (err) {
    error.value = extractErrorMessage(err, t('ticket-actions-error'));
  } finally {
    applying.value = false;
  }
}
</script>

<template>
  <Modal
    :show="show"
    :title="t('ticket-actions-dialog-title')"
    :description="t('ticket-actions-dialog-description')"
    size="md"
    @close="emit('close')"
  >
    <form class="flex flex-col gap-4" @submit.prevent="apply">
      <AlertMessage v-if="error" type="error" :message="error" />

      <p v-if="rules.length === 0" class="text-sm text-secondary">
        {{ t('ticket-actions-dialog-empty') }}
      </p>

      <div v-else class="flex flex-col gap-4 sm:flex-row sm:items-start">
        <!-- Rule list: native radios, so arrow keys move between rules. -->
        <fieldset class="flex flex-col gap-2 sm:w-2/5 min-w-0">
          <legend class="sr-only">{{ t('ticket-actions-dialog-list-label') }}</legend>
          <SearchInput
            v-if="rules.length >= SEARCH_FROM"
            v-model="search"
            :placeholder="t('ticket-actions-dialog-search-placeholder')"
          />
          <p v-if="visibleRules.length === 0" class="text-sm text-secondary px-1">
            {{ t('ticket-actions-dialog-no-match') }}
          </p>
          <div class="flex flex-col gap-1.5 max-h-72 overflow-y-auto">
            <label
              v-for="rule in visibleRules"
              :key="rule.id"
              class="relative flex flex-col gap-0.5 rounded-lg border px-3 py-2 cursor-pointer transition-colors"
              :class="rule.id === selectedId
                ? 'border-accent bg-accent/5'
                : 'border-default hover:bg-surface-hover'"
            >
              <input
                v-model="selectedId"
                type="radio"
                name="ticket-action"
                class="peer sr-only"
                :value="rule.id"
                :autofocus="rule.id === selectedId"
              />
              <span
                aria-hidden="true"
                class="pointer-events-none absolute inset-0 rounded-lg peer-focus-visible:ring-2 peer-focus-visible:ring-accent"
              />
              <span class="text-sm font-medium text-primary">{{ rule.name }}</span>
              <span v-if="rule.description" class="text-xs text-secondary">{{ rule.description }}</span>
            </label>
          </div>
        </fieldset>

        <!-- What the selected rule will do here. -->
        <section v-if="selected" class="flex flex-col gap-3 flex-1 min-w-0">
          <h3 class="text-sm font-medium text-primary">{{ t('ticket-actions-dialog-steps-label') }}</h3>
          <ul class="flex flex-col gap-2">
            <li v-for="step in steps" :key="step.position" class="flex flex-col gap-2">
              <Checkbox
                :model-value="!skipped.has(step.position)"
                :label="step.label"
                @update:model-value="toggleStep(step.position, $event)"
              />
              <div
                v-if="step.action.kind === 'reply' && !skipped.has(step.position)"
                class="ml-6 flex flex-col gap-2"
              >
                <FormTextarea
                  v-if="editingReply && step.position === firstReplyPosition"
                  :model-value="replyDraft ?? ''"
                  :label="t('ticket-actions-reply-edit-label')"
                  :description="t('ticket-actions-reply-edit-hint')"
                  :rows="6"
                  @update:model-value="replyDraft = $event"
                />
                <div class="rounded-lg border border-default bg-surface-alt px-3 py-2 text-sm text-primary">
                  <div v-if="replyPreview(step).html" v-safe-html="replyPreview(step).text" class="reply-preview" />
                  <p v-else class="whitespace-pre-wrap">{{ replyPreview(step).text }}</p>
                </div>
                <div v-if="step.position === firstReplyPosition && !editingReply">
                  <Button type="button" variant="ghost" size="sm" icon="rename" @click="editReply(step)">
                    {{ t('ticket-actions-reply-edit') }}
                  </Button>
                </div>
              </div>
            </li>
          </ul>
          <p v-if="replyIsEmpty" class="text-xs text-status-error">{{ t('ticket-actions-reply-empty') }}</p>
        </section>
      </div>

      <div class="modal-actions flex items-center gap-2">
        <Button type="button" variant="secondary" class="ml-auto" :disabled="applying" @click="emit('close')">
          {{ t('ticket-actions-dialog-cancel') }}
        </Button>
        <Button type="submit" :loading="applying" :disabled="!canApply">
          {{ t('ticket-actions-dialog-apply') }}
        </Button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.reply-preview :deep(p) {
  margin: 0 0 0.5rem;
}
.reply-preview :deep(p:last-child) {
  margin-bottom: 0;
}
</style>
