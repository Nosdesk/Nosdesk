<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import apiClient from '@/services/apiConfig'
import { useMicrosoftAuth } from '@/composables/useMicrosoftAuth'
import AuthCallbackCard, { type ErrorInfo } from '@/components/auth/AuthCallbackCard.vue'

const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const { handleMicrosoftLogoutAndRetry } = useMicrosoftAuth()

// Provider type from route meta (e.g. 'microsoft', 'oidc')
const provider = computed(() => (route.meta.oauthProvider as string) || 'oidc')
const isMicrosoft = computed(() => provider.value === 'microsoft')
const providerLabel = computed(() => isMicrosoft.value ? 'Microsoft' : 'SSO')

const error = ref<string | null>(null)
const detailedError = ref<string | null>(null)
const loading = ref(true)
const message = ref('Completing sign-in...')
const showTechnicalDetails = ref(false)

const errorInfo = computed<ErrorInfo | null>(() => {
  if (!error.value) return null

  const errorMsg = error.value.toLowerCase()

  if (errorMsg.includes('already connected') || errorMsg.includes('already linked')) {
    const actions = isMicrosoft.value
      ? [
          { label: 'Try a Different Account', action: 'logout_and_retry', primary: true },
          { label: 'Back to Settings', action: 'settings' },
          { label: 'Return to Login', action: 'login' }
        ]
      : [{ label: 'Return to Login', action: 'login', primary: true }]

    return {
      type: 'already_connected',
      title: 'Account Already Connected',
      message: `This ${providerLabel.value} account is already linked to another user in the system.`,
      suggestion: isMicrosoft.value
        ? `Try signing in with a different ${providerLabel.value} account, or contact your administrator.`
        : 'Try signing in with a different account, or contact your administrator.',
      icon: 'link',
      actions
    }
  }

  if (errorMsg.includes('not found') || errorMsg.includes('invalid')) {
    return {
      type: 'invalid_request',
      title: 'Authentication Failed',
      message: 'The authentication request was invalid or has expired.',
      suggestion: isMicrosoft.value
        ? `Please try connecting your ${providerLabel.value} account again.`
        : 'Please try signing in again.',
      icon: 'warning',
      actions: isMicrosoft.value
        ? [
            { label: 'Try Again', action: 'retry', primary: true },
            { label: 'Back to Settings', action: 'settings' }
          ]
        : [{ label: 'Try Again', action: 'login', primary: true }]
    }
  }

  return {
    type: 'generic',
    title: 'Authentication Failed',
    message: error.value,
    suggestion: 'Please try again or contact support if the problem persists.',
    icon: 'error',
    actions: [
      { label: isMicrosoft.value ? 'Try Again' : 'Return to Login', action: isMicrosoft.value ? 'retry' : 'login', primary: true },
      ...(isMicrosoft.value ? [{ label: 'Return to Login', action: 'login' }] : [])
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
    error.value = 'Missing required authentication parameters'
    detailedError.value = `Missing: ${!code ? 'code' : ''} ${!state ? 'state' : ''}`
    loading.value = false
    return
  }

  try {
    message.value = 'Processing authentication...'

    const response = await apiClient.get('/auth/oauth/callback', {
      params: { code, state }
    })

    const data = response.data

    if (data?.success && data.csrf_token) {
      message.value = 'Success! Redirecting...'
      loading.value = false

      authStore.setAuthProvider(provider.value)

      if (data.user) {
        authStore.user = data.user
      }

      let redirectPath = sessionStorage.getItem('authRedirect') || '/'
      if (redirectPath.includes(`/auth/${provider.value}/callback`)) {
        redirectPath = '/'
      }
      sessionStorage.removeItem('authRedirect')

      setTimeout(() => router.push(redirectPath), 500)
    } else {
      error.value = 'Invalid response from server'
      detailedError.value = JSON.stringify(data, null, 2)
      loading.value = false
    }
  } catch (err) {
    const axiosError = err as { response?: { status?: number; data?: { message?: string; error?: string } }; request?: unknown; message?: string }

    error.value = axiosError.response?.data?.message ||
                  axiosError.response?.data?.error ||
                  'An unexpected error occurred during authentication'

    if (axiosError.response) {
      detailedError.value = `Status: ${axiosError.response.status}\n${JSON.stringify(axiosError.response.data, null, 2)}`
    } else if (axiosError.request) {
      detailedError.value = 'No response received from server'
    } else {
      detailedError.value = axiosError.message || 'Unknown error'
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
