<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <!-- Header -->
      <header class="flex flex-wrap items-start gap-x-3 gap-y-2">
        <div class="flex flex-col gap-1 min-w-0 flex-1">
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('admin-ldap-title') }}
          </h1>
          <p class="text-secondary text-sm">{{ $t('admin-ldap-subtitle') }}</p>
        </div>
        <div class="flex items-center justify-end gap-2 w-full sm:w-auto shrink-0">
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

        <!-- Attribute mapping -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-attrs') }}</template>
          <div class="flex flex-col gap-4">
            <p class="text-xs text-tertiary">{{ $t('admin-ldap-attrs-help') }}</p>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormInput
                v-for="f in ATTR_CORE"
                :key="f.key"
                :model-value="amStr(f.key)"
                :label="$t(`admin-ldap-attr-${f.key}`)"
                :placeholder="f.def"
                @update:model-value="(v: string) => setAm(f.key, v)"
              />
            </div>
            <div v-if="showMoreAttrs" class="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormInput
                v-for="f in ATTR_MORE"
                :key="f.key"
                :model-value="amStr(f.key)"
                :label="$t(`admin-ldap-attr-${f.key}`)"
                :placeholder="f.def"
                @update:model-value="(v: string) => setAm(f.key, v)"
              />
            </div>
            <button
              type="button"
              class="text-xs text-accent hover:underline self-start"
              @click="showMoreAttrs = !showMoreAttrs"
            >
              {{ showMoreAttrs ? $t('admin-ldap-attrs-less') : $t('admin-ldap-attrs-more') }}
            </button>
          </div>
        </SectionCard>

        <!-- Groups & roles -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-groups') }}</template>
          <div class="flex flex-col gap-5">
            <FormInput
              v-model="groupBaseDn"
              :label="$t('admin-ldap-groupbase-label')"
              :placeholder="$t('admin-ldap-groupbase-placeholder')"
              :description="$t('admin-ldap-groupbase-desc')"
            />
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
              <FormInput
                v-model="groupObjectClass"
                :label="$t('admin-ldap-group-objectclass-label')"
                placeholder="group"
              />
              <FormInput
                v-model="groupMemberAttr"
                :label="$t('admin-ldap-group-member-label')"
                placeholder="member"
              />
              <FormInput
                v-model="groupNameAttr"
                :label="$t('admin-ldap-group-name-label')"
                placeholder="cn"
              />
            </div>

            <!-- Group -> role mapping (safe-by-design) -->
            <div class="flex flex-col gap-3 border-t border-default pt-4">
              <div class="flex flex-col gap-1">
                <span class="font-medium text-primary">{{ $t('admin-ldap-roles-heading') }}</span>
                <p class="text-xs text-tertiary">{{ $t('admin-ldap-roles-help') }}</p>
              </div>

              <div class="flex flex-wrap items-center gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  :loading="discovering"
                  :disabled="!groupSyncConfigured || isDirty || discovering"
                  @click="discoverGroups"
                >
                  {{ discovering ? $t('admin-ldap-discovering') : $t('admin-ldap-discover') }}
                </Button>
                <span v-if="discoveredGroups.length" class="text-xs text-tertiary">
                  {{ $t('admin-ldap-discover-found', { count: discoveredGroups.length }) }}
                </span>
                <span v-else-if="isDirty" class="text-xs text-tertiary">
                  {{ $t('admin-ldap-discover-save-first') }}
                </span>
                <span v-if="discoverError" class="text-xs text-status-error" :title="discoverError">
                  {{ discoverError }}
                </span>
              </div>

              <div v-if="roleMappings.length" class="flex flex-col gap-2">
                <div
                  v-for="(rule, idx) in roleMappings"
                  :key="idx"
                  class="flex flex-wrap sm:flex-nowrap items-end gap-2"
                >
                  <div class="flex-1 min-w-[12rem]">
                    <SearchableDropdown
                      :model-value="rule.group"
                      :options="groupOptions"
                      :label="idx === 0 ? $t('admin-ldap-role-group-label') : ''"
                      :placeholder="$t('admin-ldap-role-group-placeholder')"
                      :empty-message="$t('admin-ldap-discover-hint')"
                      size="sm"
                      @update:model-value="(v: string | string[]) => setRuleGroup(idx, v)"
                    />
                  </div>
                  <span class="text-tertiary text-sm pb-2 hidden sm:inline">&rarr;</span>
                  <div class="w-full sm:w-40">
                    <BaseDropdown
                      :model-value="rule.role"
                      :options="roleOptions"
                      :label="idx === 0 ? $t('admin-ldap-role-role-label') : ''"
                      size="sm"
                      @update:model-value="(v: string | string[]) => setRuleRole(idx, v)"
                    />
                  </div>
                  <Button
                    variant="ghost-danger"
                    size="sm"
                    icon="trash"
                    :aria-label="$t('admin-ldap-role-remove')"
                    @click="removeRule(idx)"
                  />
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2">
                <Button variant="secondary" size="sm" icon="add" @click="addRule">
                  {{ $t('admin-ldap-role-add') }}
                </Button>
                <span class="text-xs text-tertiary">{{ $t('admin-ldap-role-default-note') }}</span>
              </div>

              <p
                v-if="hasAdminRule"
                class="text-xs text-status-warning bg-status-warning/10 rounded px-2.5 py-1.5"
              >
                {{ $t('admin-ldap-role-admin-warning') }}
              </p>

              <!-- Blast-radius preview -->
              <div class="flex flex-wrap items-center gap-2 border-t border-default pt-3">
                <Button
                  variant="secondary"
                  size="sm"
                  :loading="previewing"
                  :disabled="!canTest || previewing"
                  :title="isDirty ? $t('admin-ldap-preview-save-first') : ''"
                  @click="previewSync"
                >
                  {{ previewing ? $t('admin-ldap-previewing') : $t('admin-ldap-preview') }}
                </Button>
                <span v-if="previewError" class="text-xs text-status-error" :title="previewError">
                  {{ previewError }}
                </span>
              </div>
              <div
                v-if="previewData"
                class="flex flex-col gap-2 rounded-lg bg-surface-alt p-3 text-sm"
              >
                <p class="text-primary font-medium">
                  {{ $t('admin-ldap-preview-users', { count: countDisplay(previewData.user_count, previewData.user_capped) }) }}
                </p>
                <div v-if="previewData.rules.length" class="flex flex-col gap-1.5">
                  <div
                    v-for="(rule, idx) in previewData.rules"
                    :key="idx"
                    class="flex items-center gap-2 flex-wrap"
                  >
                    <StatusPill
                      :label="$t(`admin-ldap-role-${rule.role}`)"
                      :tone="rule.role === 'admin' ? 'caution' : rule.role === 'agent' ? 'info' : 'neutral'"
                      size="xs"
                    />
                    <span class="text-secondary">{{ rule.group }}</span>
                    <span v-if="rule.found" class="text-tertiary text-xs">
                      {{ $t('admin-ldap-preview-members', { count: countDisplay(rule.member_count, rule.member_capped) }) }}
                    </span>
                    <span v-else class="text-status-error text-xs">
                      {{ $t('admin-ldap-preview-not-found') }}
                    </span>
                  </div>
                </div>
                <p class="text-tertiary text-xs">{{ $t('admin-ldap-preview-default') }}</p>
              </div>
            </div>
          </div>
        </SectionCard>

        <!-- Sync & status -->
        <SectionCard content-padding="p-4 sm:p-5">
          <template #title>{{ $t('admin-ldap-section-status') }}</template>
          <div class="flex flex-col gap-4">
            <!-- Sync mode + last reconcile -->
            <div class="flex flex-wrap items-center gap-2 text-sm">
              <StatusPill
                :label="cursor?.incremental_active ? $t('admin-ldap-mode-incremental') : $t('admin-ldap-mode-full')"
                :tone="cursor?.incremental_active ? 'info' : 'neutral'"
                size="sm"
              />
              <span v-if="cursor?.last_full_reconcile_at" class="text-tertiary">
                {{ $t('admin-ldap-last-reconcile', { when: formatRelativeTime(cursor.last_full_reconcile_at) }) }}
              </span>
            </div>

            <!-- Last run -->
            <div v-if="lastRun" class="flex flex-col gap-3">
              <div class="flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">{{ $t('admin-ldap-last-run') }}</span>
                <StatusPill
                  :label="runStatusLabel(lastRun.status)"
                  :tone="runStatusTone(lastRun.status)"
                  size="sm"
                />
                <span class="text-tertiary text-xs">{{ runTypeLabel(lastRun) }}</span>
              </div>
              <div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">{{ $t('admin-ldap-run-started') }}</span>
                  <span class="text-primary text-sm font-medium">{{ formatRelativeTime(lastRun.started_at) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">{{ $t('admin-ldap-run-duration') }}</span>
                  <span class="text-primary text-sm font-medium">{{ formatDuration(lastRun) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">{{ $t('admin-ldap-run-synced') }}</span>
                  <span class="text-primary text-sm font-medium">{{ lastRun.records_updated ?? 0 }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">{{ $t('admin-ldap-run-errors') }}</span>
                  <span
                    class="text-sm font-medium"
                    :class="(lastRun.records_failed ?? 0) > 0 ? 'text-status-error' : 'text-primary'"
                  >{{ lastRun.records_failed ?? 0 }}</span>
                </div>
              </div>
              <div
                v-if="lastRun.error_message"
                class="text-xs text-secondary bg-surface-alt rounded px-2.5 py-1.5 break-words"
              >
                {{ lastRun.error_message }}
              </div>
            </div>
            <p v-else class="text-sm text-tertiary">{{ $t('admin-ldap-no-runs') }}</p>

            <!-- Older runs -->
            <div v-if="runs.length > 1" class="flex flex-col gap-1.5 border-t border-default pt-3">
              <span class="text-tertiary text-xs uppercase font-medium">{{ $t('admin-ldap-history') }}</span>
              <div
                v-for="run in runs.slice(1)"
                :key="run.id"
                class="flex items-center gap-2.5 text-sm py-0.5"
              >
                <StatusPill
                  :label="runStatusLabel(run.status)"
                  :tone="runStatusTone(run.status)"
                  size="xs"
                />
                <span class="text-secondary">{{ formatRelativeTime(run.started_at) }}</span>
                <span class="text-tertiary text-xs hidden sm:inline">{{ runTypeLabel(run) }}</span>
                <span class="ml-auto text-tertiary text-xs">
                  {{ $t('admin-ldap-run-counts', { synced: run.records_updated ?? 0, errors: run.records_failed ?? 0 }) }}
                </span>
              </div>
            </div>
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
import SearchableDropdown from '@/components/common/SearchableDropdown.vue';
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
  type LdapSyncRun,
  type UpsertLdapSettings,
  type DiscoveredGroup,
  type RoleMapping,
  type RolePreview,
} from '@/services/ldapService';
import { useToastStore } from '@/stores/toast';
import { createErrorFromResponse } from '@/utils/errors';
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils';

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

// Sync runs + cursor state for the status panel (refreshed after a manual sync).
const HISTORY_KEY = ['ldap-sync-history'] as const;
const historyQuery = useQuery({ key: HISTORY_KEY, query: () => ldapService.getSyncHistory() });
const runs = computed<LdapSyncRun[]>(() => historyQuery.data.value?.runs ?? []);
const lastRun = computed<LdapSyncRun | null>(() => runs.value[0] ?? null);
const cursor = computed(() => historyQuery.data.value?.cursor ?? null);

function runStatusTone(status: string): 'positive' | 'caution' | 'critical' | 'neutral' {
  if (status === 'completed') return 'positive';
  if (status === 'completed_with_errors') return 'caution';
  if (status === 'failed' || status === 'error') return 'critical';
  return 'neutral';
}
function runStatusLabel(status: string): string {
  return t(`admin-ldap-run-status-${status}`);
}
function runTypeLabel(run: LdapSyncRun): string {
  return t(run.sync_type === 'ldap_reconcile' ? 'admin-ldap-run-reconcile' : 'admin-ldap-run-sync');
}
function formatDuration(run: LdapSyncRun): string {
  if (!run.completed_at) return '—';
  const ms = new Date(run.completed_at).getTime() - new Date(run.started_at).getTime();
  if (!Number.isFinite(ms) || ms < 0) return '—';
  const secs = Math.round(ms / 1000);
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}m ${secs % 60}s`;
}

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
    f.page_size !== s.page_size ||
    JSON.stringify(f.group_config) !== JSON.stringify(s.group_config ?? {}) ||
    JSON.stringify(f.attribute_map) !== JSON.stringify(s.attribute_map ?? {})
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
    // The preview reflects the saved config; an edit makes it stale.
    previewData.value = null;
  },
);

// --- Groups & roles (stored inside the group_config JSONB) ----------------
function gcStr(key: string): string {
  const v = form.value.group_config[key];
  return typeof v === 'string' ? v : '';
}
function setGc(key: string, val: string) {
  form.value.group_config = { ...form.value.group_config, [key]: val };
}
const groupBaseDn = computed({ get: () => gcStr('group_base_dn'), set: (v) => setGc('group_base_dn', v) });
const groupObjectClass = computed({ get: () => gcStr('object_class'), set: (v) => setGc('object_class', v) });
const groupMemberAttr = computed({ get: () => gcStr('member_attribute'), set: (v) => setGc('member_attribute', v) });
const groupNameAttr = computed({ get: () => gcStr('name_attribute'), set: (v) => setGc('name_attribute', v) });
const roleMappings = computed<RoleMapping[]>({
  get: () => (Array.isArray(form.value.group_config.role_mappings)
    ? (form.value.group_config.role_mappings as RoleMapping[])
    : []),
  set: (v) => {
    form.value.group_config = { ...form.value.group_config, role_mappings: v };
  },
});
const groupSyncConfigured = computed(() => groupBaseDn.value.trim().length > 0);
const hasAdminRule = computed(() => roleMappings.value.some((r) => r.role === 'admin'));

const roleOptions = computed(() => [
  { value: 'member', label: t('admin-ldap-role-member') },
  { value: 'agent', label: t('admin-ldap-role-agent') },
  { value: 'admin', label: t('admin-ldap-role-admin') },
]);

// Discovered directory groups feed the rule picker so the admin selects real
// groups rather than typing a CN blind. Saved rule names are merged in so they
// always show even before a discovery run.
const discoveredGroups = ref<DiscoveredGroup[]>([]);
const discovering = ref(false);
const discoverError = ref('');
const groupOptions = computed(() => {
  const names = new Set<string>(discoveredGroups.value.map((g) => g.name));
  for (const r of roleMappings.value) if (r.group) names.add(r.group);
  return Array.from(names)
    .sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))
    .map((n) => ({ value: n, label: n }));
});

async function discoverGroups() {
  if (discovering.value) return;
  discovering.value = true;
  discoverError.value = '';
  try {
    const res = await ldapService.discoverGroups();
    if (res.ok) discoveredGroups.value = res.groups;
    else discoverError.value = res.error ?? t('admin-ldap-discover-failed');
  } catch (e) {
    discoverError.value = createErrorFromResponse(e).getUserMessage() || t('admin-ldap-discover-failed');
  } finally {
    discovering.value = false;
  }
}

function addRule() {
  roleMappings.value = [...roleMappings.value, { group: '', role: 'agent' }];
}
function removeRule(idx: number) {
  roleMappings.value = roleMappings.value.filter((_, i) => i !== idx);
}
function setRuleGroup(idx: number, group: string | string[]) {
  const g = Array.isArray(group) ? (group[0] ?? '') : group;
  roleMappings.value = roleMappings.value.map((r, i) => (i === idx ? { ...r, group: g } : r));
}
function setRuleRole(idx: number, role: string | string[]) {
  const ro = (Array.isArray(role) ? role[0] : role) as RoleMapping['role'];
  roleMappings.value = roleMappings.value.map((r, i) => (i === idx ? { ...r, role: ro } : r));
}

// --- Blast-radius preview --------------------------------------------------
function countDisplay(n: number, capped: boolean): string {
  return capped ? `${n}+` : `${n}`;
}
const previewing = ref(false);
const previewError = ref('');
const previewData = ref<RolePreview | null>(null);
async function previewSync() {
  if (previewing.value || !canTest.value) return;
  previewing.value = true;
  previewError.value = '';
  try {
    const res = await ldapService.previewSync();
    if (res.ok && res.preview) {
      previewData.value = res.preview;
    } else {
      previewError.value = res.error ?? t('admin-ldap-preview-failed');
    }
  } catch (e) {
    previewError.value = createErrorFromResponse(e).getUserMessage() || t('admin-ldap-preview-failed');
  } finally {
    previewing.value = false;
  }
}

// --- Attribute mapping (stored in the attribute_map JSONB) ----------------
// Logical field -> LDAP attribute name; the placeholder is the AD default the
// backend falls back to when a value is blank.
const ATTR_CORE = [
  { key: 'external_id', def: 'objectGUID' },
  { key: 'email', def: 'mail' },
  { key: 'display_name', def: 'displayName' },
  { key: 'first_name', def: 'givenName' },
  { key: 'last_name', def: 'sn' },
] as const;
const ATTR_MORE = [
  { key: 'title', def: 'title' },
  { key: 'department', def: 'department' },
  { key: 'organization', def: 'company' },
  { key: 'office_location', def: 'physicalDeliveryOfficeName' },
  { key: 'phone', def: 'telephoneNumber' },
  { key: 'mobile', def: 'mobile' },
  { key: 'street', def: 'streetAddress' },
  { key: 'city', def: 'l' },
  { key: 'region', def: 'st' },
  { key: 'postal_code', def: 'postalCode' },
  { key: 'country', def: 'co' },
] as const;
const showMoreAttrs = ref(false);
function amStr(key: string): string {
  const v = form.value.attribute_map[key];
  return typeof v === 'string' ? v : '';
}
function setAm(key: string, val: string) {
  form.value.attribute_map = { ...form.value.attribute_map, [key]: val };
}

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
    await queryCache.invalidateQueries({ key: HISTORY_KEY });
  } catch (e) {
    errorMessage.value = createErrorFromResponse(e).getUserMessage() || t('admin-ldap-error-sync');
  } finally {
    syncing.value = false;
  }
}
</script>
