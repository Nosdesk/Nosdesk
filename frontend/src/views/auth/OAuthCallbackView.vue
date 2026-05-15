<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import apiClient from '@/services/apiConfig'
import { useMicrosoftAuth } from '@/composables/useMicrosoftAuth'
import AuthCallbackCard, { type ErrorInfo } from '@/components/auth/AuthCallbackCard.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const { handleMicrosoftLogoutAndRetry } = useMicrosoftAuth()

// Provider type from route meta (e.g. 'microsoft', 'oidc')
const provider = computed(
  () => (route.meta.oauthProvider as 'microsoft' | 'local' | 'oidc') || 'oidc',
)
const isMicrosoft = computed(() => provider.value === 'microsoft')
const providerLabel = computed(() => isMicrosoft.value
  ? t('auth-callback-provider-microsoft')
  : t('auth-callback-provider-sso'))

const error = ref<string | null>(null)
const detailedError = ref<string | null>(null)
const loading = ref(true)
const message = ref('')
const showTechnicalDetails = ref(false)

const errorInfo = computed<ErrorInfo | null>(() => {
  if (!error.value) return null

  const errorMsg = error.value.toLowerCase()

  if (errorMsg.includes('already connected') || errorMsg.includes('already linked')) {
    const actions = isMicrosoft.value
      ? [
          { label: t('auth-callback-action-try-different'), action: 'logout_and_retry', primary: true },
          { label: t('auth-callback-action-back-settings'), action: 'settings' },
          { label: t('auth-callback-action-return-login'), action: 'login' }
        ]
      : [{ label: t('auth-callback-action-return-login'), action: 'login', primary: true }]

    return {
      type: 'already_connected',
      title: t('auth-callback-already-title'),
      message: t('auth-callback-already-message', { provider: providerLabel.value }),
      suggestion: isMicrosoft.value
        ? t('auth-callback-already-suggestion-microsoft', { provider: providerLabel.value })
        : t('auth-callback-already-suggestion-generic'),
      icon: 'link',
      actions
    }
  }

  if (errorMsg.includes('not found') || errorMsg.includes('invalid')) {
    return {
      type: 'invalid_request',
      title: t('auth-callback-invalid-title'),
      message: t('auth-callback-invalid-message'),
      suggestion: isMicrosoft.value
        ? t('auth-callback-invalid-suggestion-microsoft', { provider: providerLabel.value })
        : t('auth-callback-invalid-suggestion-generic'),
      icon: 'warning',
      actions: isMicrosoft.value
        ? [
            { label: t('auth-callback-action-try-again'), action: 'retry', primary: true },
            { label: t('auth-callback-action-back-settings'), action: 'settings' }
          ]
        : [{ label: t('auth-callback-action-try-again'), action: 'login', primary: true }]
    }
  }

  return {
    type: 'generic',
    title: t('auth-callback-generic-title'),
    message: error.value,
    suggestion: t('auth-callback-generic-suggestion'),
    icon: 'error',
    actions: [
      { label: isMicrosoft.value ? t('auth-callback-action-try-again') : t('auth-callback-action-return-login'), action: isMicrosoft.value ? 'retry' : 'login', primary: true },
      ...(isMicrosoft.value ? [{ label: t('auth-callback-action-return-login'), action: 'login' }] : [])
    ]
  }
})

const handleAction = (action: string) => {
  switch (action) {
    case 'logout_and_retry':
      handleMicrosoftLogoutAndRetry()
      break
    case 'retry':
    case 'settings':
      router.push('/profile/settings')
      break
    case 'login':
      router.push('/login')
      break
  }
}

onMounted(async () => {
  const code = route.query.code as string | undefined
  const state = route.query.state as string | undefined
  const errorParam = route.query.error as string | undefined
  const errorDescription = route.query.error_description as string | undefined

  if (errorParam) {
    error.value = errorDescription || errorParam
    loading.value = false
    return
  }

  if (!code || !state) {
    error.value = t('auth-callback-error-missing-params')
    const missing = [
      !code ? t('auth-callback-error-missing-field-code') : '',
      !state ? t('auth-callback-error-missing-field-state') : ''
    ].filter(Boolean).join(' ')
    detailedError.value = t('auth-callback-error-missing-detail', { fields: missing })
    loading.value = false
    return
  }

  try {
    message.value = t('auth-callback-loading-processing')

    const response = await apiClient.get('/auth/oauth/callback', {
      params: { code, state }
    })

    const data = response.data

    if (data?.success && data.csrf_token) {
      message.value = t('auth-callback-loading-success')
      loading.value = false

      authStore.setAuthProvider(provider.value)

      if (data.user) {
        authStore.user = data.user
      }

      // Only honour same-origin relative paths from sessionStorage.
      // The pre-auth code stuffs window.location.pathname in there,
      // but sessionStorage is XSS-readable and could be poisoned to
      // bounce users to a phishing site after login. Reject anything
      // that's not a leading-single-slash relative path: protocol-
      // relative ("//attacker.com"), absolute URLs ("https://..."),
      // and javascript: URIs all collapse to '/'.
      const stored = sessionStorage.getItem('authRedirect')
      let redirectPath = '/'
      if (stored && stored.startsWith('/') && !stored.startsWith('//') && !stored.includes('://')) {
        redirectPath = stored
      }
      if (redirectPath.includes(`/auth/${provider.value}/callback`)) {
        redirectPath = '/'
      }
      sessionStorage.removeItem('authRedirect')

      setTimeout(() => router.push(redirectPath), 500)
    } else {
      error.value = t('auth-callback-error-invalid-response')
      detailedError.value = JSON.stringify(data, null, 2)
      loading.value = false
    }
  } catch (err) {
    const axiosError = err as { response?: { status?: number; data?: { message?: string; error?: string } }; request?: unknown; message?: string }

    error.value = axiosError.response?.data?.message ||
                  axiosError.response?.data?.error ||
                  t('auth-callback-error-generic-message')

    if (axiosError.response) {
      detailedError.value = `${t('auth-callback-error-status-prefix', { status: axiosError.response.status ?? '' })}\n${JSON.stringify(axiosError.response.data, null, 2)}`
    } else if (axiosError.request) {
      detailedError.value = t('auth-callback-error-no-response')
    } else {
      detailedError.value = axiosError.message || t('auth-callback-error-unknown')
    }

    loading.value = false
  }
})
</script>

<template>
  <AuthCallbackCard
    :loading="loading"
    :loading-message="message"
    :error="error"
    :error-info="errorInfo"
    :detailed-error="detailedError"
    v-model:show-technical-details="showTechnicalDetails"
    @action="handleAction"
  />
</template>
