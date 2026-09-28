<script setup lang="ts">
/**
 * The Microsoft Teams app: the help portal as a tab in Teams (and Outlook and
 * the Microsoft 365 app), signed in with the workspace's own Entra app. Three
 * steps: prepare the Entra app, turn the tab on, add the package to Teams.
 */
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import { teamsService } from '@nosdesk/core/services/teamsService'

const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useQueryCache()
const KEY = ['admin-teams']
const settings = useQuery({ key: KEY, query: () => teamsService.get() })

const data = computed(() => settings.data.value)
const setup = computed(() => data.value?.setup ?? null)
const saving = ref(false)
const downloading = ref(false)
const error = ref('')

async function toggle(enabled: boolean): Promise<void> {
  saving.value = true
  error.value = ''
  try {
    queryCache.setQueryData(KEY, await teamsService.save(enabled))
    toast.success(enabled ? t('teams-admin-turned-on') : t('teams-admin-turned-off'))
  } catch (e) {
    error.value = extractErrorMessage(e, t('teams-admin-save-failed'))
  } finally {
    saving.value = false
  }
}

async function download(): Promise<void> {
  downloading.value = true
  error.value = ''
  try {
    const blob = await teamsService.downloadPackage()
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = 'teams-app.zip'
    link.click()
    URL.revokeObjectURL(url)
  } catch (e) {
    error.value = extractErrorMessage(e, t('teams-admin-download-failed'))
  } finally {
    downloading.value = false
  }
}

async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
    toast.success(t('widget-admin-copied'))
  } catch {
    // Clipboard blocked: the text is selectable.
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-3xl">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('teams-admin-title') }}</h1>
        <p class="text-secondary">{{ t('teams-admin-description') }}</p>
      </div>

      <AlertMessage v-if="error" type="error" :message="error" />

      <div
        v-if="data && !data.available"
        class="flex flex-col items-start gap-3 bg-surface border border-default rounded-xl p-6"
      >
        <p class="text-sm text-secondary">
          {{ data.provider_kind ? t('teams-admin-needs-entra-other') : t('teams-admin-needs-entra') }}
        </p>
        <RouterLink to="/admin/requester-sign-in" class="text-sm font-medium text-accent hover:underline">
          {{ t('teams-admin-needs-entra-link') }}
        </RouterLink>
      </div>

      <template v-else-if="data && setup">
        <section class="flex flex-col gap-4 bg-surface border border-default rounded-xl p-6">
          <div class="flex flex-col gap-1">
            <h2 class="text-base font-semibold text-primary">{{ t('teams-admin-entra-title') }}</h2>
            <p class="text-sm text-secondary">{{ t('teams-admin-entra-hint', { clientId: data.client_id ?? '' }) }}</p>
          </div>
          <ol class="flex flex-col gap-4 text-sm text-secondary list-decimal pl-5">
            <!-- A flex <li> loses its list number: the layout lives in a div. -->
            <li>
              <div class="flex flex-col gap-2">
                <span>{{ t('teams-admin-entra-redirect') }}</span>
                <div class="flex items-center gap-2">
                  <code class="flex-1 min-w-0 truncate rounded-lg bg-surface-alt border border-default px-3 py-2 text-primary select-all">{{ setup.redirect_uri }}</code>
                  <Button type="button" variant="secondary" size="sm" icon="copy" @click="copy(setup.redirect_uri)">{{ t('widget-admin-copy') }}</Button>
                </div>
              </div>
            </li>
            <li>
              <div class="flex flex-col gap-2">
                <span>{{ t('teams-admin-entra-expose') }}</span>
                <div class="flex items-center gap-2">
                  <code class="flex-1 min-w-0 truncate rounded-lg bg-surface-alt border border-default px-3 py-2 text-primary select-all">{{ setup.application_id_uri }}</code>
                  <Button type="button" variant="secondary" size="sm" icon="copy" @click="copy(setup.application_id_uri)">{{ t('widget-admin-copy') }}</Button>
                </div>
                <div class="flex items-center gap-2">
                  <code class="flex-1 min-w-0 truncate rounded-lg bg-surface-alt border border-default px-3 py-2 text-primary select-all">{{ setup.scope }}</code>
                  <Button type="button" variant="secondary" size="sm" icon="copy" @click="copy(setup.scope)">{{ t('widget-admin-copy') }}</Button>
                </div>
              </div>
            </li>
            <li>{{ t('teams-admin-entra-consent') }}</li>
          </ol>
        </section>

        <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-6">
          <h2 class="text-base font-semibold text-primary">{{ t('teams-admin-enable-title') }}</h2>
          <ToggleSwitch
            :model-value="data.enabled"
            :label="t('teams-admin-enable-label')"
            :disabled="saving"
            @update:model-value="toggle"
          />
          <p v-if="data.allowed_domains.length" class="text-sm text-secondary">
            {{ t('teams-admin-domains', { domains: data.allowed_domains.join(', ') }) }}
          </p>
          <p v-else class="text-sm text-status-warning">{{ t('teams-admin-no-domains') }}</p>
        </section>

        <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-6">
          <div class="flex flex-col gap-1">
            <h2 class="text-base font-semibold text-primary">{{ t('teams-admin-package-title') }}</h2>
            <p class="text-sm text-secondary">{{ t('teams-admin-package-hint') }}</p>
          </div>
          <ol class="flex flex-col gap-2 text-sm text-secondary list-decimal pl-5">
            <li>{{ t('teams-admin-package-upload') }}</li>
            <li>{{ t('teams-admin-package-pin') }}</li>
            <li>{{ t('teams-admin-package-updates', { version: data.package_version }) }}</li>
          </ol>
          <div class="flex">
            <Button type="button" icon="download" :loading="downloading" @click="download">
              {{ t('teams-admin-package-download') }}
            </Button>
          </div>
        </section>
      </template>
    </div>
  </div>
</template>
