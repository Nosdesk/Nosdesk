<script setup lang="ts">
/** Live SPF / DKIM / DMARC / MX readout for the domain the workspace sends from. */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import workspaceEmailService, {
  type EmailAuthReport,
  type RecordCheck,
} from '@nosdesk/core/services/workspaceEmailService';
import { extractErrorMessage } from '@/utils/errors';

defineProps<{
  /** Shown under the title, e.g. which include SPF needs. */
  description: string;
}>();

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const checking = ref(false);
const error = ref('');
const report = ref<EmailAuthReport | null>(null);

const rows = computed<Array<{ key: string; label: string; check: RecordCheck }>>(() => {
  const r = report.value;
  if (!r) return [];
  return [
    { key: 'spf', label: t('email-domain-dns-spf'), check: r.spf },
    { key: 'dkim', label: t('email-domain-dns-dkim'), check: r.dkim },
    { key: 'dmarc', label: t('email-domain-dns-dmarc'), check: r.dmarc },
    { key: 'mx', label: t('email-domain-dns-mx'), check: r.mx },
  ];
});

function statusClass(status: RecordCheck['status']): string {
  switch (status) {
    case 'pass':
      return 'bg-status-success/20 text-status-success border-status-success/50';
    case 'fail':
      return 'bg-status-error/20 text-status-error border-status-error/50';
    case 'warn':
      return 'bg-status-warning/20 text-status-warning border-status-warning/50';
    default:
      return 'bg-surface-alt text-tertiary border-default';
  }
}

async function run() {
  error.value = '';
  checking.value = true;
  try {
    report.value = await workspaceEmailService.dnsCheck();
  } catch (e) {
    error.value = extractErrorMessage(e, t('email-domain-error-dns-check'));
  } finally {
    checking.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-3 border-t border-default pt-4">
    <div class="flex items-center justify-between gap-3 flex-wrap">
      <div class="flex flex-col gap-0.5">
        <span class="text-sm font-medium text-primary">{{ t('email-domain-dns-title') }}</span>
        <span class="text-xs text-tertiary">{{ description }}</span>
      </div>
      <Button variant="secondary" size="sm" :loading="checking" @click="run">
        {{ t('email-domain-dns-check-button') }}
      </Button>
    </div>
    <AlertMessage v-if="error" type="error" :message="error" />
    <ul v-if="report" class="flex flex-col gap-2">
      <li
        v-for="row in rows"
        :key="row.key"
        class="flex items-start gap-3 p-2.5 rounded-lg bg-surface-alt"
      >
        <span
          class="mt-0.5 px-2 py-0.5 text-xs font-medium rounded-full border uppercase shrink-0 w-16 text-center"
          :class="statusClass(row.check.status)"
        >
          {{ row.check.status }}
        </span>
        <div class="flex flex-col gap-0.5 min-w-0">
          <span class="text-sm text-primary font-medium">{{ row.label }}</span>
          <span class="text-xs text-secondary">{{ row.check.summary }}</span>
          <code
            v-if="row.check.value"
            class="mt-1 font-mono text-xs text-tertiary break-all select-all"
            >{{ row.check.value }}</code
          >
        </div>
      </li>
    </ul>
  </div>
</template>
