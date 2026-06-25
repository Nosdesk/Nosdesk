<script setup lang="ts">
/**
 * Admin editor for the workspace's user custom-field schema. Reuses the asset
 * schema builder (AttributeEditor) bound to the schema object; defines the
 * optional/custom fields (gender, birthday, year level, …) that appear on every
 * user's profile alongside the standard contact fields. `synced` fields (e.g.
 * office_location) are fed read-only by the directory sync.
 */
import { onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import BackButton from '@/components/common/BackButton.vue';
import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import AttributeEditor from '@/components/assetKindComponents/AttributeEditor.vue';
import { getUserFieldSchema, setUserFieldSchema } from '@/services/userContactService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

const schema = ref<Record<string, unknown>>({ type: 'object', properties: {} });
const isLoading = ref(true);
const loadError = ref('');
const saving = ref(false);
const conflictCount = ref<number | null>(null);

onMounted(async () => {
  try {
    const loaded = await getUserFieldSchema();
    schema.value =
      loaded && typeof loaded === 'object' && !Array.isArray(loaded)
        ? loaded
        : { type: 'object', properties: {} };
  } catch (err) {
    loadError.value = extractErrorMessage(err, t('admin-user-fields-error-load'));
  } finally {
    isLoading.value = false;
  }
});

async function persist(force: boolean): Promise<void> {
  saving.value = true;
  try {
    schema.value = await setUserFieldSchema(schema.value, force);
    conflictCount.value = null;
    toast.success(t('admin-user-fields-saved'));
  } catch (err) {
    const response = (err as { response?: { status?: number; data?: { invalid_count?: number } } })
      .response;
    if (response?.status === 409) {
      // Existing profile values would be invalidated; confirm before forcing.
      conflictCount.value = response.data?.invalid_count ?? 0;
      return;
    }
    toast.error(extractErrorMessage(err, t('admin-user-fields-error-save')));
  } finally {
    saving.value = false;
  }
}

const save = () => persist(false);
const confirmForce = () => persist(true);
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <div class="flex flex-col gap-2">
        <BackButton :fallback-route="'/admin'" :label="t('admin-user-fields-back-label')" compact />
        <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
          <div class="flex flex-col gap-1">
            <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-user-fields-title') }}</h1>
            <p class="text-secondary text-sm sm:text-base">{{ t('admin-user-fields-description') }}</p>
          </div>
          <Button :loading="saving" class="self-start sm:self-auto" @click="save">
            {{ t('common-save') }}
          </Button>
        </div>
      </div>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />

      <div v-if="!isLoading" class="bg-surface border border-default rounded-lg p-4">
        <AttributeEditor v-model="schema" />
      </div>
    </div>

    <ConfirmModal
      :show="conflictCount !== null"
      variant="warning"
      :title="t('admin-user-fields-conflict-title')"
      :message="t('admin-user-fields-conflict-message', { count: conflictCount ?? 0 })"
      :confirm-label="t('admin-user-fields-conflict-confirm')"
      :loading="saving"
      @confirm="confirmForce"
      @close="conflictCount = null"
    />
  </div>
</template>
