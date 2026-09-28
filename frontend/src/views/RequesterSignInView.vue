<script setup lang="ts">
/**
 * Requester sign-in: let people sign in to the portal with their work account
 * (Microsoft Entra ID, Google Workspace, or on self-hosted any OpenID Connect
 * provider). Emailed sign-in links keep working for everyone else.
 */
import { computed, ref, watch } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import {
  entraTenant,
  requesterSsoService,
  type RequesterSsoKind,
} from '@nosdesk/core/services/requesterSsoService'

const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useQueryCache()
const KEY = ['requester-sso']
const state = useQuery({ key: KEY, query: () => requesterSsoService.get() })

const kind = ref<RequesterSsoKind>('entra')
const tenantId = ref('')
const issuerUrl = ref('')
const clientId = ref('')
const clientSecret = ref('')
const domains = ref('')
const displayName = ref('')
const enabled = ref(false)
const saving = ref(false)
const error = ref('')

const provider = computed(() => state.data.value?.provider ?? null)
// A saved secret belongs to the saved app: pointing at another app drops it.
const secretKept = computed(() => {
  const p = provider.value
  if (!p?.has_client_secret) return false
  const sameIssuer =
    p.kind === 'entra'
      ? entraTenant(p.issuer_url) === tenantId.value.trim()
      : p.kind !== 'oidc' || p.issuer_url === issuerUrl.value.trim()
  return p.kind === kind.value && p.client_id === clientId.value.trim() && sameIssuer
})
const kindOptions = computed(() => [
  { value: 'entra' as const, label: t('requester-sso-kind-entra') },
  { value: 'google' as const, label: t('requester-sso-kind-google') },
  ...(state.data.value?.generic_oidc_allowed
    ? [{ value: 'oidc' as const, label: t('requester-sso-kind-oidc') }]
    : []),
])

watch(
  provider,
  (p) => {
    if (!p) return
    kind.value = p.kind
    tenantId.value = p.kind === 'entra' ? entraTenant(p.issuer_url) : ''
    issuerUrl.value = p.kind === 'oidc' ? p.issuer_url : ''
    clientId.value = p.client_id
    domains.value = p.allowed_domains.join(', ')
    displayName.value = p.display_name
    enabled.value = p.enabled
  },
  { immediate: true },
)

async function save(): Promise<void> {
  saving.value = true
  error.value = ''
  try {
    await requesterSsoService.save({
      kind: kind.value,
      display_name: displayName.value.trim() || null,
      tenant_id: kind.value === 'entra' ? tenantId.value.trim() : null,
      issuer_url: kind.value === 'oidc' ? issuerUrl.value.trim() : null,
      client_id: clientId.value.trim(),
      client_secret: clientSecret.value.trim() || null,
      allowed_domains: domains.value.split(/[\s,]+/).filter(Boolean),
      enabled: enabled.value,
    })
    clientSecret.value = ''
    await queryCache.invalidateQueries({ key: KEY })
    toast.success(t('requester-sso-saved'))
  } catch (e) {
    error.value = extractErrorMessage(e, t('requester-sso-save-failed'))
  } finally {
    saving.value = false
  }
}

async function remove(): Promise<void> {
  saving.value = true
  try {
    await requesterSsoService.remove()
    await queryCache.invalidateQueries({ key: KEY })
    toast.success(t('requester-sso-removed'))
  } catch (e) {
    error.value = extractErrorMessage(e, t('requester-sso-save-failed'))
  } finally {
    saving.value = false
  }
}

async function copyRedirect(): Promise<void> {
  const uri = state.data.value?.redirect_uri
  if (!uri) return
  try {
    await navigator.clipboard.writeText(uri)
    toast.success(t('requester-sso-copied'))
  } catch {
    // Clipboard blocked: the field is selectable.
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-3xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('requester-sso-title') }}</h1>
        <p class="text-secondary">{{ t('requester-sso-description') }}</p>
      </div>

      <AlertMessage v-if="error" type="error" :message="error" />

      <form
        v-if="state.data.value"
        class="flex flex-col gap-5 bg-surface border border-default rounded-xl p-6"
        @submit.prevent="save"
      >
        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('requester-sso-kind-label') }}</span>
          <SegmentedControl v-model="kind" :options="kindOptions" :aria-label="t('requester-sso-kind-label')" />
        </div>

        <FormInput
          v-if="kind === 'entra'"
          v-model="tenantId"
          :label="t('requester-sso-tenant-label')"
          :description="t('requester-sso-tenant-hint')"
          required
        />
        <FormInput
          v-if="kind === 'oidc'"
          v-model="issuerUrl"
          :label="t('requester-sso-issuer-label')"
          placeholder="https://id.example.com/realms/staff"
          required
        />
        <FormInput v-model="clientId" :label="t('requester-sso-client-id-label')" required />
        <FormInput
          v-model="clientSecret"
          type="password"
          autocomplete="off"
          :label="t('requester-sso-client-secret-label')"
          :description="t('requester-sso-client-secret-hint')"
          :placeholder="secretKept ? t('requester-sso-client-secret-kept') : ''"
        />
        <FormInput
          v-model="domains"
          :label="t('requester-sso-domains-label')"
          :description="t('requester-sso-domains-hint')"
          placeholder="acme.com, acme.co.uk"
          required
        />
        <FormInput
          v-model="displayName"
          :label="t('requester-sso-button-label')"
          :placeholder="kind === 'entra' ? 'Microsoft' : kind === 'google' ? 'Google' : ''"
        />

        <div v-if="state.data.value.redirect_uri" class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ t('requester-sso-redirect-label') }}</span>
          <div class="flex items-center gap-2">
            <code class="flex-1 min-w-0 truncate rounded-lg bg-surface-alt border border-default px-3 py-2 text-sm text-primary select-all">
              {{ state.data.value.redirect_uri }}
            </code>
            <Button type="button" variant="secondary" size="sm" icon="copy" @click="copyRedirect">
              {{ t('requester-sso-copy') }}
            </Button>
          </div>
          <p class="text-xs text-tertiary">{{ t('requester-sso-redirect-hint') }}</p>
        </div>

        <ToggleSwitch
          v-model="enabled"
          :label="t('requester-sso-enabled-label')"
          :description="t('requester-sso-enabled-hint')"
        />

        <div class="flex items-center gap-2">
          <Button v-if="provider" type="button" variant="ghost" :disabled="saving" @click="remove">
            {{ t('requester-sso-remove') }}
          </Button>
          <Button type="submit" class="ml-auto" :loading="saving">{{ t('requester-sso-save') }}</Button>
        </div>
      </form>
    </div>
  </div>
</template>
