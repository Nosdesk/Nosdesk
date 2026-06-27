<script setup lang="ts">
/**
 * Contact details on a user's profile: the standard SCIM-Enterprise fields
 * (job title, organization, department) plus the workspace's custom fields
 * (rendered from the user-field schema via DynamicAttributeForm). Directory-
 * synced fields render read-only. Phones + addresses are separate cards.
 */
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import SectionCard from '@/components/common/SectionCard.vue';
import FormInput from '@/components/common/FormInput.vue';
import Button from '@/components/common/Button.vue';
import DynamicAttributeForm, {
  type Schema,
  type SchemaProperty,
} from '@/components/assets/DynamicAttributeForm.vue';
import {
  getUserFieldSchema,
  getUserProfileFields,
  setUserProfileFields,
} from '@nosdesk/core/services/userContactService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';

const props = defineProps<{ uuid: string; editable: boolean }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

const schema = ref<Schema>({ type: 'object', properties: {} });
const jobTitle = ref('');
const organization = ref('');
const department = ref('');
const customFields = ref<Record<string, unknown>>({});
const directorySynced = ref(false);
const loading = ref(true);
const saving = ref(false);

const properties = computed(() => (schema.value.properties ?? {}) as Record<string, SchemaProperty>);

/** The user-editable form. A `synced` field is only excluded (read-only) when
 *  THIS user is actually directory-synced; for a local user there's no directory
 *  feeding it, so it's an ordinary editable field. */
const editableSchema = computed<Schema>(() => {
  const out: Record<string, SchemaProperty> = {};
  for (const [k, v] of Object.entries(properties.value)) {
    if (!v.synced || !directorySynced.value) out[k] = v;
  }
  return { type: 'object', properties: out, required: schema.value.required };
});

/** Synced fields rendered read-only — only when the directory actually feeds
 *  this user. */
const syncedFields = computed(() =>
  directorySynced.value
    ? Object.entries(properties.value)
        .filter(([, v]) => v.synced)
        .map(([key, v]) => ({ key, title: v.title ?? key }))
    : [],
);

const hasCustomFields = computed(() => Object.keys(properties.value).length > 0);

function display(value: unknown): string {
  if (value === null || value === undefined || value === '') return '—';
  return String(value);
}

async function load(): Promise<void> {
  loading.value = true;
  try {
    const [s, profile] = await Promise.all([getUserFieldSchema(), getUserProfileFields(props.uuid)]);
    schema.value =
      s && typeof s === 'object' && !Array.isArray(s) ? (s as Schema) : { type: 'object', properties: {} };
    jobTitle.value = profile.job_title ?? '';
    organization.value = profile.organization ?? '';
    department.value = profile.department ?? '';
    customFields.value = profile.custom_fields ?? {};
    directorySynced.value = profile.directory_synced;
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-contact-error-load')));
  } finally {
    loading.value = false;
  }
}
onMounted(load);

async function save(): Promise<void> {
  saving.value = true;
  try {
    const saved = await setUserProfileFields(props.uuid, {
      job_title: jobTitle.value.trim() || null,
      organization: organization.value.trim() || null,
      department: department.value.trim() || null,
      custom_fields: customFields.value,
    });
    jobTitle.value = saved.job_title ?? '';
    organization.value = saved.organization ?? '';
    department.value = saved.department ?? '';
    customFields.value = saved.custom_fields ?? {};
    toast.success(t('user-contact-saved'));
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-contact-error-save')));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <SectionCard v-if="!loading" content-padding="p-4">
    <template #title>
      <span class="flex items-center gap-2">
        {{ t('user-contact-title') }}
        <span
          v-if="directorySynced"
          class="text-xs px-1.5 py-0.5 rounded bg-surface-alt text-tertiary font-normal"
        >{{ t('user-contact-synced-badge') }}</span>
      </span>
    </template>

    <div class="flex flex-col gap-4">
      <!-- Standard SCIM-Enterprise fields -->
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <FormInput
          v-model="jobTitle"
          :label="t('user-contact-field-job-title')"
          :disabled="!editable || directorySynced"
          size="sm"
        />
        <FormInput
          v-model="organization"
          :label="t('user-contact-field-organization')"
          :disabled="!editable || directorySynced"
          size="sm"
        />
        <FormInput
          v-model="department"
          :label="t('user-contact-field-department')"
          :disabled="!editable || directorySynced"
          size="sm"
        />
      </div>

      <!-- Synced custom fields (read-only) -->
      <div v-if="syncedFields.length > 0" class="flex flex-col gap-2 pt-1 border-t border-subtle">
        <div v-for="f in syncedFields" :key="f.key" class="flex items-center justify-between gap-3">
          <span class="text-xs font-medium uppercase tracking-wide text-tertiary">{{ f.title }}</span>
          <span class="text-sm text-secondary">{{ display(customFields[f.key]) }}</span>
        </div>
      </div>

      <!-- Editable custom fields -->
      <div v-if="Object.keys(editableSchema.properties ?? {}).length > 0" class="pt-1 border-t border-subtle">
        <DynamicAttributeForm
          v-model="customFields"
          :schema="editableSchema"
        />
      </div>

      <p v-else-if="!hasCustomFields" class="text-sm text-tertiary">
        {{ t('user-contact-no-custom-fields') }}
      </p>

      <div v-if="editable" class="flex justify-end pt-1">
        <Button size="sm" :loading="saving" @click="save">{{ t('common-save') }}</Button>
      </div>
    </div>
  </SectionCard>
</template>
