<script setup lang="ts">
/**
 * How this workspace sends: the server default, its own domain, or its own
 * SMTP server. Choosing an option opens its settings; the workspace switches
 * only when the admin saves or presses the option's "use" button, and every
 * option keeps its settings when another is in use.
 */
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';
import StatusPill from '@/components/common/StatusPill.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import ServerDefaultPanel from './ServerDefaultPanel.vue';
import SendingDomainPanel from './SendingDomainPanel.vue';
import RelayServerForm from './RelayServerForm.vue';
import DnsHealth from './DnsHealth.vue';
import EmailTestResult from './EmailTestResult.vue';
import workspaceEmailService, {
  type EmailTestResult as TestResult,
  type OutboundSettings,
  type SendingMode,
} from '@nosdesk/core/services/workspaceEmailService';
import { errorStatus, extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const SETTINGS_KEY = ['outbound-email-settings'];
const queryCache = useQueryCache();
const settingsQuery = useQuery({
  key: SETTINGS_KEY,
  query: () => workspaceEmailService.get(),
});
const serverQuery = useQuery({
  key: ['email-config'],
  query: () => workspaceEmailService.getServerConfig(),
});
const settings = computed(() => settingsQuery.data.value ?? null);
const server = computed(() => serverQuery.data.value ?? null);
const managed = computed(() => !!server.value?.managed);
const active = computed<SendingMode | null>(() => settings.value?.sending_mode ?? null);

const loadError = computed(() => {
  const e = settingsQuery.error.value ?? serverQuery.error.value;
  return e ? extractErrorMessage(e, t('email-domain-error-load')) : '';
});

// Opens on the option in use; after that it follows the admin.
const selected = ref<SendingMode | null>(null);
watch(
  active,
  (mode) => {
    if (mode && !selected.value) selected.value = mode;
  },
  { immediate: true },
);

const options = computed<Array<{ value: SendingMode; icon: IconName; title: string; body: string }>>(() => [
  {
    value: 'fallback',
    icon: 'email',
    title: t('email-mode-default-title'),
    body: managed.value ? t('email-mode-default-body-managed') : t('email-mode-default-body'),
  },
  {
    value: 'verified_domain',
    icon: 'at',
    title: t('email-mode-domain-title'),
    body: managed.value ? t('email-mode-domain-body-managed') : t('email-mode-domain-body'),
  },
  {
    value: 'smtp_relay',
    icon: 'server',
    title: t('email-mode-relay-title'),
    body: t('email-mode-relay-body'),
  },
]);

function refresh() {
  settingsQuery.refetch();
  serverQuery.refetch();
  testResult.value = null;
}

function onRelaySaved(saved: OutboundSettings) {
  queryCache.setQueryData(SETTINGS_KEY, saved);
  serverQuery.refetch();
  testResult.value = null;
}

// Test whatever the workspace sends with now.
const testing = ref(false);
const testError = ref('');
const testResult = ref<TestResult | null>(null);
async function sendTest() {
  testError.value = '';
  testResult.value = null;
  testing.value = true;
  try {
    testResult.value = await workspaceEmailService.sendTest();
  } catch (e) {
    testError.value =
      errorStatus(e) === 429
        ? t('email-relay-error-rate-limited')
        : extractErrorMessage(e, t('email-domain-error-test'));
  } finally {
    testing.value = false;
  }
}
</script>

<template>
  <section class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-6">
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">{{ t('email-mode-heading') }}</h2>
      <p class="text-sm text-secondary">{{ t('email-mode-subtitle') }}</p>
    </div>

    <AlertMessage v-if="loadError && !settings" type="error" :message="loadError" />

    <RadioGroupRoot
      v-model="selected"
      :aria-label="t('email-mode-heading')"
      class="grid grid-cols-1 gap-3 md:grid-cols-3"
    >
      <RadioGroupItem
        v-for="opt in options"
        :key="opt.value"
        :value="opt.value"
        class="group flex flex-col gap-2 rounded-lg border p-4 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-default hover:bg-surface-hover data-[state=checked]:border-accent data-[state=checked]:bg-accent/5"
      >
        <div class="flex items-center gap-2">
          <Icon :name="opt.icon" size="sm" class="text-tertiary group-data-[state=checked]:text-accent" />
          <span class="flex-1 text-sm font-medium text-primary">{{ opt.title }}</span>
          <StatusPill
            v-if="active === opt.value"
            :label="t('email-mode-in-use')"
            tone="positive"
            size="xs"
            dot
          />
        </div>
        <span class="text-xs text-secondary">{{ opt.body }}</span>
      </RadioGroupItem>
    </RadioGroupRoot>

    <div v-if="settings && selected" class="flex flex-col gap-5">
      <ServerDefaultPanel
        v-if="selected === 'fallback'"
        :config="server"
        :active="active === 'fallback'"
        @changed="refresh"
      />
      <SendingDomainPanel
        v-else-if="selected === 'verified_domain'"
        :key="settings.sending_domain ?? 'none'"
        :settings="settings"
        :active="active === 'verified_domain'"
        :managed="managed"
        @changed="refresh"
      />
      <template v-else>
        <RelayServerForm
          :settings="settings"
          :active="active === 'smtp_relay'"
          :managed="managed"
          @saved="onRelaySaved"
        />
        <DnsHealth
          v-if="active === 'smtp_relay'"
          :description="t('email-relay-dns-description')"
        />
      </template>
    </div>

    <div class="flex flex-col gap-3 border-t border-default pt-5">
      <div class="flex items-center justify-between gap-3 flex-wrap">
        <div class="flex flex-col gap-0.5">
          <span class="text-sm font-medium text-primary">{{ t('email-test-heading') }}</span>
          <span class="text-xs text-tertiary">{{ t('email-test-description') }}</span>
        </div>
        <Button variant="secondary" size="sm" icon="send" :loading="testing" @click="sendTest">
          {{ t('email-domain-test-button') }}
        </Button>
      </div>
      <AlertMessage v-if="testError" type="error" :message="testError" />
      <EmailTestResult v-if="testResult" :result="testResult" :managed="managed" />
    </div>
  </section>
</template>
