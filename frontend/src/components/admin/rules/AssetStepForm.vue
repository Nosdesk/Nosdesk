<script setup lang="ts">
/**
 * Settings for the rule steps that act on assets: what the step does to
 * the ticket's linked assets or the requester's, and for a loan how long
 * it runs. Leaving the length empty lends with no due date.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import FormNumber from '@/components/common/FormNumber.vue';

const props = defineProps<{
  kind: 'assign_assets_to_requester' | 'lend_assets' | 'return_requester_assets';
  config: Record<string, unknown> | undefined;
}>();

const emit = defineEmits<{
  (e: 'update', field: string, value: unknown): void;
}>();

const { $t: t } = useFluent();

const HINTS: Record<typeof props.kind, string> = {
  assign_assets_to_requester: 'admin-rule-editor-assets-assign-hint',
  lend_assets: 'admin-rule-editor-assets-lend-hint',
  return_requester_assets: 'admin-rule-editor-assets-return-hint',
};

const dueInDays = computed<number | null>(() => {
  const days = Number(props.config?.due_in_days);
  return days > 0 ? days : null;
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <FormNumber
      v-if="kind === 'lend_assets'"
      :model-value="dueInDays"
      :label="t('admin-rule-editor-assets-due-label')"
      :description="t('admin-rule-editor-assets-due-hint')"
      :min="1"
      :max="3650"
      integer
      size="sm"
      @update:model-value="emit('update', 'due_in_days', $event)"
    />
    <p class="text-xs text-secondary">{{ t(HINTS[kind]) }}</p>
  </div>
</template>
