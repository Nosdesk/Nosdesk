<script setup lang="ts">
/**
 * Auto-acknowledgement: the reply a customer gets when their email opens a
 * ticket, whichever channel it came in on (IMAP, forwarding, or the hosted
 * managed address). Workspace-wide (site_settings), on by default. Shares the
 * `branding-config` cache key with the branding and security note surfaces,
 * all reading the admin settings, never the public branding.
 */
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import brandingService, { type BrandingConfig } from '@nosdesk/core/services/brandingService';
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
const brandingConfig = computed<BrandingConfig | null>(() => brandingQuery.data.value ?? null);
// The form never shows values it couldn't load.
const loadFailed = computed(() => !!brandingQuery.error.value && !brandingQuery.data.value);

const autoAckEnabled = ref(true);
const autoAckTemplate = ref('');
const savingAutoAck = ref(false);

// One-shot seed from the cached query; later revalidations don't clobber
// in-progress edits.
const autoAckSeeded = ref(false);
watch(
  brandingQuery.data,
  (data) => {
    if (!data || autoAckSeeded.value) return;
    autoAckEnabled.value = data.channel_auto_ack_enabled;
    autoAckTemplate.value = data.channel_auto_ack_template ?? '';
    autoAckSeeded.value = true;
  },
  { immediate: true },
);

const autoAckIsDirty = computed(() => {
  const cfg = brandingConfig.value;
  if (!cfg) return false;
  return (
    autoAckEnabled.value !== cfg.channel_auto_ack_enabled ||
    autoAckTemplate.value !== (cfg.channel_auto_ack_template ?? '')
  );
});

async function saveAutoAck() {
  if (!autoAckIsDirty.value) return;
  errorMessage.value = '';
  savingAutoAck.value = true;
  try {
    const updated = await brandingService.updateBrandingConfig({
      channel_auto_ack_enabled: autoAckEnabled.value,
      // Empty string clears back to the built-in localized default.
      channel_auto_ack_template: autoAckTemplate.value,
    });
    queryCache.setQueryData(BRANDING_KEY, updated);
    toast.success(t('admin-channels-email-auto-ack-success-saved'));
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-email-auto-ack-error-save'));
    setTimeout(() => { errorMessage.value = ''; }, 5000);
  } finally {
    savingAutoAck.value = false;
  }
}
</script>

<template>
  <AlertMessage v-if="loadFailed" type="error" :message="$t('admin-branding-error-load')" />
  <form
    v-else-if="brandingConfig"
    class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6"
    @submit.prevent="saveAutoAck"
  >
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">
        {{ $t('admin-channels-email-auto-ack-heading') }}
      </h2>
      <p class="text-sm text-secondary">
        {{ $t('admin-channels-email-auto-ack-subtitle') }}
      </p>
    </div>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <ToggleSwitch
      v-model="autoAckEnabled"
      :label="$t('admin-channels-email-auto-ack-toggle-label')"
      :description="$t('admin-channels-email-auto-ack-toggle-description')"
    />

    <div class="flex flex-col gap-2">
      <FormTextarea
        v-model="autoAckTemplate"
        :label="$t('admin-channels-email-auto-ack-template-label')"
        :placeholder="$t('admin-channels-email-auto-ack-template-placeholder')"
        :description="$t('admin-channels-email-auto-ack-template-hint')"
        :rows="6"
        mono
        :disabled="!autoAckEnabled"
      />
      <p class="text-xs text-tertiary">
        {{ $t('admin-channels-email-auto-ack-variables-hint') }}
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;ticket_id&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;ticket_title&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;customer_name&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;customer_first_name&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;app_name&#125;&#125;</code>
      </p>
    </div>

    <div class="flex justify-end border-t border-default pt-4">
      <Button type="submit" :loading="savingAutoAck" :disabled="!autoAckIsDirty">
        {{ savingAutoAck ? $t('admin-channels-email-auto-ack-saving') : $t('admin-channels-email-auto-ack-save') }}
      </Button>
    </div>
  </form>
</template>
