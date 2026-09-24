<script setup lang="ts">
/**
 * The workspace's own SMTP server. "Test and use" sends a test to the admin
 * first and saves only when the server accepts it; a failed test offers
 * "Save anyway" for servers that are right but refuse the admin's address.
 * A test counts only for the exact settings it ran with.
 */
import { computed, reactive, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import FormInput from '@/components/common/FormInput.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import PasswordInput from '@/components/common/PasswordInput.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import EmailTestResult from './EmailTestResult.vue';
import {
  SMTP_PRESETS,
  defaultPort,
  portSecurityCheck,
  presetForHost,
  type SmtpPreset,
} from './smtpPresets';
import workspaceEmailService, {
  type EmailTestResult as TestResult,
  type OutboundSettings,
  type RelaySettings,
  type SmtpSecurity,
} from '@nosdesk/core/services/workspaceEmailService';
import { errorCode, errorStatus, extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';

const props = defineProps<{
  settings: OutboundSettings;
  /** The workspace sends through this server now. */
  active: boolean;
  managed?: boolean;
}>();
const emit = defineEmits<{ saved: [settings: OutboundSettings] }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args);
const toast = useToastStore();

const form = reactive({
  host: '',
  port: 587 as number | null,
  security: 'starttls' as SmtpSecurity,
  username: '',
  password: '',
  fromName: '',
  fromEmail: '',
});
const preset = ref<SmtpPreset | undefined>();

function seed(s: OutboundSettings) {
  form.host = s.smtp_host;
  form.port = s.smtp_port || 587;
  form.security = s.smtp_security;
  form.username = s.smtp_username;
  form.password = '';
  form.fromName = s.from_name;
  form.fromEmail = s.from_email;
  preset.value = presetForHost(s.smtp_host);
}
seed(props.settings);

function applyPreset(p: SmtpPreset) {
  preset.value = p;
  if (p.host !== undefined) form.host = p.host;
  if (p.port !== undefined) form.port = p.port;
  if (p.security) form.security = p.security;
}

// Changing security moves the port along when it was a standard one.
watch(
  () => form.security,
  (security) => {
    if (form.port === null || [25, 465, 587].includes(form.port)) {
      if (security !== 'plaintext') form.port = defaultPort(security);
      else if (form.port === 465) form.port = 587;
    }
  },
);

const securityOptions = computed(() => [
  { value: 'starttls' as const, label: 'STARTTLS' },
  { value: 'tls' as const, label: 'TLS' },
  { value: 'plaintext' as const, label: t('email-relay-security-none') },
]);

const portCheck = computed(() => portSecurityCheck(form.port, form.security));

const payload = computed<RelaySettings>(() => ({
  from_name: form.fromName.trim(),
  from_email: form.fromEmail.trim(),
  smtp_host: form.host.trim(),
  smtp_port: form.port ?? 0,
  smtp_security: form.security,
  smtp_username: form.username.trim(),
  password: form.password || undefined,
}));
const complete = computed(
  () =>
    !!payload.value.smtp_host &&
    !!payload.value.from_email &&
    !!form.port &&
    portCheck.value.level !== 'error',
);

// Server-side refusals mapped to the field at fault.
const fieldErrors = reactive<Record<string, string>>({});
const formError = ref('');
const FIELD_OF: Record<string, string> = {
  RELAY_HOST_INVALID: 'host',
  RELAY_PORT_INVALID: 'port',
  SMTP_CONFIG_MISMATCH: 'port',
  RELAY_SECURITY_INVALID: 'port',
  FROM_EMAIL_INVALID: 'fromEmail',
  RELAY_PASSWORD_REQUIRED: 'password',
};
const MESSAGE_OF: Record<string, string> = {
  RELAY_HOST_INVALID: 'email-relay-error-host',
  RELAY_PORT_INVALID: 'email-relay-error-port',
  SMTP_CONFIG_MISMATCH: 'email-relay-error-port-security',
  RELAY_SECURITY_INVALID: 'email-relay-error-port-security',
  FROM_EMAIL_INVALID: 'email-relay-error-from',
  RELAY_PASSWORD_REQUIRED: 'email-relay-error-password-required',
};

function clearErrors() {
  for (const k of Object.keys(fieldErrors)) delete fieldErrors[k];
  formError.value = '';
}

function showError(e: unknown, fallbackKey: string) {
  const code = errorCode(e);
  if (code && FIELD_OF[code]) {
    fieldErrors[FIELD_OF[code]] = t(MESSAGE_OF[code]);
  } else if (errorStatus(e) === 429) {
    formError.value = t('email-relay-error-rate-limited');
  } else {
    formError.value = extractErrorMessage(e, t(fallbackKey));
  }
}

// A result belongs to the settings it ran with; any edit clears it.
const result = ref<TestResult | null>(null);
const testedWith = ref('');
watch(payload, (p) => {
  if (result.value && JSON.stringify(p) !== testedWith.value) result.value = null;
  clearErrors();
});

const testing = ref(false);
const saving = ref(false);
/** Which button started the current run, so only that one spins. */
const running = ref<'use' | 'test' | null>(null);
const busy = computed(() => testing.value || saving.value);

async function runTest(): Promise<TestResult | null> {
  clearErrors();
  testing.value = true;
  const sent = payload.value;
  try {
    const r = await workspaceEmailService.testRelay(sent);
    result.value = r;
    testedWith.value = JSON.stringify(sent);
    return r;
  } catch (e) {
    result.value = null;
    showError(e, 'email-domain-error-test');
    return null;
  } finally {
    testing.value = false;
  }
}

async function save() {
  clearErrors();
  saving.value = true;
  try {
    const saved = await workspaceEmailService.saveRelay(payload.value);
    form.password = '';
    result.value = null;
    toast.success(t(props.active ? 'email-relay-saved' : 'email-relay-now-active'));
    emit('saved', saved);
  } catch (e) {
    showError(e, 'email-relay-error-save');
  } finally {
    saving.value = false;
  }
}

async function testAndSave() {
  running.value = 'use';
  try {
    const r = await runTest();
    if (r?.ok) await save();
  } finally {
    running.value = null;
  }
}

async function testOnly() {
  running.value = 'test';
  try {
    await runTest();
  } finally {
    running.value = null;
  }
}

// Forgetting the stored password keeps the rest of the server.
const removingPassword = ref(false);
async function removePassword() {
  removingPassword.value = true;
  try {
    emit('saved', await workspaceEmailService.removeRelayPassword());
    toast.success(t('email-relay-password-removed'));
  } catch (e) {
    formError.value = extractErrorMessage(e, t('email-relay-error-save'));
  } finally {
    removingPassword.value = false;
  }
}

const passwordPlaceholder = computed(() =>
  props.settings.password_configured ? t('email-relay-password-stored') : '',
);
</script>

<template>
  <form class="flex flex-col gap-5" @submit.prevent="testAndSave">
    <div class="flex flex-col gap-2">
      <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
        {{ t('email-relay-provider-label') }}
      </span>
      <div class="flex flex-wrap gap-1.5">
        <button
          v-for="p in SMTP_PRESETS"
          :key="p.id"
          type="button"
          class="rounded-full border px-3 py-1 text-xs font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          :class="
            preset?.id === p.id
              ? 'border-accent bg-accent/10 text-primary'
              : 'border-default text-secondary hover:text-primary hover:bg-surface-hover'
          "
          :aria-pressed="preset?.id === p.id"
          @click="applyPreset(p)"
        >
          {{ p.id === 'other' ? t('email-relay-preset-other') : p.name }}
        </button>
      </div>
      <p v-if="preset?.helpKey" class="flex items-start gap-1.5 text-xs text-secondary">
        <Icon name="info" size="sm" class="mt-px shrink-0 text-tertiary" />
        <span>{{ t(preset.helpKey) }}</span>
      </p>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-[1fr_8rem]">
      <FormInput
        v-model="form.host"
        :label="t('email-relay-host-label')"
        :placeholder="preset?.hostPlaceholder ?? 'smtp.example.com'"
        :error="fieldErrors.host"
        autocomplete="off"
        spellcheck="false"
        required
      />
      <FormNumber
        v-model="form.port"
        :label="t('email-relay-port-label')"
        :min="1"
        :max="65535"
        integer
        :error="fieldErrors.port || (portCheck.level === 'error' ? t(portCheck.key!) : undefined)"
        required
      />
    </div>

    <div class="flex flex-col gap-1.5">
      <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
        {{ t('email-relay-security-label') }}
      </span>
      <div>
        <SegmentedControl
          v-model="form.security"
          :options="securityOptions"
          :aria-label="t('email-relay-security-label')"
        />
      </div>
      <p v-if="portCheck.level === 'warn'" class="text-xs text-status-warning">
        {{ t(portCheck.key!) }}
      </p>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <FormInput
        v-model="form.username"
        :label="t('email-relay-username-label')"
        autocomplete="off"
        spellcheck="false"
        :description="form.username.trim() ? undefined : t('email-relay-no-username-warning')"
      />
      <div class="flex flex-col gap-1">
        <PasswordInput
          v-model="form.password"
          :label="t('email-relay-password-label')"
          :placeholder="passwordPlaceholder"
          :error="fieldErrors.password"
          autocomplete="new-password"
          :disabled="!form.username.trim()"
        />
        <button
          v-if="settings.password_configured"
          type="button"
          class="self-start text-xs text-tertiary hover:text-status-error disabled:opacity-50"
          :disabled="removingPassword"
          @click="removePassword"
        >
          {{ t('email-relay-password-remove') }}
        </button>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <FormInput
        v-model="form.fromName"
        :label="t('email-domain-from-name-label')"
        :placeholder="t('email-domain-from-name-placeholder')"
      />
      <FormInput
        v-model="form.fromEmail"
        type="email"
        :label="t('email-domain-from-email-label')"
        :placeholder="t('email-domain-from-email-placeholder')"
        :description="t('email-relay-from-help')"
        :error="fieldErrors.fromEmail"
        required
      />
    </div>

    <AlertMessage v-if="formError" type="error" :message="formError" />

    <EmailTestResult v-if="result" :result="result" :managed="managed">
      <div v-if="!result.ok" class="pt-1">
        <Button type="button" variant="secondary" size="sm" :loading="saving" @click="save">
          {{ t('email-relay-save-anyway') }}
        </Button>
      </div>
    </EmailTestResult>

    <div class="flex flex-wrap items-center gap-2">
      <Button type="submit" :loading="running === 'use'" :disabled="!complete || busy">
        {{ active ? t('email-relay-test-and-save') : t('email-relay-test-and-use') }}
      </Button>
      <Button
        type="button"
        variant="secondary"
        :loading="running === 'test'"
        :disabled="!complete || busy"
        @click="testOnly"
      >
        {{ t('email-relay-test-only') }}
      </Button>
      <span class="text-xs text-tertiary">{{ t('email-relay-test-note') }}</span>
    </div>
  </form>
</template>
