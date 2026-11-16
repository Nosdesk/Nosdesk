<script setup lang="ts">
/**
 * Settings for a rule's reply step: public or internal, and the message.
 * An agent applying a manual rule replies as themselves; a rule that runs
 * on its own replies as the agent picked here. Left unpicked, that is the
 * rule's author (`defaultSendAs`), so the step saves nothing until the
 * admin picks someone else.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BaseDropdown from '@/components/common/BaseDropdown.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import UserPicker from '@/components/ticketComponents/UserPicker.vue';

type PickerRow = { uuid: string; name: string } | null;

const props = defineProps<{
  config: Record<string, unknown> | undefined;
  /** The rule runs on its own, so the reply needs an agent to send as. */
  automated: boolean;
  /** Who the reply sends as when the step names no one. */
  defaultSendAs: string | null;
  /** Name lookup for the people the picker shows. */
  userRow?: (uuid: string) => PickerRow;
}>();

const emit = defineEmits<{
  (e: 'update', field: string, value: unknown): void;
}>();

const { $t: t } = useFluent();

const visibilityOptions = computed(() => [
  { value: 'public', label: t('admin-rule-editor-reply-public') },
  { value: 'internal', label: t('admin-rule-editor-reply-internal') },
]);

const sendAs = computed(() => {
  const picked = props.config?.send_as_uuid;
  return typeof picked === 'string' && picked ? picked : (props.defaultSendAs ?? '');
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <BaseDropdown
      :model-value="String(config?.visibility ?? 'public')"
      :options="visibilityOptions"
      size="sm"
      @update:model-value="emit('update', 'visibility', String($event))"
    />
    <FormTextarea
      :model-value="String(config?.body ?? '')"
      :rows="3"
      :placeholder="t('admin-rule-editor-reply-placeholder')"
      @update:model-value="emit('update', 'body', $event)"
    />
    <div v-if="automated" class="flex flex-col gap-1" data-test="reply-send-as">
      <p class="text-sm font-medium text-primary">{{ t('admin-rule-editor-reply-send-as-label') }}</p>
      <UserPicker
        type="assignee"
        :model-value="sendAs"
        :current-user="sendAs ? (userRow?.(sendAs) ?? null) : null"
        :placeholder="t('admin-rule-editor-reply-send-as-placeholder')"
        hide-inline-clear
        @update:model-value="emit('update', 'send_as_uuid', $event || null)"
      />
      <p class="text-xs text-secondary">{{ t('admin-rule-editor-reply-send-as-hint') }}</p>
    </div>
  </div>
</template>
