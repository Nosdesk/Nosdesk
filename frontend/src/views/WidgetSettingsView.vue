<script setup lang="ts">
/**
 * The embeddable help widget: which sites may show it, whether visitors who
 * aren't signed in get help articles and a request form, and the secret a
 * site signs its visitors in with.
 */
import { computed, ref, watch } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import { widgetService } from '@nosdesk/core/services/widgetService'

const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useQueryCache()
const KEY = ['admin-widget']
const settings = useQuery({ key: KEY, query: () => widgetService.get() })

const enabled = ref(false)
const origins = ref('')
const allowAnonymous = ref(true)
const saving = ref(false)
const error = ref('')
const newSecret = ref('')
const rotating = ref(false)

watch(
  () => settings.data.value,
  (s) => {
    if (!s) return
    enabled.value = s.enabled
    origins.value = s.allowed_origins.join('\n')
    allowAnonymous.value = s.allow_anonymous
  },
  { immediate: true },
)

const snippet = computed(() => {
  const url = settings.data.value?.script_url
  return url ? `<script src="${url}" async></scr` + `ipt>` : ''
})

async function save(): Promise<void> {
  saving.value = true
  error.value = ''
  try {
    await widgetService.save({
      enabled: enabled.value,
      allowed_origins: origins.value.split(/[\s,]+/).filter(Boolean),
      allow_anonymous: allowAnonymous.value,
    })
    await queryCache.invalidateQueries({ key: KEY })
    toast.success(t('widget-admin-saved'))
  } catch (e) {
    error.value = extractErrorMessage(e, t('widget-admin-save-failed'))
  } finally {
    saving.value = false
  }
}

async function rotate(): Promise<void> {
  rotating.value = true
  try {
    newSecret.value = await widgetService.rotateSecret()
    await queryCache.invalidateQueries({ key: KEY })
  } catch (e) {
    error.value = extractErrorMessage(e, t('widget-admin-save-failed'))
  } finally {
    rotating.value = false
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
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('widget-admin-title') }}</h1>
        <p class="text-secondary">{{ t('widget-admin-description') }}</p>
      </div>

      <AlertMessage v-if="error" type="error" :message="error" />

      <form v-if="settings.data.value" class="flex flex-col gap-5 bg-surface border border-default rounded-xl p-6" @submit.prevent="save">
        <ToggleSwitch v-model="enabled" :label="t('widget-admin-enabled-label')" />
        <FormTextarea
          v-model="origins"
          :label="t('widget-admin-origins-label')"
          :description="t('widget-admin-origins-hint')"
          placeholder="https://www.acme.com"
          :rows="3"
        />
        <ToggleSwitch
          v-model="allowAnonymous"
          :label="t('widget-admin-anonymous-label')"
          :description="t('widget-admin-anonymous-hint')"
        />
        <div class="flex">
          <Button type="submit" class="ml-auto" :loading="saving">{{ t('widget-admin-save') }}</Button>
        </div>
      </form>

      <section v-if="snippet" class="flex flex-col gap-2 bg-surface border border-default rounded-xl p-6">
        <h2 class="text-base font-semibold text-primary">{{ t('widget-admin-install-title') }}</h2>
        <p class="text-sm text-secondary">{{ t('widget-admin-install-hint') }}</p>
        <div class="flex items-center gap-2">
          <code class="flex-1 min-w-0 truncate rounded-lg bg-surface-alt border border-default px-3 py-2 text-sm text-primary select-all">{{ snippet }}</code>
          <Button type="button" variant="secondary" size="sm" icon="copy" @click="copy(snippet)">{{ t('widget-admin-copy') }}</Button>
        </div>
      </section>

      <section v-if="settings.data.value" class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-6">
        <h2 class="text-base font-semibold text-primary">{{ t('widget-admin-secret-title') }}</h2>
        <p class="text-sm text-secondary">{{ t('widget-admin-secret-hint') }}</p>
        <div v-if="newSecret" class="flex flex-col gap-2">
          <p class="text-sm font-medium text-status-warning">{{ t('widget-admin-secret-once') }}</p>
          <div class="flex items-center gap-2">
            <code class="flex-1 min-w-0 truncate rounded-lg bg-surface-alt border border-default px-3 py-2 text-sm text-primary select-all">{{ newSecret }}</code>
            <Button type="button" variant="secondary" size="sm" icon="copy" @click="copy(newSecret)">{{ t('widget-admin-copy') }}</Button>
          </div>
        </div>
        <div class="flex items-center gap-3">
          <span class="text-sm text-secondary flex-1">
            {{ settings.data.value.has_secret ? t('widget-admin-secret-set') : t('widget-admin-secret-none') }}
          </span>
          <Button type="button" variant="secondary" :loading="rotating" @click="rotate">
            {{ settings.data.value.has_secret ? t('widget-admin-secret-rotate') : t('widget-admin-secret-generate') }}
          </Button>
        </div>
      </section>
    </div>
  </div>
</template>
