<script setup lang="ts">
/**
 * Request acknowledgement: the email a requester gets when a request is filed
 * for them through the portal, the request form or the API. Mail that opens a
 * ticket has its own acknowledgement (EmailAutoAckForm). Workspace-wide
 * (site_settings), on by default. Shares the `branding-config` cache key with
 * the other admin email forms.
 */
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import brandingService, { type AdminBrandingConfig } from '@nosdesk/core/services/brandingService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';

const toast = useToastStore();
const fluent = useFluent();
const t = (key: string) => fluent.$t(key);
const errorMessage = ref('');

const queryCache = useQueryCache();
const BRANDING_KEY = ['branding-config'] as const;
const brandingQuery = useQuery({
  key: BRANDING_KEY,
  query: () => brandingService.getAdminBrandingConfig(),
});
const brandingConfig = computed<AdminBrandingConfig | null>(() => brandingQuery.data.value ?? null);
const loadFailed = computed(() => !!brandingQuery.error.value && !brandingQuery.data.value);

const enabled = ref(true);
const template = ref('');
const saving = ref(false);

// One-shot seed, so a revalidation doesn't clobber an edit in progress.
const seeded = ref(false);
watch(
  brandingQuery.data,
  (data) => {
    if (!data || seeded.value) return;
    enabled.value = data.requester_ack_enabled;
    template.value = data.requester_ack_template ?? '';
    seeded.value = true;
  },
  { immediate: true },
);

const isDirty = computed(() => {
  const cfg = brandingConfig.value;
  if (!cfg) return false;
  return (
    enabled.value !== cfg.requester_ack_enabled ||
    template.value !== (cfg.requester_ack_template ?? '')
  );
});

async function save() {
  if (!isDirty.value) return;
  errorMessage.value = '';
  saving.value = true;
  try {
    const updated = await brandingService.updateBrandingConfig({
      requester_ack_enabled: enabled.value,
      // Empty string clears back to the built-in wording.
      requester_ack_template: template.value,
    });
    queryCache.setQueryData(BRANDING_KEY, updated);
    toast.success(t('admin-requester-ack-success-saved'));
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-requester-ack-error-save'));
    setTimeout(() => { errorMessage.value = ''; }, 5000);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <AlertMessage v-if="loadFailed" type="error" :message="$t('admin-branding-error-load')" />
  <form
    v-else-if="brandingConfig"
    class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6"
    @submit.prevent="save"
  >
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">
        {{ $t('admin-requester-ack-heading') }}
      </h2>
      <p class="text-sm text-secondary">
        {{ $t('admin-requester-ack-subtitle') }}
      </p>
    </div>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <ToggleSwitch
      v-model="enabled"
      :label="$t('admin-requester-ack-toggle-label')"
      :description="$t('admin-requester-ack-toggle-description')"
    />

    <div class="flex flex-col gap-2">
      <FormTextarea
        v-model="template"
        :label="$t('admin-requester-ack-template-label')"
        :placeholder="$t('admin-requester-ack-template-placeholder')"
        :description="$t('admin-requester-ack-template-hint')"
        :rows="4"
        mono
        :disabled="!enabled"
      />
      <p class="text-xs text-tertiary">
        {{ $t('admin-channels-email-auto-ack-variables-hint') }}
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;ticket_id&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;ticket_title&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;customer_name&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;customer_first_name&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;app_name&#125;&#125;</code>
      </p>
      <p class="text-xs text-tertiary">
        {{ $t('admin-requester-ack-unproven-hint') }}
      </p>
    </div>

    <div class="flex justify-end border-t border-default pt-4">
      <Button type="submit" :loading="saving" :disabled="!isDirty">
        {{ saving ? $t('admin-channels-email-auto-ack-saving') : $t('admin-requester-ack-save') }}
      </Button>
    </div>
  </form>
</template>
