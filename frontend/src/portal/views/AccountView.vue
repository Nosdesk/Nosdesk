<script setup lang="ts">
/** The signed-in person's own account: their profile, how they sign in, and where they're signed in. */
import { computed, ref, watch } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SessionsSettings from '@/components/settings/SessionsSettings.vue'
import { useTimezoneOptions } from '@/composables/useTimezoneOptions'
import { SUPPORTED_LOCALES } from '@/i18n'
import { extractErrorMessage } from '@/utils/errors'
import { useDateStore } from '@nosdesk/core/stores/dateStore'

import AuthenticatorSection from '../components/account/AuthenticatorSection.vue'
import PasskeysSection from '../components/account/PasskeysSection.vue'
import PasswordSection from '../components/account/PasswordSection.vue'
import PortalLayout from '../components/PortalLayout.vue'
import { usePortalMe } from '../usePortalMe'
import { getAccount, updateAccount, type PortalAccount } from '../service'

const { $t: t } = useFluent()
const queryCache = useQueryCache()
const dateStore = useDateStore()
// A link's session has no account to manage: the layout offers sign-in instead.
const { linkSession, fullSession } = usePortalMe()
const account = useQuery({
  key: ['portal', 'account'],
  query: getAccount,
  enabled: () => fullSession.value,
})

// 'system' stands for "this device's timezone", stored as no choice.
const DEVICE = 'system'
const name = ref('')
const pronouns = ref('')
const locale = ref('')
const timezone = ref(DEVICE)
function seed(a: PortalAccount): void {
  name.value = a.name
  pronouns.value = a.pronouns ?? ''
  locale.value = a.locale ?? ''
  timezone.value = a.timezone || DEVICE
}
watch(
  () => account.data.value,
  (a) => {
    if (a) seed(a)
  },
  { immediate: true },
)

const localeOptions = computed(() => [
  { value: '', label: t('settings-locale-site-default') },
  ...SUPPORTED_LOCALES.map((code) => ({ value: code, label: t(`settings-locale-${code}`) || code })),
])
const ianaOptions = useTimezoneOptions()
const timezoneOptions = computed<DropdownOption[]>(() => [
  { value: DEVICE, label: t('settings-timezone-use-device'), description: dateStore.browserTimezone },
  ...ianaOptions.value,
])

const dirty = computed(() => {
  const a = account.data.value
  if (!a) return false
  return (
    name.value.trim() !== a.name ||
    pronouns.value.trim() !== (a.pronouns ?? '') ||
    locale.value !== (a.locale ?? '') ||
    timezone.value !== (a.timezone || DEVICE)
  )
})
const canSave = computed(() => dirty.value && (!account.data.value?.sections.name || name.value.trim() !== ''))

const saving = ref(false)
const message = ref<{ tone: 'success' | 'error'; text: string } | null>(null)
async function save(): Promise<void> {
  const a = account.data.value
  if (!a || !canSave.value) return
  saving.value = true
  message.value = null
  try {
    await updateAccount({
      ...(a.sections.name && name.value.trim() !== a.name ? { name: name.value.trim() } : {}),
      pronouns: pronouns.value.trim(),
      locale: locale.value,
      timezone: timezone.value === DEVICE ? '' : timezone.value,
    })
    await queryCache.invalidateQueries({ key: ['portal', 'account'] })
    // The layout reads the name and language from here.
    void queryCache.invalidateQueries({ key: ['portal', 'me'] })
    dateStore.setUserTimezone(timezone.value)
    message.value = { tone: 'success', text: t('portal-account-saved') }
  } catch (e) {
    message.value = { tone: 'error', text: extractErrorMessage(e, t('portal-account-save-failed')) }
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <PortalLayout>
    <header class="flex flex-col gap-1">
      <h1 class="text-2xl font-semibold text-primary">{{ t('portal-account-title') }}</h1>
      <p class="text-sm text-secondary">{{ t('portal-account-intro') }}</p>
    </header>

    <p
      v-if="message"
      :role="message.tone === 'error' ? 'alert' : 'status'"
      class="text-sm"
      :class="message.tone === 'error' ? 'text-status-error' : 'text-status-success'"
    >
      {{ message.text }}
    </p>

    <template v-if="linkSession" />
    <p v-else-if="account.error.value && !account.data.value" role="alert" class="text-sm text-status-error">
      {{ t('portal-account-load-failed') }}
    </p>

    <template v-else-if="account.data.value">
      <SectionCard content-padding="p-4 sm:p-6">
        <template #title>{{ t('portal-account-profile-title') }}</template>
        <form class="flex flex-col gap-5" @submit.prevent="save">
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <FormInput
              v-if="account.data.value.sections.name"
              v-model="name"
              :label="t('portal-account-name-label')"
              autocomplete="name"
              required
              size="sm"
              :disabled="saving"
            />
            <div v-else class="flex flex-col gap-1.5">
              <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
                {{ t('portal-account-name-label') }}
              </span>
              <span class="text-sm text-primary">{{ account.data.value.name }}</span>
            </div>
            <FormInput
              v-model="pronouns"
              :label="t('settings-profile-pronouns-label')"
              :placeholder="t('settings-profile-pronouns-placeholder')"
              size="sm"
              :disabled="saving"
            />
            <div class="flex flex-col gap-1.5">
              <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
                {{ t('settings-language-label') }}
              </span>
              <BaseDropdown
                v-model="locale"
                :options="localeOptions"
                :disabled="saving"
                :placeholder="t('settings-language-label')"
                size="sm"
              />
            </div>
            <div class="flex flex-col gap-1.5">
              <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
                {{ t('settings-timezone-label') }}
              </span>
              <SearchableDropdown
                v-model="timezone"
                :options="timezoneOptions"
                :disabled="saving"
                :aria-label="t('settings-timezone-label')"
                :placeholder="t('settings-timezone-label')"
                :search-placeholder="t('settings-timezone-search-placeholder')"
                :empty-message="t('settings-timezone-no-matches')"
                size="sm"
              />
            </div>
          </div>
          <p v-if="account.data.value.email" class="text-sm text-secondary">
            {{ t('portal-account-email', { email: account.data.value.email }) }}
          </p>
          <div class="flex justify-end">
            <Button type="submit" :disabled="!canSave" :loading="saving">{{ t('settings-save') }}</Button>
          </div>
        </form>
      </SectionCard>

      <!-- How they sign in, where the server says it's theirs to manage. -->
      <PasswordSection
        v-if="account.data.value.sections.password"
        @success="(text: string) => (message = { tone: 'success', text })"
        @error="(text: string) => (message = { tone: 'error', text })"
      />
      <AuthenticatorSection
        v-if="account.data.value.sections.authenticator"
        @success="(text: string) => (message = { tone: 'success', text })"
        @error="(text: string) => (message = { tone: 'error', text })"
      />
      <PasskeysSection
        v-if="account.data.value.sections.passkeys"
        @success="(text: string) => (message = { tone: 'success', text })"
        @error="(text: string) => (message = { tone: 'error', text })"
      />

      <SessionsSettings
        @success="(text: string) => (message = { tone: 'success', text })"
        @error="(text: string) => (message = { tone: 'error', text })"
      />
    </template>
  </PortalLayout>
</template>
