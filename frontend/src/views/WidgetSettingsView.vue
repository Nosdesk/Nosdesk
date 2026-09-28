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
  },
  { immediate: true },
)

const signedSnippet = computed(() => {
  const url = settings.data.value?.script_url
  return url
    ? [
        '<script>',
        '  window.NosdeskWidget = {',
        "    getToken: () => fetch('/nosdesk-token').then((r) => r.text()),",
        '  }',
        '</scr' + 'ipt>',
        `<script src="${url}" async></scr` + 'ipt>',
      ].join('\n')
    : ''
})
const audience = computed(() => settings.data.value?.token_audience ?? 'https://your-help-portal')
const kid = computed(() => settings.data.value?.secret_kid ?? 'KEY_ID')
const nodeExample = computed(() =>
  [
    '// npm install jsonwebtoken',
    "const jwt = require('jsonwebtoken')",
    "const crypto = require('node:crypto')",
    '',
    "app.get('/nosdesk-token', requireSignIn, (req, res) => {",
    '  const token = jwt.sign(',
    '    {',
    '      sub: String(req.user.id), // your own id for the person (required)',
    '      email: req.user.email,',
    '      name: req.user.name,',
    '      email_verified: req.user.emailVerified, // true to reach an existing account',
    '    },',
    '    process.env.NOSDESK_WIDGET_SECRET,',
    `    { algorithm: 'HS256', expiresIn: '5m', audience: '${audience.value}', keyid: '${kid.value}', jwtid: crypto.randomUUID() },`,
    '  )',
    "  res.type('text/plain').send(token)",
    '})',
  ].join('\n'),
)
const pythonExample = computed(() =>
  [
    '# pip install pyjwt',
    'import secrets, time, jwt',
    '',
    'def nosdesk_token(user):',
    '    now = int(time.time())',
    '    claims = {',
    '        "sub": str(user.id), "email": user.email, "name": user.name,',
    '        "email_verified": user.email_verified,',
    `        "aud": "${audience.value}", "iat": now, "exp": now + 300, "jti": secrets.token_hex(16),`,
    '    }',
    `    return jwt.encode(claims, NOSDESK_WIDGET_SECRET, algorithm="HS256", headers={"kid": "${kid.value}"})`,
  ].join('\n'),
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
        <p class="text-xs text-secondary">{{ t('widget-admin-anonymous-hint') }}</p>
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
        <details v-if="signedSnippet" class="flex flex-col gap-2 text-sm">
          <summary class="cursor-pointer font-medium text-primary">{{ t('widget-admin-signing-title') }}</summary>
          <p class="text-secondary mt-2">{{ t('widget-admin-signing-page') }}</p>
          <pre class="overflow-x-auto rounded-lg bg-surface-alt border border-default p-3 text-xs text-primary">{{ signedSnippet }}</pre>
          <p class="text-secondary mt-2">{{ t('widget-admin-signing-server') }}</p>
          <p class="text-secondary">{{ t('widget-admin-signing-rules') }}</p>
          <pre class="overflow-x-auto rounded-lg bg-surface-alt border border-default p-3 text-xs text-primary">{{ nodeExample }}</pre>
          <pre class="overflow-x-auto rounded-lg bg-surface-alt border border-default p-3 text-xs text-primary">{{ pythonExample }}</pre>
        </details>
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
