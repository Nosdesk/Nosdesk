<script setup lang="ts">
/**
 * Verified sending domain: mail goes through the server's relay, signed with
 * a DKIM key for the From domain. Setting a domain up (again) makes a new key,
 * so a domain that is already set up is switched to, not re-entered.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import FormInput from '@/components/common/FormInput.vue';
import Button from '@/components/common/Button.vue';
import IconButton from '@/components/common/IconButton.vue';
import Icon from '@/components/common/Icon.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import DnsHealth from './DnsHealth.vue';
import workspaceEmailService, {
  type OutboundSettings,
} from '@nosdesk/core/services/workspaceEmailService';
import { errorCode, extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';

const props = defineProps<{
  settings: OutboundSettings;
  active: boolean;
  managed?: boolean;
}>();
const emit = defineEmits<{ changed: [] }>();

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);
const toast = useToastStore();

const hasDomain = computed(() => !!props.settings.sending_domain && !!props.settings.dkim_record);
const verified = computed(() => props.settings.verification_status === 'verified');
const record = computed(() => props.settings.dkim_record);

// Shown with no domain yet, or after the From address moved off the domain.
const fromName = ref(props.settings.from_name);
const fromEmail = ref(props.settings.from_email);
const redo = ref(false);
const showForm = computed(() => !hasDomain.value || redo.value);

const error = ref('');
const busy = ref<'setup' | 'verify' | 'use' | 'remove' | null>(null);

async function run(kind: NonNullable<typeof busy.value>, fn: () => Promise<void>, fallbackKey: string) {
  error.value = '';
  busy.value = kind;
  try {
    await fn();
  } catch (e) {
    if (errorCode(e) === 'MODE_DOMAIN_MISMATCH') {
      error.value = t('email-domain-error-from-moved');
      redo.value = true;
    } else {
      error.value = extractErrorMessage(e, t(fallbackKey));
    }
  } finally {
    busy.value = null;
  }
}

const setup = () =>
  run(
    'setup',
    async () => {
      await workspaceEmailService.setDomain({
        from_name: fromName.value.trim(),
        from_email: fromEmail.value.trim(),
      });
      redo.value = false;
      emit('changed');
      toast.success(t('email-domain-setup-success'));
    },
    'email-domain-error-setup',
  );

const verify = () =>
  run(
    'verify',
    async () => {
      const { verification_status } = await workspaceEmailService.verify();
      emit('changed');
      if (verification_status === 'verified') toast.success(t('email-domain-verified-success'));
      else toast.info(t('email-domain-pending-still'));
    },
    'email-domain-error-verify',
  );

const use = () =>
  run(
    'use',
    async () => {
      await workspaceEmailService.setMode('verified_domain');
      emit('changed');
      toast.success(t('email-domain-now-active'));
    },
    'email-mode-error-switch',
  );

const confirmingRemove = ref(false);
const remove = () =>
  run(
    'remove',
    async () => {
      confirmingRemove.value = false;
      await workspaceEmailService.reset();
      emit('changed');
      toast.success(t('email-domain-removed'));
    },
    'email-domain-error-remove',
  );

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success(t('email-domain-copied'));
  } catch {
    /* clipboard denied; the value is select-all so the admin can copy manually */
  }
}
</script>

<template>
  <div class="flex flex-col gap-5">
    <AlertMessage v-if="error" type="error" :message="error" />

    <form v-if="showForm" class="flex flex-col gap-4" @submit.prevent="setup">
      <p class="text-sm text-secondary">
        {{ managed ? t('email-domain-setup-intro') : t('email-domain-setup-intro-self-hosted') }}
      </p>
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <FormInput
          v-model="fromName"
          :label="t('email-domain-from-name-label')"
          :placeholder="t('email-domain-from-name-placeholder')"
        />
        <FormInput
          v-model="fromEmail"
          type="email"
          :label="t('email-domain-from-email-label')"
          :placeholder="t('email-domain-from-email-placeholder')"
          :description="t('email-domain-from-email-help')"
          required
        />
      </div>
      <div class="flex flex-wrap gap-2">
        <Button type="submit" :loading="busy === 'setup'" :disabled="!fromEmail.trim()">
          {{ t('email-domain-setup-button') }}
        </Button>
        <Button v-if="redo" type="button" variant="ghost" @click="redo = false">
          {{ t('email-mode-cancel') }}
        </Button>
      </div>
    </form>

    <template v-else>
      <div class="flex items-center justify-between gap-3 flex-wrap">
        <div class="text-sm text-secondary min-w-0 break-all">
          <span class="font-medium text-primary">{{ settings.from_name }}</span>
          &lt;{{ settings.from_email }}&gt;
        </div>
        <StatusPill
          :label="verified ? t('email-domain-status-verified') : t('email-domain-status-pending')"
          :tone="verified ? 'positive' : 'caution'"
          dot
        />
      </div>

      <p v-if="active && !verified" class="text-sm text-secondary">
        {{ t('email-domain-pending-fallback-note') }}
      </p>

      <div v-if="record" class="flex flex-col gap-3">
        <p class="text-sm text-secondary">{{ t('email-domain-record-instructions') }}</p>
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary">{{ t('email-domain-record-name-label') }}</span>
          <div class="flex items-center gap-2">
            <code class="flex-1 font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ record.name }}</code>
            <IconButton size="sm" icon="copy" :label="t('email-domain-copy')" @click="copy(record.name)" />
          </div>
        </div>
        <div class="flex flex-col gap-1">
          <span class="text-xs text-tertiary">{{ t('email-domain-record-value-label') }}</span>
          <div class="flex items-start gap-2">
            <code class="flex-1 font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ record.txt_value }}</code>
            <IconButton size="sm" icon="copy" :label="t('email-domain-copy')" @click="copy(record.txt_value)" />
          </div>
        </div>
        <p v-if="!verified" class="text-xs text-tertiary flex items-center gap-1">
          <Icon name="info" size="sm" />
          {{ t('email-domain-dns-propagation-note') }}
        </p>
      </div>

      <div class="flex items-center gap-2 flex-wrap">
        <Button v-if="!active" :loading="busy === 'use'" @click="use">
          {{ t('email-domain-use-button') }}
        </Button>
        <Button
          v-if="!verified"
          :variant="active ? 'primary' : 'secondary'"
          :loading="busy === 'verify'"
          @click="verify"
        >
          {{ t('email-domain-verify-button') }}
        </Button>
        <Button variant="ghost-danger" :loading="busy === 'remove'" @click="confirmingRemove = true">
          {{ t('email-domain-remove-button') }}
        </Button>
      </div>

      <DnsHealth v-if="active" :description="t('email-domain-dns-description')" />
    </template>

    <ConfirmModal
      :show="confirmingRemove"
      :title="t('email-domain-remove-confirm-title')"
      :message="t('email-domain-remove-confirm-message')"
      :confirm-label="t('email-domain-remove-button')"
      variant="danger"
      @confirm="remove"
      @close="confirmingRemove = false"
    />
  </div>
</template>
