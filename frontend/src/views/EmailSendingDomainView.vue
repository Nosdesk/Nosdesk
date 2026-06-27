<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import FormInput from '@/components/common/FormInput.vue';
import Button from '@/components/common/Button.vue';
import workspaceEmailService, {
  type OutboundSettings,
  type EmailAuthReport,
  type RecordCheck,
} from '@nosdesk/core/services/workspaceEmailService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';

// `embedded`: render as a section inside the consolidated Email delivery page
// (no page header / outer padding), vs. the standalone admin route.
const props = withDefaults(defineProps<{ embedded?: boolean }>(), { embedded: false });

const toast = useToastStore();
const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const SETTINGS_KEY = ['outbound-email-settings'] as const;
const settingsQuery = useQuery({
  key: SETTINGS_KEY,
  query: () => workspaceEmailService.get(),
});
const settings = computed<OutboundSettings | null>(() => settingsQuery.data.value ?? null);
const isFirstLoad = computed(
  () => settingsQuery.status.value === 'pending' && settingsQuery.data.value === undefined,
);
const loadError = computed(() =>
  settingsQuery.error.value
    ? extractErrorMessage(settingsQuery.error.value, t('email-domain-error-load'))
    : '',
);

const isVerifiedDomain = computed(() => settings.value?.sending_mode === 'verified_domain');
const isVerified = computed(() => settings.value?.verification_status === 'verified');
const dkimRecord = computed(() => settings.value?.dkim_record ?? null);

// Setup form (shown until a domain is configured). Seeded from any stored
// identity so re-opening preserves what was entered.
const fromName = ref('');
const fromEmail = ref('');
const seeded = ref(false);
watch(
  settings,
  (s) => {
    if (!s || seeded.value) return;
    fromName.value = s.from_name;
    fromEmail.value = s.from_email;
    seeded.value = true;
  },
  { immediate: true },
);

const actionError = ref('');
const saving = ref(false);
const verifying = ref(false);
const testing = ref(false);
const removing = ref(false);
const checkingDns = ref(false);
const dnsReport = ref<EmailAuthReport | null>(null);

// SPF/DKIM/DMARC/MX checks in display order.
const dnsChecks = computed<Array<{ key: string; label: string; check: RecordCheck }>>(() => {
  const r = dnsReport.value;
  if (!r) return [];
  return [
    { key: 'dkim', label: t('email-domain-dns-dkim'), check: r.dkim },
    { key: 'dmarc', label: t('email-domain-dns-dmarc'), check: r.dmarc },
    { key: 'spf', label: t('email-domain-dns-spf'), check: r.spf },
    { key: 'mx', label: t('email-domain-dns-mx'), check: r.mx },
  ];
});

function dnsStatusClass(status: RecordCheck['status']): string {
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

async function setupDomain() {
  if (!fromEmail.value.trim()) return;
  actionError.value = '';
  saving.value = true;
  try {
    await workspaceEmailService.setDomain({
      from_name: fromName.value.trim(),
      from_email: fromEmail.value.trim(),
    });
    await settingsQuery.refetch();
    toast.success(t('email-domain-setup-success'));
  } catch (error) {
    actionError.value = extractErrorMessage(error, t('email-domain-error-setup'));
  } finally {
    saving.value = false;
  }
}

async function verify() {
  actionError.value = '';
  verifying.value = true;
  try {
    const { verification_status } = await workspaceEmailService.verify();
    await settingsQuery.refetch();
    if (verification_status === 'verified') {
      toast.success(t('email-domain-verified-success'));
    } else {
      toast.info(t('email-domain-pending-still'));
    }
  } catch (error) {
    actionError.value = extractErrorMessage(error, t('email-domain-error-verify'));
  } finally {
    verifying.value = false;
  }
}

async function sendTest() {
  actionError.value = '';
  testing.value = true;
  try {
    const { to } = await workspaceEmailService.sendTest();
    toast.success(t('email-domain-test-sent') + ' ' + to);
  } catch (error) {
    actionError.value = extractErrorMessage(error, t('email-domain-error-test'));
  } finally {
    testing.value = false;
  }
}

async function runDnsCheck() {
  actionError.value = '';
  checkingDns.value = true;
  try {
    dnsReport.value = await workspaceEmailService.dnsCheck();
  } catch (error) {
    actionError.value = extractErrorMessage(error, t('email-domain-error-dns-check'));
  } finally {
    checkingDns.value = false;
  }
}

async function remove() {
  actionError.value = '';
  removing.value = true;
  try {
    await workspaceEmailService.reset();
    await settingsQuery.refetch();
    seeded.value = false;
    toast.success(t('email-domain-removed'));
  } catch (error) {
    actionError.value = extractErrorMessage(error, t('email-domain-error-remove'));
  } finally {
    removing.value = false;
  }
}

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
  <div :class="props.embedded ? 'flex flex-col gap-6' : 'max-w-2xl mx-auto p-6 flex flex-col gap-6'">
    <header v-if="!props.embedded" class="flex flex-col gap-1">
      <h1 class="text-xl font-semibold text-primary">{{ t('email-domain-title') }}</h1>
      <p class="text-sm text-tertiary">{{ t('email-domain-description') }}</p>
    </header>

    <AlertMessage v-if="loadError" type="error" :message="loadError" />
    <AlertMessage v-if="actionError" type="error" :message="actionError" />

    <div v-if="isFirstLoad" class="flex items-center gap-2 text-tertiary text-sm">
      <Spinner /> {{ t('email-domain-loading') }}
    </div>

    <!-- Setup form: no verified domain configured yet -->
    <section
      v-else-if="!isVerifiedDomain"
      class="flex flex-col gap-4 p-5 rounded-lg border border-default bg-surface"
    >
      <p class="text-sm text-secondary">{{ t('email-domain-setup-intro') }}</p>
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
      />
      <div>
        <Button :loading="saving" :disabled="!fromEmail.trim()" @click="setupDomain">
          {{ t('email-domain-setup-button') }}
        </Button>
      </div>
    </section>

    <!-- Configured: show identity, status, the DNS record, and actions -->
    <section v-else class="flex flex-col gap-5 p-5 rounded-lg border border-default bg-surface">
      <div class="flex items-center justify-between gap-3 flex-wrap">
        <div class="text-sm text-secondary">
          <span class="font-medium text-primary">{{ settings?.from_name }}</span>
          &lt;{{ settings?.from_email }}&gt;
        </div>
        <span
          class="px-2 py-0.5 text-xs rounded-full border"
          :class="
            isVerified
              ? 'bg-status-success/20 text-status-success border-status-success/50'
              : 'bg-status-warning/20 text-status-warning border-status-warning/50'
          "
        >
          {{ isVerified ? t('email-domain-status-verified') : t('email-domain-status-pending') }}
        </span>
      </div>

      <div v-if="dkimRecord" class="flex flex-col gap-3">
        <p class="text-sm text-secondary">{{ t('email-domain-record-instructions') }}</p>
        <div class="flex flex-col gap-2">
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary">{{ t('email-domain-record-name-label') }}</span>
            <div class="flex items-center gap-2">
              <code class="flex-1 font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ dkimRecord.name }}</code>
              <Button
                variant="ghost"
                size="sm"
                icon="copy"
                :ariaLabel="t('email-domain-copy')"
                @click="copy(dkimRecord!.name)"
              />
            </div>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary">{{ t('email-domain-record-value-label') }}</span>
            <div class="flex items-start gap-2">
              <code class="flex-1 font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ dkimRecord.txt_value }}</code>
              <Button
                variant="ghost"
                size="sm"
                icon="copy"
                :ariaLabel="t('email-domain-copy')"
                @click="copy(dkimRecord!.txt_value)"
              />
            </div>
          </div>
        </div>
        <p v-if="!isVerified" class="text-xs text-tertiary flex items-center gap-1">
          <Icon name="info" class="w-3.5 h-3.5" />
          {{ t('email-domain-dns-propagation-note') }}
        </p>
      </div>

      <!-- DNS health: live SPF/DKIM/DMARC/MX readout for self-diagnosis. -->
      <div class="flex flex-col gap-3 pt-1 border-t border-default">
        <div class="flex items-center justify-between gap-3 flex-wrap pt-3">
          <div class="flex flex-col gap-0.5">
            <span class="text-sm font-medium text-primary">{{ t('email-domain-dns-title') }}</span>
            <span class="text-xs text-tertiary">{{ t('email-domain-dns-description') }}</span>
          </div>
          <Button variant="secondary" size="sm" :loading="checkingDns" @click="runDnsCheck">
            {{ t('email-domain-dns-check-button') }}
          </Button>
        </div>

        <ul v-if="dnsReport" class="flex flex-col gap-2">
          <li
            v-for="row in dnsChecks"
            :key="row.key"
            class="flex items-start gap-3 p-2.5 rounded-lg bg-surface-alt"
          >
            <span
              class="mt-0.5 px-2 py-0.5 text-xs font-medium rounded-full border uppercase shrink-0 w-16 text-center"
              :class="dnsStatusClass(row.check.status)"
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

      <div class="flex items-center gap-2 flex-wrap">
        <Button v-if="!isVerified" :loading="verifying" @click="verify">
          {{ t('email-domain-verify-button') }}
        </Button>
        <Button v-if="isVerified" variant="secondary" :loading="testing" @click="sendTest">
          {{ t('email-domain-test-button') }}
        </Button>
        <Button variant="ghost-danger" :loading="removing" @click="remove">
          {{ t('email-domain-remove-button') }}
        </Button>
      </div>
    </section>
  </div>
</template>
