<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <!-- Header -->
      <header class="flex flex-wrap items-start gap-3">
        <div class="flex flex-col gap-1 min-w-0">
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('admin-ldap-title') }}
          </h1>
          <p class="text-secondary text-sm">{{ $t('admin-ldap-subtitle') }}</p>
        </div>
        <div class="ml-auto flex items-center gap-2">
          <StatusPill
            v-if="hasLoadedData"
            :label="savedEnabled ? $t('admin-ldap-status-enabled') : $t('admin-ldap-status-disabled')"
            :tone="savedEnabled ? 'positive' : 'neutral'"
            size="sm"
          />
        </div>
      </header>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Cold-load skeleton (only on a genuine cold cache) -->
      <Skeleton v-if="isFirstLoad" class="flex flex-col gap-4">
        <SkeletonBar v-for="n in 3" :key="n" class="h-40 rounded-xl" />
      </Skeleton>

      <template v-else>
        <!-- On/off + provider preset -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-general') }}</template>
          <div class="flex flex-col gap-5">
            <ToggleSwitch
              v-model="form.enabled"
              :label="$t('admin-ldap-enabled-label')"
              :description="$t('admin-ldap-enabled-desc')"
            />
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <BaseDropdown
                :model-value="selectedPreset"
                :options="presetOptions"
                :label="$t('admin-ldap-preset-label')"
                :description="$t('admin-ldap-preset-desc')"
                :placeholder="$t('admin-ldap-preset-placeholder')"
                size="md"
                @update:model-value="applyPreset"
              />
            </div>
          </div>
        </SectionCard>

        <!-- Connection -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-connection') }}</template>
          <div class="flex flex-col gap-5">
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
              <FormInput
                v-model="form.host"
                class="sm:col-span-2"
                :label="$t('admin-ldap-host-label')"
                :placeholder="$t('admin-ldap-host-placeholder')"
                :description="$t('admin-ldap-host-desc')"
                required
              />
              <FormNumber
                v-model="form.port"
                :label="$t('admin-ldap-port-label')"
                :min="1"
                :max="65535"
                integer
              />
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 items-end">
              <div class="flex flex-col gap-1.5">
                <label class="text-xs font-medium text-tertiary uppercase">
                  {{ $t('admin-ldap-tls-label') }}
                </label>
                <SegmentedControl
                  v-model="form.tls_mode"
                  :options="tlsOptions"
                  :aria-label="$t('admin-ldap-tls-label')"
                />
                <p class="text-xs text-tertiary">{{ $t('admin-ldap-tls-desc') }}</p>
              </div>
              <FormNumber
                v-model="form.connect_timeout_secs"
                :label="$t('admin-ldap-timeout-label')"
                :description="$t('admin-ldap-timeout-desc')"
                :min="1"
                :max="120"
                integer
              />
            </div>
            <ToggleSwitch
              v-model="form.verify_certs"
              :label="$t('admin-ldap-verify-label')"
              :description="$t('admin-ldap-verify-desc')"
            />
            <FormTextarea
              v-model="form.ca_cert_pem"
              :label="$t('admin-ldap-cacert-label')"
              :description="$t('admin-ldap-cacert-desc')"
              :placeholder="'-----BEGIN CERTIFICATE-----'"
              :rows="3"
              mono
            />
          </div>
        </SectionCard>

        <!-- Authentication -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-auth') }}</template>
          <div class="flex flex-col gap-5">
            <FormInput
              v-model="form.bind_dn"
              :label="$t('admin-ldap-binddn-label')"
              :placeholder="$t('admin-ldap-binddn-placeholder')"
              :description="$t('admin-ldap-binddn-desc')"
            />
            <div class="flex flex-col gap-1.5">
              <label class="text-xs font-medium text-tertiary uppercase">
                {{ $t('admin-ldap-bindpw-label') }}
              </label>
              <PasswordInput
                v-model="form.bind_password"
                :placeholder="hasBindPassword ? $t('admin-ldap-bindpw-keep') : $t('admin-ldap-bindpw-placeholder')"
                autocomplete="off"
              />
              <p class="text-xs text-tertiary">
                {{ hasBindPassword ? $t('admin-ldap-bindpw-stored') : $t('admin-ldap-bindpw-desc') }}
                <button
                  v-if="hasBindPassword"
                  type="button"
                  class="text-status-error hover:underline ml-1"
                  @click="clearBindPassword = !clearBindPassword"
                >
                  {{ clearBindPassword ? $t('admin-ldap-bindpw-clear-undo') : $t('admin-ldap-bindpw-clear') }}
                </button>
              </p>
            </div>
          </div>
        </SectionCard>

        <!-- User mapping -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-users') }}</template>
          <div class="flex flex-col gap-5">
            <FormInput
              v-model="form.user_base_dn"
              :label="$t('admin-ldap-userbase-label')"
              :placeholder="$t('admin-ldap-userbase-placeholder')"
              :description="$t('admin-ldap-userbase-desc')"
              :required="form.enabled"
            />
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormInput
                v-model="form.username_attribute"
                :label="$t('admin-ldap-userattr-label')"
                :description="$t('admin-ldap-userattr-desc')"
              />
              <FormNumber
                v-model="form.page_size"
                :label="$t('admin-ldap-pagesize-label')"
                :description="$t('admin-ldap-pagesize-desc')"
                :min="1"
                :max="10000"
                integer
              />
            </div>
            <FormInput
              v-model="form.user_filter"
              :label="$t('admin-ldap-filter-label')"
              :error="filterError"
              :description="$t('admin-ldap-filter-desc')"
              mono
            />
          </div>
        </SectionCard>

        <!-- Actions: test + save -->
        <div
          class="flex flex-wrap items-center gap-3 sticky bottom-0 bg-app/80 backdrop-blur border-t border-default py-3"
        >
          <div class="flex items-center gap-2">
            <Button
              variant="secondary"
              :loading="testing"
              :disabled="!canTest || testing"
              @click="testConnection"
            >
              {{ testing ? $t('admin-ldap-testing') : $t('admin-ldap-test') }}
            </Button>
            <span
              v-if="testResult === 'ok'"
              class="text-sm text-status-success inline-flex items-center gap-1.5"
            >
              <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-success" />
              {{ $t('admin-ldap-test-ok') }}
            </span>
            <span
              v-else-if="testResult === 'failed'"
              class="text-sm text-status-error inline-flex items-center gap-1.5"
              :title="testErrorMessage"
            >
              <span class="inline-block w-1.5 h-1.5 rounded-full bg-status-error" />
              {{ testErrorMessage || $t('admin-ldap-test-failed') }}
            </span>
            <span v-else-if="isDirty" class="text-xs text-tertiary">
              {{ $t('admin-ldap-test-save-first') }}
            </span>
          </div>

          <div class="ml-auto flex items-center gap-2">
            <Button
              v-if="hasLoadedData"
              variant="secondary"
              :loading="syncing"
              :disabled="!canSync || syncing"
              :title="syncHint"
              @click="runSync"
            >
              {{ syncing ? $t('admin-ldap-syncing') : $t('admin-ldap-sync') }}
            </Button>
            <Button :loading="saving" :disabled="!canSave || !isDirty || saving" @click="save">
              {{ saving ? $t('admin-ldap-saving') : $t('admin-ldap-save') }}
            </Button>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormNumber from '@/components/common/FormNumber.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import PasswordInput from '@/components/common/PasswordInput.vue';
import Button from '@/components/common/Button.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import {
  ldapService,
  type LdapSettings,
  type LdapSettingsResponse,
  type LdapPreset,
  type UpsertLdapSettings,
} from '@/services/ldapService';
import { useToastStore } from '@/stores/toast';
import { createErrorFromResponse } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

interface FormState extends Omit<UpsertLdapSettings, 'ca_cert_pem'> {
  ca_cert_pem: string; // '' <-> null
  bind_password: string; // write-only, never seeded
}

/** AD-flavoured defaults for a brand-new config. */
function emptyForm(): FormState {
  return {
    enabled: false,
    host: '',
    port: 636,
    tls_mode: 'ldaps',
    verify_certs: true,
    ca_cert_pem: '',
    follow_referrals: false,
    connect_timeout_secs: 10,
    auth_mode: 'simple_bind',
    bind_dn: '',
    bind_password: '',
    user_base_dn: '',
    username_attribute: 'sAMAccountName',
    user_filter: '(&(objectCategory=person)(objectClass=user)(sAMAccountName={username}))',
    page_size: 500,
    attribute_map: {},
    group_config: {},
    provisioning: {},
  };
}

// Cache-first config load; the skeleton only shows on a cold cache.
const LDAP_KEY = ['ldap-settings'] as const;
const queryCache = useQueryCache();
const settingsQuery = useQuery({
  key: LDAP_KEY,
  query: () => ldapService.getSettings(),
});
const presetsQuery = useQuery({ key: ['ldap-presets'], query: () => ldapService.getPresets() });

const saved = computed<LdapSettings | null>(() => settingsQuery.data.value?.settings ?? null);
const hasBindPassword = computed(() => settingsQuery.data.value?.has_bind_password ?? false);
const savedEnabled = computed(() => saved.value?.enabled ?? false);
const hasLoadedData = computed(() => settingsQuery.data.value !== undefined);
const isFirstLoad = computed(
  () => settingsQuery.status.value === 'pending' && settingsQuery.data.value === undefined,
);
const loadError = computed(() => {
  const err = settingsQuery.error.value;
  return err ? createErrorFromResponse(err).getUserMessage() || t('admin-ldap-error-load') : '';
});

const form = ref<FormState>(emptyForm());
const clearBindPassword = ref(false);
const saving = ref(false);
const testing = ref(false);
const syncing = ref(false);
const errorMessage = ref('');
const testResult = ref<'idle' | 'ok' | 'failed'>('idle');
const testErrorMessage = ref('');
const selectedPreset = ref<string>('');

// One-shot seed from the cached query; background revalidations don't clobber
// in-progress edits (the component remounts + reseeds on nav-back).
const seeded = ref(false);
watch(
  settingsQuery.data,
  (data) => {
    if (data === undefined || seeded.value) return;
    if (data.settings) populateForm(data.settings);
    seeded.value = true;
  },
  { immediate: true },
);

function populateForm(s: LdapSettings) {
  form.value = {
    enabled: s.enabled,
    host: s.host,
    port: s.port,
    tls_mode: s.tls_mode,
    verify_certs: s.verify_certs,
    ca_cert_pem: s.ca_cert_pem ?? '',
    follow_referrals: s.follow_referrals,
    connect_timeout_secs: s.connect_timeout_secs,
    auth_mode: s.auth_mode,
    bind_dn: s.bind_dn,
    bind_password: '',
    user_base_dn: s.user_base_dn,
    username_attribute: s.username_attribute,
    user_filter: s.user_filter,
    page_size: s.page_size,
    attribute_map: s.attribute_map ?? {},
    group_config: s.group_config ?? {},
    provisioning: s.provisioning ?? {},
  };
}

const tlsOptions = computed(() => [
  { value: 'ldaps', label: t('admin-ldap-tls-ldaps') },
  { value: 'starttls', label: t('admin-ldap-tls-starttls') },
]);

const presetOptions = computed(() => [
  { value: '', label: t('admin-ldap-preset-placeholder') },
  ...(presetsQuery.data.value ?? []).map((p: LdapPreset) => ({ value: p.id, label: p.label })),
]);

function applyPreset(value: string | string[]) {
  const id = Array.isArray(value) ? (value[0] ?? '') : value;
  selectedPreset.value = id;
  const preset = (presetsQuery.data.value ?? []).find((p) => p.id === id);
  if (!preset) return;
  const d = preset.defaults;
  // Prefill the connection + mapping fields; the admin still supplies host,
  // base DN and credentials. Object maps replace wholesale.
  if (d.port != null) form.value.port = d.port;
  if (d.tls_mode) form.value.tls_mode = d.tls_mode;
  if (d.username_attribute) form.value.username_attribute = d.username_attribute;
  if (d.user_filter) form.value.user_filter = d.user_filter;
  if (d.attribute_map) form.value.attribute_map = d.attribute_map;
  if (d.group_config) form.value.group_config = d.group_config;
}

const filterError = computed(() =>
  form.value.user_filter && !form.value.user_filter.includes('{username}')
    ? t('admin-ldap-filter-error')
    : '',
);

function toUpsert(): UpsertLdapSettings {
  const f = form.value;
  return {
    enabled: f.enabled,
    host: f.host.trim(),
    port: f.port,
    tls_mode: f.tls_mode,
    verify_certs: f.verify_certs,
    ca_cert_pem: f.ca_cert_pem.trim() ? f.ca_cert_pem : null,
    follow_referrals: f.follow_referrals,
    connect_timeout_secs: f.connect_timeout_secs,
    auth_mode: f.auth_mode,
    bind_dn: f.bind_dn.trim(),
    user_base_dn: f.user_base_dn.trim(),
    username_attribute: f.username_attribute.trim(),
    user_filter: f.user_filter.trim(),
    page_size: f.page_size,
    attribute_map: f.attribute_map,
    group_config: f.group_config,
    provisioning: f.provisioning,
  };
}

// Dirty against the saved config (the password field is dirty on its own, and a
// pending clear counts). A brand-new (unsaved) config is dirty once it has a host.
const isDirty = computed(() => {
  if (form.value.bind_password.length > 0 || clearBindPassword.value) return true;
  const s = saved.value;
  if (!s) return form.value.host.trim().length > 0 || form.value.enabled;
  const f = toUpsert();
  return (
    f.enabled !== s.enabled ||
    f.host !== s.host ||
    f.port !== s.port ||
    f.tls_mode !== s.tls_mode ||
    f.verify_certs !== s.verify_certs ||
    (f.ca_cert_pem ?? null) !== (s.ca_cert_pem ?? null) ||
    f.follow_referrals !== s.follow_referrals ||
    f.connect_timeout_secs !== s.connect_timeout_secs ||
    f.auth_mode !== s.auth_mode ||
    f.bind_dn !== s.bind_dn ||
    f.user_base_dn !== s.user_base_dn ||
    f.username_attribute !== s.username_attribute ||
    f.user_filter !== s.user_filter ||
    f.page_size !== s.page_size
  );
});

const canSave = computed(() => {
  const f = form.value;
  if (filterError.value) return false;
  // An enabled config needs somewhere to connect + search.
  if (f.enabled && (!f.host.trim() || !f.user_base_dn.trim())) return false;
  return true;
});

// Test + sync run against the SAVED config, so they're gated on a clean form.
const canTest = computed(() => saved.value !== null && !isDirty.value);
const canSync = computed(() => savedEnabled.value && !isDirty.value);
const syncHint = computed(() => {
  if (isDirty.value) return t('admin-ldap-sync-save-first');
  if (!savedEnabled.value) return t('admin-ldap-sync-enable-first');
  return '';
});

// Any edit invalidates a prior test result (it authenticated the old values).
watch(
  () => JSON.stringify(toUpsert()) + form.value.bind_password,
  () => {
    testResult.value = 'idle';
    testErrorMessage.value = '';
  },
);

async function save() {
  if (!canSave.value || saving.value) return;
  saving.value = true;
  errorMessage.value = '';
  try {
    const res: LdapSettingsResponse = await ldapService.updateSettings(
      toUpsert(),
      form.value.bind_password,
      clearBindPassword.value,
    );
    queryCache.setQueryData(LDAP_KEY, res);
    if (res.settings) populateForm(res.settings);
    form.value.bind_password = '';
    clearBindPassword.value = false;
    selectedPreset.value = '';
    toast.success(t('admin-ldap-saved'));
  } catch (e) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage() || t('admin-ldap-error-save');
  } finally {
    saving.value = false;
  }
}

async function testConnection() {
  if (!canTest.value || testing.value) return;
  testing.value = true;
  testResult.value = 'idle';
  testErrorMessage.value = '';
  try {
    const res = await ldapService.testConnection();
    testResult.value = res.ok ? 'ok' : 'failed';
    testErrorMessage.value = res.error ?? '';
  } catch (e) {
    testResult.value = 'failed';
    testErrorMessage.value = createErrorFromResponse(e).getUserMessage() || t('admin-ldap-test-failed');
  } finally {
    testing.value = false;
  }
}

async function runSync() {
  if (!canSync.value || syncing.value) return;
  syncing.value = true;
  errorMessage.value = '';
  try {
    const res = await ldapService.runSync();
    const s = res.stats;
    toast.success(
      t('admin-ldap-sync-done', { synced: s.synced, seen: s.seen, errors: s.errors }),
    );
  } catch (e) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage() || t('admin-ldap-error-sync');
  } finally {
    syncing.value = false;
  }
}
</script>
