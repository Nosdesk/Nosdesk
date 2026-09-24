<script setup lang="ts">
/**
 * The server default. Self-hosted: the SMTP settings in the server's
 * environment, read-only here. Hosted: Nosdesk's managed sending, whose relay
 * is never shown.
 */
import { ref } from 'vue';
import { useFluent } from 'fluent-vue';
import Button from '@/components/common/Button.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import workspaceEmailService, {
  type ServerEmailConfig,
} from '@nosdesk/core/services/workspaceEmailService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';

defineProps<{
  config: ServerEmailConfig | null;
  active: boolean;
}>();
const emit = defineEmits<{ changed: [] }>();

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);
const toast = useToastStore();

const ENV_VARS = [
  'SMTP_ENABLED',
  'SMTP_HOST',
  'SMTP_PORT',
  'SMTP_USERNAME',
  'SMTP_PASSWORD',
  'SMTP_FROM_NAME',
  'SMTP_FROM_EMAIL',
];

const error = ref('');
const switching = ref(false);
async function use() {
  error.value = '';
  switching.value = true;
  try {
    await workspaceEmailService.setMode('fallback');
    emit('changed');
    toast.success(t('email-default-now-active'));
  } catch (e) {
    error.value = extractErrorMessage(e, t('email-mode-error-switch'));
  } finally {
    switching.value = false;
  }
}

const from = (c: ServerEmailConfig) =>
  c.from_name ? `${c.from_name} <${c.from_email}>` : c.from_email;
</script>

<template>
  <div class="flex flex-col gap-4">
    <AlertMessage v-if="error" type="error" :message="error" />

    <!-- Hosted: managed sending. -->
    <template v-if="config?.managed">
      <p class="text-sm text-secondary">{{ t('email-default-managed-note') }}</p>
      <div v-if="active && config.from_email" class="flex flex-col gap-1">
        <span class="text-xs text-tertiary">{{ t('email-domain-from-email-label') }}</span>
        <code class="w-fit max-w-full font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ from(config) }}</code>
      </div>
    </template>

    <!-- Self-hosted: the environment's relay. -->
    <template v-else-if="config">
      <p class="text-sm text-secondary">{{ t('email-default-env-note') }}</p>
      <dl
        v-if="config.is_configured"
        class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm"
      >
        <dt class="text-tertiary">{{ t('email-relay-host-label') }}</dt>
        <dd class="min-w-0 font-mono text-xs text-primary break-all self-center">
          {{ config.smtp_host }}:{{ config.smtp_port }}
        </dd>
        <dt class="text-tertiary">{{ t('email-domain-from-email-label') }}</dt>
        <dd class="min-w-0 font-mono text-xs text-primary break-all self-center">{{ from(config) }}</dd>
        <dt class="text-tertiary">{{ t('email-relay-password-label') }}</dt>
        <dd>
          <StatusPill
            :label="
              config.smtp_password_configured
                ? t('email-default-password-set')
                : t('email-default-password-not-set')
            "
            :tone="config.smtp_password_configured ? 'positive' : 'caution'"
            size="xs"
          />
        </dd>
      </dl>
      <p
        v-else
        class="rounded-lg border border-status-warning/40 bg-status-warning/10 p-3 text-sm text-primary"
      >
        {{ t('email-default-not-configured') }}
      </p>
      <div class="flex flex-col gap-1.5 text-xs">
        <span class="text-tertiary">{{ t('email-default-env-vars') }}</span>
        <div class="flex flex-wrap gap-1">
          <code
            v-for="v in ENV_VARS"
            :key="v"
            class="bg-surface-alt text-secondary px-1.5 py-0.5 rounded"
          >{{ v }}</code>
        </div>
      </div>
    </template>

    <div v-if="!active">
      <Button :loading="switching" @click="use">{{ t('email-default-use-button') }}</Button>
    </div>
  </div>
</template>
