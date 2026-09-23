<script setup lang="ts">
/**
 * Licence & Cloud (self-hosted, platform admins).
 *
 * Status first, then the one action that matters for that status, then
 * detail. A Community instance is a complete product, so the page says what
 * already works before it says what a licence adds, and never nags. Plan:
 * docs/plans/self-hosted-license-activation.md.
 *
 * Reads through Pinia Colada (cache-first); every write returns the whole
 * overview, which replaces the cached copy.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import Button from '@/components/common/Button.vue';
import Callout from '@/components/common/Callout.vue';
import CodeBlock from '@/components/common/CodeBlock.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import type { StatusPillTone } from '@/components/common/statusPillTone';
import { button } from '@/recipes/button';
import { errorCode } from '@/utils/errors';
import licenseService from '@nosdesk/core/services/licenseService';
import { getControlPlaneUrl } from '@nosdesk/core/services/instanceConfig';
import { formatDate, formatRelativeTime } from '@nosdesk/core/utils/dateUtils';
import type { LicenseOverview, PushMode } from '@nosdesk/core/types/license';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const OVERVIEW_KEY = ['admin-license'] as const;
const queryCache = useQueryCache();
const overviewQuery = useQuery({
  key: OVERVIEW_KEY,
  query: () => licenseService.getOverview(),
});
const overview = computed(() => overviewQuery.data.value);
const loadError = computed(() =>
  overviewQuery.error.value && !overview.value ? t('admin-license-error-load') : '',
);

function apply(next: LicenseOverview) {
  queryCache.setQueryData(OVERVIEW_KEY, next);
  // The Workspaces page reads the cap from the edition endpoint.
  queryCache.invalidateQueries({ key: ['admin-edition'] });
}

// Links out. CONTROL_PLANE_URL lets a staging instance point at staging.
const dashboardBase = computed(() => (getControlPlaneUrl() || 'https://manage.nosdesk.com').replace(/\/$/, ''));
const licenseDashboardUrl = computed(() => `${dashboardBase.value}/account/license`);
const PRICING_URL = 'https://nosdesk.com/pricing#self-host';
const DOCS_URL = 'https://nosdesk.com/docs/operations/licensing';

// --- Licence status ------------------------------------------------------

const license = computed(() => overview.value?.license);
const details = computed(() => license.value?.details ?? null);
const isLicensed = computed(() => overview.value?.edition === 'enterprise');
const envManaged = computed(() => license.value?.env_managed ?? false);
const canRemove = computed(
  () => !envManaged.value && (license.value?.source === 'pasted' || license.value?.source === 'linked'),
);

const DAY = 86_400;
const daysLeft = computed(() => {
  if (!details.value) return null;
  return Math.floor((details.value.expires_at - Date.now() / 1000) / DAY);
});

const statusPill = computed<{ label: string; tone: StatusPillTone }>(() => {
  const err = license.value?.error;
  if (err === 'expired') return { label: t('admin-license-pill-expired'), tone: 'critical' };
  if (err) return { label: t('admin-license-pill-invalid'), tone: 'critical' };
  if (!isLicensed.value) return { label: t('admin-license-pill-none'), tone: 'neutral' };
  const d = daysLeft.value ?? 0;
  if (d <= 30) return { label: t('admin-license-pill-expiring', { days: Math.max(d, 0) }), tone: 'caution' };
  return { label: t('admin-license-pill-active'), tone: 'positive' };
});

const expiresAt = computed(() =>
  details.value ? new Date(details.value.expires_at * 1000) : null,
);

const sourceLabel = computed(() => {
  switch (license.value?.source) {
    case 'env': return t('admin-license-source-env');
    case 'linked': return t('admin-license-source-linked');
    case 'pasted': return t('admin-license-source-pasted');
    default: return '';
  }
});

const errorMessage = computed(() => {
  const err = license.value?.error;
  return err && err !== 'expired' ? t(`admin-license-reason-${err.replace(/_/g, '-')}`) : '';
});

/** Email first: the row that is the same on both sides says the most. */
const compareRows = computed(() => [
  { label: t('admin-license-compare-email'), now: t('admin-license-compare-email-both'), licensed: t('admin-license-compare-email-both'), same: true },
  { label: t('admin-license-compare-workspaces'), now: '1', licensed: t('admin-license-compare-workspaces-licensed'), same: false },
  { label: t('admin-license-compare-push'), now: t('admin-license-compare-push-now'), licensed: t('admin-license-compare-push-licensed'), same: false },
  { label: t('admin-license-compare-support'), now: t('admin-license-compare-support-now'), licensed: t('admin-license-compare-support-licensed'), same: false },
]);

// --- Install a key -------------------------------------------------------

const showPaste = ref(false);
const pastedKey = ref('');
const installing = ref(false);
const installError = ref('');
const notice = ref('');
const pasteOpen = computed(
  () => !envManaged.value && (showPaste.value || (!isLicensed.value && !details.value)),
);

const INSTALL_ERRORS: Record<string, string> = {
  license_malformed: 'admin-license-reason-malformed',
  license_unknown_key: 'admin-license-reason-unknown-key',
  license_bad_signature: 'admin-license-reason-bad-signature',
  license_wrong_issuer: 'admin-license-reason-wrong-issuer',
  license_invalid_claims: 'admin-license-reason-invalid-claims',
  license_expired: 'admin-license-reason-expired',
  license_env_managed: 'admin-license-env-managed',
};

async function install() {
  installError.value = '';
  notice.value = '';
  installing.value = true;
  try {
    apply(await licenseService.install(pastedKey.value.trim()));
    pastedKey.value = '';
    showPaste.value = false;
    notice.value = t('admin-license-installed');
  } catch (e) {
    const key = INSTALL_ERRORS[errorCode(e) ?? ''];
    installError.value = key ? t(key) : t('admin-license-error-install');
  } finally {
    installing.value = false;
  }
}

const confirmRemove = ref(false);
const removing = ref(false);
async function remove() {
  removing.value = true;
  notice.value = '';
  try {
    apply(await licenseService.remove());
    confirmRemove.value = false;
    notice.value = t('admin-license-removed');
  } catch {
    installError.value = t('admin-license-error-remove');
  } finally {
    removing.value = false;
  }
}

// --- Push ------------------------------------------------------------------

const push = computed(() => overview.value?.push ?? null);
const pushOptions = computed<{ value: PushMode; label: string }[]>(() => [
  { value: 'default', label: t('admin-license-push-mode-default') },
  { value: 'native', label: t('admin-license-push-mode-native') },
  { value: 'relay', label: t('admin-license-push-mode-relay') },
  { value: 'off', label: t('admin-license-push-mode-off') },
]);
const pushSaving = ref(false);
const pushError = ref('');

async function setPushMode(mode: PushMode) {
  pushError.value = '';
  pushSaving.value = true;
  try {
    apply(await licenseService.setPushMode(mode));
  } catch (e) {
    pushError.value =
      errorCode(e) === 'push_mode_unavailable'
        ? t('admin-license-push-error-unavailable')
        : t('admin-license-push-error-save');
  } finally {
    pushSaving.value = false;
  }
}

const retrying = ref(false);
async function retryRelay() {
  retrying.value = true;
  try {
    apply(await licenseService.retryRelay());
  } finally {
    retrying.value = false;
  }
}

/** One plain sentence and a tone for whatever push is doing right now. */
const pushStatus = computed<{ label: string; tone: StatusPillTone; hint?: string } | null>(() => {
  const p = push.value;
  if (!p) return null;
  if (p.sender === 'relay') {
    // No licence: say so even before the first exchange, which after a
    // restart has not happened yet.
    const outcome = details.value ? (p.relay?.last_outcome ?? null) : 'no_license';
    switch (outcome) {
      case 'ok':
        return { label: t('admin-license-relay-ok'), tone: 'positive' };
      case null:
        return { label: t('admin-license-relay-idle'), tone: 'neutral' };
      case 'no_license':
        return { label: t('admin-license-relay-no-license'), tone: 'caution', hint: t('admin-license-relay-no-license-hint') };
      case 'dpa_required':
        return { label: t('admin-license-relay-dpa'), tone: 'caution', hint: t('admin-license-relay-dpa-hint') };
      case 'invalid_license':
        return { label: t('admin-license-relay-rejected'), tone: 'critical', hint: t('admin-license-relay-rejected-hint') };
      case 'usage_cap':
        return { label: t('admin-license-relay-cap'), tone: 'caution', hint: t('admin-license-relay-cap-hint') };
      case 'rate_limited':
        return { label: t('admin-license-relay-throttled'), tone: 'caution' };
      case 'unreachable':
      case 'relay_unavailable':
        return { label: t('admin-license-relay-unreachable'), tone: 'critical', hint: t('admin-license-relay-unreachable-hint') };
      default:
        return { label: t('admin-license-relay-unexpected'), tone: 'critical' };
    }
  }
  if (p.sender === 'native') {
    return { label: t('admin-license-push-native-ok'), tone: 'positive' };
  }
  if (p.mode === 'off') return { label: t('admin-license-push-off'), tone: 'neutral' };
  return {
    label: t('admin-license-push-none'),
    tone: 'neutral',
    hint: t('admin-license-push-none-hint'),
  };
});

const relayNeedsDashboard = computed(() => {
  const o = push.value?.relay?.last_outcome;
  return o === 'dpa_required' || o === 'no_license' || o === 'invalid_license' || o === 'usage_cap';
});
const relayLastSuccess = computed(() => {
  const at = push.value?.relay?.last_success_at;
  return at ? formatRelativeTime(new Date(at * 1000)) : '';
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <header>
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-license-title') }}</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">{{ $t('admin-license-description') }}</p>
      </header>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />
      <AlertMessage v-if="notice" type="success" :message="notice" />

      <template v-if="overview">
        <!-- Status -->
        <section class="bg-surface border border-default rounded-xl p-4 sm:p-5 flex flex-col gap-4">
          <div class="flex flex-col sm:flex-row sm:items-start gap-3">
            <div class="flex-1 min-w-0 flex flex-col gap-1">
              <div class="flex flex-wrap items-center gap-2">
                <h2 class="text-lg font-semibold text-primary truncate">
                  {{ details ? details.licensee : $t('admin-license-community-title') }}
                </h2>
                <StatusPill :label="statusPill.label" :tone="statusPill.tone" size="sm" />
              </div>
              <p class="text-sm text-secondary">
                {{ isLicensed || details ? $t('admin-license-licensed-subtitle') : $t('admin-license-community-body') }}
              </p>
            </div>
            <div v-if="!isLicensed && !details" class="flex flex-wrap gap-2 shrink-0">
              <a
                :href="licenseDashboardUrl"
                target="_blank"
                rel="noopener"
                :class="button({ variant: 'primary', size: 'sm' })"
              >
                <span>{{ $t('admin-license-get') }}</span>
                <Icon name="link" size="xs" />
              </a>
            </div>
          </div>

          <dl v-if="details" class="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div class="rounded-lg bg-surface-alt px-3 py-2">
              <dt class="text-xs text-tertiary">{{ $t('admin-license-fact-workspaces') }}</dt>
              <dd class="text-sm font-medium text-primary tabular-nums">
                {{ $t('admin-license-fact-workspaces-value', { used: overview.active_workspaces, max: details.max_workspaces }) }}
              </dd>
            </div>
            <div class="rounded-lg bg-surface-alt px-3 py-2">
              <dt class="text-xs text-tertiary">
                {{ license?.error === 'expired' ? $t('admin-license-fact-expired') : $t('admin-license-fact-expires') }}
              </dt>
              <dd class="text-sm font-medium text-primary">
                {{ formatDate(expiresAt) }}
                <span class="text-tertiary font-normal">· {{ formatRelativeTime(expiresAt) }}</span>
              </dd>
            </div>
            <div class="rounded-lg bg-surface-alt px-3 py-2">
              <dt class="text-xs text-tertiary">{{ $t('admin-license-fact-source') }}</dt>
              <dd class="text-sm font-medium text-primary">{{ sourceLabel }}</dd>
            </div>
          </dl>

          <Callout v-if="license?.error === 'expired'" severity="error">
            <p class="px-4 py-3 text-sm text-secondary">
              {{ $t('admin-license-expired-body') }}
              <a :href="licenseDashboardUrl" target="_blank" rel="noopener" class="text-accent hover:underline">{{ $t('admin-license-open-account') }}</a>
            </p>
          </Callout>
          <Callout v-else-if="errorMessage" severity="error">
            <p class="px-4 py-3 text-sm text-secondary">{{ errorMessage }}</p>
          </Callout>
          <Callout v-else-if="isLicensed && daysLeft !== null && daysLeft <= 30" severity="warning">
            <p class="px-4 py-3 text-sm text-secondary">
              {{ $t('admin-license-expiring-body') }}
              <a :href="licenseDashboardUrl" target="_blank" rel="noopener" class="text-accent hover:underline">{{ $t('admin-license-open-account') }}</a>
            </p>
          </Callout>

          <div v-if="details || envManaged" class="flex flex-wrap items-center gap-x-4 gap-y-2 pt-1 border-t border-subtle text-xs text-tertiary">
            <span v-if="details" class="font-mono truncate">{{ details.license_id }}</span>
            <span v-if="envManaged">{{ $t('admin-license-env-managed') }}</span>
            <div v-if="!envManaged" class="flex gap-1 ml-auto">
              <Button v-if="!pasteOpen" size="sm" variant="ghost" @click="showPaste = true">
                {{ $t('admin-license-replace') }}
              </Button>
              <Button v-if="canRemove" size="sm" variant="ghost-danger" @click="confirmRemove = true">
                {{ $t('admin-license-remove') }}
              </Button>
            </div>
          </div>
        </section>

        <!-- What a licence adds: only while there is none, and never as a gate
             on what already works. -->
        <SectionCard v-if="!isLicensed && !details" content-padding="">
          <template #title>{{ $t('admin-license-compare-title') }}</template>
          <!-- Table from sm up; stacked rows on a phone, where three columns
               wrap every cell word by word. -->
          <div class="hidden sm:block">
            <table class="w-full text-sm">
              <thead class="text-tertiary text-xs">
                <tr class="border-b border-subtle">
                  <th class="text-left font-medium px-4 py-2 w-2/5"><span class="sr-only">{{ $t('admin-license-compare-feature') }}</span></th>
                  <th class="text-left font-medium px-4 py-2">{{ $t('admin-license-compare-now') }}</th>
                  <th class="text-left font-medium px-4 py-2">{{ $t('admin-license-compare-licensed') }}</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr v-for="row in compareRows" :key="row.label">
                  <th scope="row" class="text-left font-normal text-secondary px-4 py-2.5">{{ row.label }}</th>
                  <td v-for="(cell, i) in [row.now, row.licensed]" :key="i" class="px-4 py-2.5 text-primary">
                    <span class="inline-flex items-center gap-1.5">
                      <Icon v-if="row.same" name="check" size="sm" class="text-status-success shrink-0" />{{ cell }}
                    </span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <dl class="sm:hidden divide-y divide-subtle text-sm">
            <div v-for="row in compareRows" :key="row.label" class="px-4 py-3 flex flex-col gap-1">
              <dt class="text-secondary">{{ row.label }}</dt>
              <dd v-if="row.same" class="text-primary inline-flex items-center gap-1.5">
                <Icon name="check" size="sm" class="text-status-success shrink-0" />{{ row.now }}
              </dd>
              <template v-else>
                <dd class="text-tertiary text-xs">{{ $t('admin-license-compare-now') }}: <span class="text-primary text-sm">{{ row.now }}</span></dd>
                <dd class="text-tertiary text-xs">{{ $t('admin-license-compare-licensed') }}: <span class="text-primary text-sm">{{ row.licensed }}</span></dd>
              </template>
            </div>
          </dl>
          <div class="px-4 py-2.5 border-t border-subtle text-xs">
            <a :href="PRICING_URL" target="_blank" rel="noopener" class="text-accent hover:underline">{{ $t('admin-license-pricing') }}</a>
          </div>
        </SectionCard>

        <!-- Install a key -->
        <SectionCard v-if="pasteOpen">
          <template #title>{{ details ? $t('admin-license-paste-title-replace') : $t('admin-license-paste-title') }}</template>
          <form class="flex flex-col gap-3" @submit.prevent="install">
            <FormTextarea
              id="license-key"
              v-model="pastedKey"
              :label="$t('admin-license-paste-label')"
              :description="$t('admin-license-paste-help')"
              :error="installError"
              placeholder="nsk_lic_…"
              :rows="3"
              mono
            />
            <div class="flex items-center gap-2">
              <Button type="submit" size="sm" :loading="installing" :disabled="!pastedKey.trim()">
                {{ $t('admin-license-paste-submit') }}
              </Button>
              <Button v-if="details" type="button" size="sm" variant="ghost" @click="showPaste = false; installError = ''">
                {{ $t('admin-license-cancel') }}
              </Button>
              <a :href="licenseDashboardUrl" target="_blank" rel="noopener" class="ml-auto text-xs text-accent hover:underline">
                {{ $t('admin-license-paste-where') }}
              </a>
            </div>
          </form>
        </SectionCard>

        <!-- Push -->
        <SectionCard v-if="push">
          <template #title>{{ $t('admin-license-push-title') }}</template>
          <div class="flex flex-col gap-3">
            <p class="text-sm text-secondary">{{ $t('admin-license-push-body') }}</p>

            <div v-if="push.env_managed" class="text-sm text-secondary flex items-center gap-2">
              <Icon name="lock" size="sm" class="text-tertiary" />
              {{ $t('admin-license-push-env-managed', { mode: push.mode }) }}
            </div>
            <SegmentedControl
              v-else
              :model-value="push.mode"
              :options="pushOptions"
              :aria-label="$t('admin-license-push-title')"
              size="sm"
              class="self-start max-w-full"
              @update:model-value="setPushMode"
            />
            <AlertMessage v-if="pushError" type="error" :message="pushError" />

            <div v-if="pushStatus" class="flex flex-col gap-2 rounded-lg bg-surface-alt px-3 py-2.5">
              <div class="flex flex-wrap items-center gap-2">
                <StatusPill :label="pushStatus.label" :tone="pushStatus.tone" />
                <span v-if="relayLastSuccess" class="text-xs text-tertiary">
                  {{ $t('admin-license-relay-last-success', { when: relayLastSuccess }) }}
                </span>
                <Button
                  v-if="push.sender === 'relay'"
                  size="sm"
                  variant="ghost"
                  icon="refresh"
                  class="ml-auto"
                  :loading="retrying || pushSaving"
                  @click="retryRelay"
                >
                  {{ $t('admin-license-relay-retry') }}
                </Button>
              </div>
              <p v-if="pushStatus.hint" class="text-xs text-secondary">
                {{ pushStatus.hint }}
                <a v-if="relayNeedsDashboard" :href="licenseDashboardUrl" target="_blank" rel="noopener" class="text-accent hover:underline">{{ $t('admin-license-open-account') }}</a>
                <a v-else-if="push.sender === 'none' && push.mode !== 'off'" :href="DOCS_URL" target="_blank" rel="noopener" class="text-accent hover:underline">{{ $t('admin-license-docs') }}</a>
              </p>
            </div>

            <p v-if="push.mode === 'native' && !push.native_credentials" class="text-xs text-status-warning">
              {{ $t('admin-license-push-native-missing') }}
            </p>
            <p v-if="push.mode === 'relay'" class="text-xs text-tertiary">
              {{ $t('admin-license-push-relay-disclosure') }}
            </p>
          </div>
        </SectionCard>

        <!-- Offline -->
        <SectionCard>
          <template #title>{{ $t('admin-license-offline-title') }}</template>
          <div class="flex flex-col gap-2">
            <p class="text-sm text-secondary">{{ $t('admin-license-offline-body') }}</p>
            <CodeBlock v-if="overview.instance_id" :code="overview.instance_id" />
          </div>
        </SectionCard>
      </template>
    </div>

    <ConfirmModal
      :show="confirmRemove"
      :title="$t('admin-license-remove-confirm-title')"
      :message="$t('admin-license-remove-confirm-body')"
      :confirm-label="$t('admin-license-remove')"
      variant="danger"
      :loading="removing"
      @confirm="remove"
      @close="confirmRemove = false"
    />
  </div>
</template>
