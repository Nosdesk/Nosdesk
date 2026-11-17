<script setup lang="ts">
/**
 * Settings for a rule's notify step: an email about the ticket to its
 * requester or to a person in the workspace, with a subject and a message.
 * It goes out by email only; nothing is posted on the ticket.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import UserPicker from '@/components/ticketComponents/UserPicker.vue';

type PickerRow = { uuid: string; name: string } | null;

const props = defineProps<{
  config: Record<string, unknown> | undefined;
  /** Name lookup for the person the picker shows. */
  userRow?: (uuid: string) => PickerRow;
}>();

const emit = defineEmits<{
  (e: 'update', field: string, value: unknown): void;
}>();

const { $t: t } = useFluent();

const recipient = computed(() => (props.config?.recipient === 'user' ? 'user' : 'requester'));
const recipientOptions = computed(() => [
  { value: 'requester', label: t('admin-rule-editor-notify-to-requester') },
  { value: 'user', label: t('admin-rule-editor-notify-to-person') },
]);
const person = computed(() => String(props.config?.user_uuid ?? ''));

function setRecipient(next: string): void {
  emit('update', 'recipient', next);
  if (next !== 'user') emit('update', 'user_uuid', undefined);
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div>
      <SegmentedControl
        :model-value="recipient"
        :options="recipientOptions"
        :aria-label="t('admin-rule-editor-notify-to')"
        size="sm"
        data-test="notify-recipient"
        @update:model-value="setRecipient(String($event))"
      />
    </div>
    <UserPicker
      v-if="recipient === 'user'"
      type="requester"
      :model-value="person"
      :current-user="person ? (userRow?.(person) ?? null) : null"
      :placeholder="t('admin-rule-editor-notify-person-placeholder')"
      @update:model-value="emit('update', 'user_uuid', $event)"
    />
    <FormInput
      :model-value="String(config?.subject ?? '')"
      :label="t('admin-rule-editor-notify-subject-label')"
      size="sm"
      data-test="notify-subject"
      @update:model-value="emit('update', 'subject', $event)"
    />
    <FormTextarea
      :model-value="String(config?.body ?? '')"
      :label="t('admin-rule-editor-notify-body-label')"
      :rows="3"
      data-test="notify-body"
      @update:model-value="emit('update', 'body', $event)"
    />
    <p class="text-xs text-secondary">{{ t('admin-rule-editor-notify-hint') }}</p>
  </div>
</template>
