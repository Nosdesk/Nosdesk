<script setup lang="ts">
/**
 * Your own name in this workspace (the per-workspace persona). Only you set it;
 * it overlays your account name wherever this workspace shows you.
 */
import { onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import { extractErrorMessage } from '@/utils/errors';
import {
  getUserProfileFields,
  setUserProfileFields,
  type UserProfileFields,
} from '@nosdesk/core/services/userContactService';

const props = defineProps<{ uuid: string }>();
const emit = defineEmits<{ success: [message: string]; error: [message: string] }>();
const { $t: t } = useFluent();

const fields = ref<UserProfileFields | null>(null);
const name = ref('');
const saving = ref(false);

onMounted(async () => {
  try {
    fields.value = await getUserProfileFields(props.uuid);
    name.value = fields.value.display_name ?? '';
  } catch {
    fields.value = null;
  }
});

async function save(): Promise<void> {
  if (!fields.value) return;
  saving.value = true;
  try {
    const f = fields.value;
    fields.value = await setUserProfileFields(props.uuid, {
      job_title: f.job_title,
      organization: f.organization,
      department: f.department,
      custom_fields: f.custom_fields,
      display_name: name.value.trim() || null,
    });
    emit('success', t('workspace-name-saved'));
  } catch (err) {
    emit('error', extractErrorMessage(err, t('workspace-name-error')));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <SectionCard v-if="fields" content-padding="p-4 sm:p-6">
    <template #leading>
      <span class="text-accent inline-flex"><Icon name="user" /></span>
    </template>
    <template #title>{{ t('workspace-name-title') }}</template>
    <form class="flex flex-col gap-3" @submit.prevent="save">
      <p class="text-sm text-secondary">{{ t('workspace-name-description') }}</p>
      <div class="flex items-end gap-2">
        <FormInput v-model="name" :label="t('workspace-name-label')" class="flex-1" :disabled="saving" />
        <Button type="submit" :loading="saving">{{ t('workspace-name-save') }}</Button>
      </div>
    </form>
  </SectionCard>
</template>
