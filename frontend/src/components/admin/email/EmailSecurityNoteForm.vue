<script setup lang="ts">
/**
 * Anti-phishing security note: footer copy for transactional mail (password
 * reset, invitation). Workspace-wide (site_settings), off by default. Shares
 * the `branding-config` cache key with the branding and auto-ack surfaces.
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
  query: () => brandingService.getBrandingConfig(),
});
const brandingConfig = computed<BrandingConfig | null>(() => brandingQuery.data.value ?? null);

const securityNoteEnabled = ref(false);
const securityNoteTemplate = ref('');
const savingSecurityNote = ref(false);

// One-shot seed from the cached query; later revalidations don't clobber
// in-progress edits.
const securityNoteSeeded = ref(false);
watch(
  brandingQuery.data,
  (data) => {
    if (!data || securityNoteSeeded.value) return;
    securityNoteEnabled.value = data.email_security_note_enabled;
    securityNoteTemplate.value = data.email_security_note_template ?? '';
    securityNoteSeeded.value = true;
  },
  { immediate: true },
);

const securityNoteIsDirty = computed(() => {
  const cfg = brandingConfig.value;
  if (!cfg) return false;
  return (
    securityNoteEnabled.value !== cfg.email_security_note_enabled ||
    securityNoteTemplate.value !== (cfg.email_security_note_template ?? '')
  );
});

async function saveSecurityNote() {
  if (!securityNoteIsDirty.value) return;
  errorMessage.value = '';
  savingSecurityNote.value = true;
  try {
    const updated = await brandingService.updateBrandingConfig({
      email_security_note_enabled: securityNoteEnabled.value,
      // Empty string clears back to the built-in localized default.
      email_security_note_template: securityNoteTemplate.value,
    });
    queryCache.setQueryData(BRANDING_KEY, updated);
    toast.success(t('admin-email-security-note-success-saved'));
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('admin-email-security-note-error-save'));
    setTimeout(() => { errorMessage.value = ''; }, 5000);
  } finally {
    savingSecurityNote.value = false;
  }
}
</script>

<template>
  <form
    v-if="brandingConfig"
    class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6"
    @submit.prevent="saveSecurityNote"
  >
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">
        {{ $t('admin-email-security-note-heading') }}
      </h2>
      <p class="text-sm text-secondary">
        {{ $t('admin-email-security-note-subtitle') }}
      </p>
    </div>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

    <ToggleSwitch
      v-model="securityNoteEnabled"
      :label="$t('admin-email-security-note-toggle-label')"
      :description="$t('admin-email-security-note-toggle-description')"
    />

    <div class="flex flex-col gap-2">
      <FormTextarea
        v-model="securityNoteTemplate"
        :label="$t('admin-email-security-note-template-label')"
        :placeholder="$t('admin-email-security-note-template-placeholder')"
        :description="$t('admin-email-security-note-template-hint')"
        :rows="4"
        mono
        :disabled="!securityNoteEnabled"
      />
      <p class="text-xs text-tertiary">
        {{ $t('admin-email-security-note-variables-hint') }}
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;brand_name&#125;&#125;</code>,
        <code class="text-3xs bg-surface-alt px-1 rounded">&#123;&#123;domain&#125;&#125;</code>
      </p>
    </div>

    <div class="flex justify-end border-t border-default pt-4">
      <Button type="submit" :loading="savingSecurityNote" :disabled="!securityNoteIsDirty">
        {{ savingSecurityNote ? $t('admin-email-security-note-saving') : $t('admin-email-security-note-save') }}
      </Button>
    </div>
  </form>
</template>
